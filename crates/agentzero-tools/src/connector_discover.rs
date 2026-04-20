//! `connector_discover` tool — introspects a configured data source and
//! returns its entity schemas.
//!
//! Returns only schema metadata (field names, types, constraints) — never
//! sample data. Safe for LLM reasoning without PII exposure.

use agentzero_connectors::registry::ConnectorRegistry;
use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct ConnectorDiscoverInput {
    /// Name of the connector to discover (must be configured in agentzero.toml).
    connector: String,
}

#[tool(
    name = "connector_discover",
    description = "Discover the schema of a configured data source. Returns entity names, field names/types, and primary keys. Use this to understand what data a connector exposes before creating data links."
)]
pub struct ConnectorDiscoverTool {
    registry: Arc<RwLock<ConnectorRegistry>>,
}

impl ConnectorDiscoverTool {
    pub fn new(registry: Arc<RwLock<ConnectorRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for ConnectorDiscoverTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(ConnectorDiscoverInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: serde_json::Value = serde_json::from_str(input)
            .unwrap_or_else(|_| serde_json::json!({"connector": input.trim()}));

        let connector_name = parsed
            .get("connector")
            .and_then(|v| v.as_str())
            .unwrap_or(input.trim());

        let mut registry = self.registry.write().await;

        // Check the connector exists.
        if registry.config(connector_name).is_none() {
            let available = registry.connector_names();
            return Ok(ToolResult {
                output: format!(
                    "Connector `{connector_name}` not found. Available connectors: {}",
                    if available.is_empty() {
                        "none configured".to_string()
                    } else {
                        available.join(", ")
                    }
                ),
            });
        }

        // Discover schema.
        let (entities, drift_warnings) = registry.discover(connector_name).await?;

        // Build response — schema metadata only, never sample data.
        let mut response = serde_json::json!({
            "connector": connector_name,
            "entities": entities.iter().map(|e| {
                serde_json::json!({
                    "name": e.name,
                    "primary_key": e.primary_key,
                    "fields": e.fields.iter().map(|f| {
                        serde_json::json!({
                            "name": f.name,
                            "type": f.field_type.to_string(),
                            "required": f.required,
                            "description": f.description,
                        })
                    }).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
        });

        if !drift_warnings.is_empty() {
            response["drift_warnings"] = serde_json::json!(drift_warnings);
        }

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&response)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_connectors::*;
    use std::collections::HashMap;

    fn test_registry() -> Arc<RwLock<ConnectorRegistry>> {
        let mut reg = ConnectorRegistry::new();
        let mut settings = HashMap::new();
        settings.insert(
            "path".to_string(),
            serde_json::Value::String("/nonexistent.csv".to_string()),
        );
        reg.load_configs(vec![ConnectorConfig {
            name: "test_src".to_string(),
            connector_type: ConnectorType::File,
            settings,
            auth: AuthConfig::None,
            privacy_boundary: String::new(),
            rate_limit: RateLimitConfig::default(),
            pagination: PaginationStrategy::None,
            batch_size: 100,
        }]);
        Arc::new(RwLock::new(reg))
    }

    #[tokio::test]
    async fn connector_not_found() {
        let registry = test_registry();
        let tool = ConnectorDiscoverTool::new(registry);
        let ctx = ToolContext::new(String::new());

        let result = tool
            .execute(r#"{"connector": "nonexistent"}"#, &ctx)
            .await
            .expect("execute");

        assert!(result.output.contains("not found"));
        assert!(result.output.contains("test_src"));
    }
}
