//! `data_link` tool — CRUD operations on data links between connectors.
//!
//! Allows the AI agent to create, list, update, and delete data links.
//! When creating with `auto_map: true`, returns both source and target
//! schemas so the agent can propose field mappings using LLM reasoning.

use agentzero_connectors::registry::ConnectorRegistry;
use agentzero_connectors::{DataEndpoint, DataLink, FieldMapping, SyncMode};
use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct DataLinkInput {
    /// Operation: "create", "list", "get", "update", "delete", "validate"
    action: String,
    /// Link ID (required for get, update, delete)
    #[serde(default)]
    id: Option<String>,
    /// Link name (for create)
    #[serde(default)]
    name: Option<String>,
    /// Source connector name (for create)
    #[serde(default)]
    source_connector: Option<String>,
    /// Source entity name (for create)
    #[serde(default)]
    source_entity: Option<String>,
    /// Target connector name (for create)
    #[serde(default)]
    target_connector: Option<String>,
    /// Target entity name (for create)
    #[serde(default)]
    target_entity: Option<String>,
    /// Field mappings as JSON array (for create/update)
    #[serde(default)]
    field_mappings: Option<serde_json::Value>,
    /// If true, return both schemas for AI-assisted mapping (for create)
    #[serde(default)]
    auto_map: bool,
    /// Sync mode: "on_demand" (default), "scheduled" (requires cron field), "event_driven" (requires event_topic field)
    #[serde(default)]
    sync_mode: Option<String>,
    /// Cron expression for scheduled sync (e.g. "0 */15 * * *")
    #[serde(default)]
    cron: Option<String>,
    /// EventBus topic for event-driven sync
    #[serde(default)]
    event_topic: Option<String>,
}

#[tool(
    name = "data_link",
    description = "Create, list, update, or delete data links between connectors. A data link defines how data flows from a source connector entity to a target connector entity with field mappings. Use action='create' with auto_map=true to get both schemas for AI-assisted field mapping."
)]
pub struct DataLinkTool {
    registry: Arc<RwLock<ConnectorRegistry>>,
}

