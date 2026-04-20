//! `data_sync` tool — executes a data link: reads from source, applies
//! field mappings, writes to target.
//!
//! Data flows mechanically through the sync engine — the LLM never sees
//! record contents. Only sync summaries (counts, errors) are returned.

use agentzero_connectors::registry::ConnectorRegistry;
use agentzero_connectors::sync_engine;
use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct DataSyncInput {
    /// ID of the data link to sync.
    link_id: String,
    /// If true, perform a dry run without writing to target.
    #[serde(default)]
    dry_run: bool,
}

#[tool(
    name = "data_sync",
    description = "Execute a data link: read records from the source connector, apply field mappings, and write to the target connector. Returns a sync summary with record counts and any errors. Data flows mechanically — no LLM reasoning on record contents."
)]
pub struct DataSyncTool {
    registry: Arc<RwLock<ConnectorRegistry>>,
}

impl DataSyncTool {
    pub fn new(registry: Arc<RwLock<ConnectorRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for DataSyncTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(DataSyncInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: DataSyncInput = serde_json::from_str(input)?;
        let registry = self.registry.read().await;

        let link = registry
            .link(&parsed.link_id)
            .ok_or_else(|| anyhow::anyhow!("link `{}` not found", parsed.link_id))?
            .clone();

        // Pre-flight validation: check all mapped fields still exist.
        let validation_errors = registry.validate_link_mappings(&link);
        if !validation_errors.is_empty() {
            return Ok(ToolResult {
                output: serde_json::to_string_pretty(&serde_json::json!({
                    "error": "pre-flight validation failed",
                    "link_id": parsed.link_id,
                    "validation_errors": validation_errors,
                    "hint": "Run connector_discover on the affected connectors to check for schema drift, then update the link's field_mappings.",
                }))?,
            });
        }

        // Look up the primary key for the target entity.
        let target_primary_key = registry
            .manifest(&link.target.connector)
            .and_then(|m| {
                m.entities
                    .iter()
                    .find(|e| e.name == link.target.entity)
                    .map(|e| e.primary_key.clone())
            })
            .unwrap_or_else(|| "id".to_string());

        // Read records from source, transform, write to target — in batches.
        let batch_size = 100u32;
        let mut total_read = 0u64;
        let mut total_written = 0u64;
        let mut total_skipped = 0u64;
        let mut all_errors: Vec<agentzero_connectors::SyncError> = Vec::new();
        let mut cursor = link.last_sync_cursor.clone();

        loop {
            // Read a batch from the source.
            let read_request = agentzero_connectors::templates::ReadRequest {
                entity: link.source.entity.clone(),
                cursor: cursor.clone(),
                batch_size,
            };

            let read_result = registry
                .read_records(&link.source.connector, &read_request)
                .await?;

            let batch_count = read_result.records.len() as u64;
            total_read += batch_count;

            if read_result.records.is_empty() {
                break;
            }

            // Apply field mappings.
            let (transformed, transform_errors) =
                sync_engine::transform_batch(&link, &read_result.records);
            all_errors.extend(transform_errors);

            // Write to target (unless dry run).
            if !parsed.dry_run && !transformed.is_empty() {
                let write_request = agentzero_connectors::templates::WriteRequest {
                    entity: link.target.entity.clone(),
                    records: transformed,
                    primary_key: target_primary_key.clone(),
                };

                let write_result = registry
                    .write_records(&link.target.connector, &write_request)
                    .await?;

                total_written += write_result.written;
                total_skipped += write_result.skipped;
                all_errors.extend(write_result.errors);
            }

            // Advance cursor.
            cursor = read_result.next_cursor;
            if cursor.is_none() {
                break;
            }
        }

        drop(registry);

        // Update cursor in registry after sync and persist to encrypted store.
        if !parsed.dry_run {
            let mut registry = self.registry.write().await;
            if let Some(link_mut) = registry.link_mut(&parsed.link_id) {
                if let Some(ref c) = cursor {
                    link_mut.last_sync_cursor = Some(c.clone());
                }
                link_mut.last_sync_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
            }
            registry.persist_sync_state();
        }

        let result = sync_engine::build_result(
            &parsed.link_id,
            total_read,
            total_written,
            total_skipped,
            all_errors,
            cursor,
        );

        let response = serde_json::json!({
            "link_id": result.link_id,
            "dry_run": parsed.dry_run,
            "records_read": result.records_read,
            "records_written": result.records_written,
            "records_skipped": result.records_skipped,
            "records_failed": result.records_failed,
            "errors": result.errors,
            "cursor": result.cursor,
        });

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&response)?,
        })
    }
}
