//! Conversation time-range query tool.

use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct SchemaInput {
    /// Start timestamp (unix seconds)
    #[serde(default)]
    since: Option<i64>,
    /// End timestamp (unix seconds)
    #[serde(default)]
    until: Option<i64>,
    /// Maximum entries to return (default: 50)
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ExecuteInput {
    since: Option<i64>,
    until: Option<i64>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

/// Query conversation memory within a time range.
#[tool(
    name = "conversation_timerange",
    description = "Query conversation history within a specific time range (unix timestamps)."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct ConversationTimerangeTool;

#[async_trait]
impl Tool for ConversationTimerangeTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(SchemaInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let _parsed: ExecuteInput = serde_json::from_str(input)
            .map_err(|e| anyhow::anyhow!("conversation_timerange expects JSON: {e}"))?;

        Ok(ToolResult {
            output: "conversation time-range query not yet connected to memory store".to_string(),
        })
    }
}