impl DataLinkTool {
    pub fn new(registry: Arc<RwLock<ConnectorRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for DataLinkTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(DataLinkInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: DataLinkInput = serde_json::from_str(input)?;

        match parsed.action.as_str() {
            "create" => self.create_link(parsed, ctx).await,
            "list" => self.list_links().await,
            "get" => self.get_link(parsed).await,
            "update" => self.update_link(parsed).await,
            "delete" => self.delete_link(parsed).await,
            "validate" => self.validate_link(parsed).await,
            other => Ok(ToolResult {
                output: format!(
                    "Unknown action `{other}`. Valid actions: create, list, get, update, delete, validate"
                ),
            }),
        }
    }
}

impl DataLinkTool {
    async fn create_link(
        &self,
        input: DataLinkInput,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let source_connector = input
            .source_connector
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("source_connector is required"))?;
        let source_entity = input
            .source_entity
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("source_entity is required"))?;
        let target_connector = input
            .target_connector
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("target_connector is required"))?;
        let target_entity = input
            .target_entity
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("target_entity is required"))?;

        let mut registry = self.registry.write().await;

        // If auto_map is true, discover both schemas and return them
        // for the agent to propose field mappings.
        if input.auto_map {
            let source_schemas = if registry.manifest(source_connector).is_some() {
                registry
                    .manifest(source_connector)
                    .map(|m| m.entities.clone())
                    .unwrap_or_default()
            } else {
                // Try discovering if not cached.
                match registry.discover(source_connector).await {
                    Ok((entities, _)) => entities,
                    Err(e) => {
                        return Ok(ToolResult {
                            output: format!("Failed to discover source `{source_connector}`: {e}"),
                        })
                    }
                }
            };

            let target_schemas = if registry.manifest(target_connector).is_some() {
                registry
                    .manifest(target_connector)
                    .map(|m| m.entities.clone())
                    .unwrap_or_default()
            } else {
                match registry.discover(target_connector).await {
                    Ok((entities, _)) => entities,
                    Err(e) => {
                        return Ok(ToolResult {
                            output: format!("Failed to discover target `{target_connector}`: {e}"),
                        })
                    }
                }
            };

            let source_entity_schema = source_schemas.iter().find(|e| e.name == source_entity);
            let target_entity_schema = target_schemas.iter().find(|e| e.name == target_entity);

            let response = serde_json::json!({
                "action": "auto_map",
                "message": "Here are both schemas. Please propose field_mappings as an array of {source_field, target_field, transform?} objects, then call data_link with action='create' and the mappings.",
                "source": {
                    "connector": source_connector,
                    "entity": source_entity,
                    "schema": source_entity_schema,
                },
                "target": {
                    "connector": target_connector,
                    "entity": target_entity,
                    "schema": target_entity_schema,
                },
            });

            return Ok(ToolResult {
                output: serde_json::to_string_pretty(&response)?,
            });
        }

        // Parse field mappings.
        let field_mappings: Vec<FieldMapping> = input
            .field_mappings
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let link_id = uuid::Uuid::new_v4().to_string();
        let link_name = input.name.unwrap_or_else(|| {
            format!("{source_connector}_{source_entity}_to_{target_connector}_{target_entity}")
        });

        let link = DataLink {
            id: link_id.clone(),
            name: link_name.clone(),
            source: DataEndpoint {
                connector: source_connector.to_string(),
                entity: source_entity.to_string(),
            },
            target: DataEndpoint {
                connector: target_connector.to_string(),
                entity: target_entity.to_string(),
            },
            field_mappings,
            sync_mode: match input.sync_mode.as_deref() {
                Some("scheduled") => {
                    let cron_expr = input
                        .cron
                        .clone()
                        .unwrap_or_else(|| "0 * * * *".to_string());
                    SyncMode::Scheduled { cron: cron_expr }
                }
                Some("event_driven") => {
                    let topic = input.event_topic.clone().unwrap_or_else(|| {
                        format!("connector:{source_connector}:{source_entity}:changed")
                    });
                    SyncMode::EventDriven { event_topic: topic }
                }
                _ => SyncMode::OnDemand,
            },
            transform: None,
            last_sync_cursor: None,
            last_sync_at: 0,
        };

        // Validate mappings if schemas are available.
        let validation_errors = registry.validate_link_mappings(&link);

        // Register cron job for scheduled syncs.
        #[cfg(feature = "tools-extended")]
        if let SyncMode::Scheduled { ref cron } = link.sync_mode {
            let cron_id = format!("data_sync_{}", link_id);
            let cron_command = format!(
                "Run data_sync with link_id={} to sync data from {}.{} to {}.{}",
                link_id, source_connector, source_entity, target_connector, target_entity
            );
            let cron_dir = std::path::PathBuf::from(&ctx.workspace_root).join(".agentzero");
            match crate::cron_store::CronStore::new(&cron_dir) {
                Ok(store) => match store.add(&cron_id, cron, &cron_command) {
                    Ok(_) => {
                        tracing::info!(cron_id = %cron_id, schedule = %cron, "registered scheduled sync")
                    }
                    Err(e) => {
                        tracing::warn!(cron_id = %cron_id, error = %e, "failed to register scheduled sync")
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, "failed to open cron store for scheduled sync")
                }
            }
        }

        registry.upsert_link(link);

        let mut response = serde_json::json!({
            "created": true,
            "link_id": link_id,
            "name": link_name,
        });

        if !validation_errors.is_empty() {
            response["warnings"] = serde_json::json!(validation_errors);
        }

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&response)?,
        })
    }

    async fn list_links(&self) -> anyhow::Result<ToolResult> {
        let registry = self.registry.read().await;
        let links: Vec<serde_json::Value> = registry
            .links()
            .values()
            .map(|link| {
                serde_json::json!({
                    "id": link.id,
                    "name": link.name,
                    "source": format!("{}.{}", link.source.connector, link.source.entity),
                    "target": format!("{}.{}", link.target.connector, link.target.entity),
                    "mappings": link.field_mappings.len(),
                    "sync_mode": serde_json::to_value(&link.sync_mode).unwrap_or_default(),
                    "last_sync_at": link.last_sync_at,
                })
            })
            .collect();

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&serde_json::json!({
                "links": links,
                "total": links.len(),
            }))?,
        })
    }

    async fn get_link(&self, input: DataLinkInput) -> anyhow::Result<ToolResult> {
        let id = input
            .id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("id is required for get"))?;

        let registry = self.registry.read().await;
        match registry.link(id) {
            Some(link) => Ok(ToolResult {
                output: serde_json::to_string_pretty(link)?,
            }),
            None => Ok(ToolResult {
                output: format!("Link `{id}` not found"),
            }),
        }
    }

    async fn update_link(&self, input: DataLinkInput) -> anyhow::Result<ToolResult> {
        let id = input
            .id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("id is required for update"))?;

        let mut registry = self.registry.write().await;
        let link = registry
            .link_mut(id)
            .ok_or_else(|| anyhow::anyhow!("link `{id}` not found"))?;

        if let Some(name) = input.name {
            link.name = name;
        }
        if let Some(mappings_val) = input.field_mappings {
            let mappings: Vec<FieldMapping> = serde_json::from_value(mappings_val)?;
            link.field_mappings = mappings;
        }

        Ok(ToolResult {
            output: serde_json::json!({"updated": true, "link_id": id}).to_string(),
        })
    }

    async fn delete_link(&self, input: DataLinkInput) -> anyhow::Result<ToolResult> {
        let id = input
            .id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("id is required for delete"))?;

        let mut registry = self.registry.write().await;
        match registry.remove_link(id) {
            Some(_) => Ok(ToolResult {
                output: serde_json::json!({"deleted": true, "link_id": id}).to_string(),
            }),
            None => Ok(ToolResult {
                output: format!("Link `{id}` not found"),
            }),
        }
    }

    async fn validate_link(&self, input: DataLinkInput) -> anyhow::Result<ToolResult> {
        let id = input
            .id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("id is required for validate"))?;

        let registry = self.registry.read().await;
        let link = registry
            .link(id)
            .ok_or_else(|| anyhow::anyhow!("link `{id}` not found"))?;

        let errors = registry.validate_link_mappings(link);

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&serde_json::json!({
                "link_id": id,
                "valid": errors.is_empty(),
                "errors": errors,
            }))?,
        })
    }
}
