use agentzero_core::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct Input {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

/// Search tool for Discord message history.
#[derive(Debug, Default, Clone, Copy)]
pub struct DiscordSearchTool;

#[async_trait]
impl Tool for DiscordSearchTool {
    fn name(&self) -> &'static str {
        "discord_search"
    }

    fn description(&self) -> &'static str {
        "Search Discord message history for keywords. Returns matching messages with sender names and timestamps."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query keywords" },
                "limit": { "type": "integer", "description": "Maximum results (default: 20)", "default": 20 }
            },
            "required": ["query"]
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input = serde_json::from_str(input).map_err(|e| {
            anyhow::anyhow!("discord_search expects JSON: {{\"query\": \"...\"}}: {e}")
        })?;

        if parsed.query.trim().is_empty() {
            return Err(anyhow::anyhow!("query must not be empty"));
        }

        // TODO: Query SQLite discord_messages table
        // For now, return placeholder indicating the infrastructure is in place
        Ok(ToolResult {
            output: format!(
                "Discord search for '{}' (limit: {}) — history database not yet connected",
                parsed.query, parsed.limit
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_search_has_schema() {
        let tool = DiscordSearchTool;
        let schema = tool.input_schema();
        assert!(
            schema.is_some(),
            "discord_search should have an input schema"
        );
        let schema = schema.expect("schema should be present");
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["query"].is_object());
        assert!(schema["properties"]["limit"].is_object());
        let required = schema["required"]
            .as_array()
            .expect("required should be an array");
        assert!(required.contains(&json!("query")));
    }

    #[tokio::test]
    async fn discord_search_empty_query_error() {
        let tool = DiscordSearchTool;
        let ctx = ToolContext::new(".".to_string());
        let result = tool.execute(r#"{"query": "  "}"#, &ctx).await;
        assert!(result.is_err(), "empty query should produce an error");
        let err_msg = result.expect_err("should be error").to_string();
        assert!(
            err_msg.contains("query must not be empty"),
            "error should mention empty query: {err_msg}"
        );
    }

    #[tokio::test]
    async fn discord_search_invalid_json_error() {
        let tool = DiscordSearchTool;
        let ctx = ToolContext::new(".".to_string());
        let result = tool.execute("not json", &ctx).await;
        assert!(result.is_err(), "invalid JSON should produce an error");
    }

    #[tokio::test]
    async fn discord_search_valid_query() {
        let tool = DiscordSearchTool;
        let ctx = ToolContext::new(".".to_string());
        let result = tool
            .execute(r#"{"query": "hello world", "limit": 5}"#, &ctx)
            .await;
        assert!(result.is_ok(), "valid query should succeed");
        let output = result.expect("should succeed").output;
        assert!(output.contains("hello world"));
        assert!(output.contains("limit: 5"));
    }

    #[tokio::test]
    async fn discord_search_default_limit() {
        let tool = DiscordSearchTool;
        let ctx = ToolContext::new(".".to_string());
        let result = tool.execute(r#"{"query": "test"}"#, &ctx).await;
        assert!(result.is_ok());
        let output = result.expect("should succeed").output;
        assert!(output.contains("limit: 20"), "default limit should be 20");
    }
}
