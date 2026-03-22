//! Conversation time-range query tool.

use agentzero_core::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Input {
    since: Option<i64>,
    until: Option<i64>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

/// Query conversation memory within a time range.
#[derive(Debug, Default, Clone, Copy)]
pub struct ConversationTimerangeTool;

#[async_trait]
impl Tool for ConversationTimerangeTool {
    fn name(&self) -> &'static str {
        "conversation_timerange"
    }

    fn description(&self) -> &'static str {
        "Query conversation history within a specific time range (unix timestamps)."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "since": { "type": "integer", "description": "Start timestamp (unix seconds)" },
                "until": { "type": "integer", "description": "End timestamp (unix seconds)" },
                "limit": { "type": "integer", "description": "Maximum entries to return (default: 50)" }
            }
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let _parsed: Input = serde_json::from_str(input)
            .map_err(|e| anyhow::anyhow!("conversation_timerange expects JSON: {e}"))?;

        Ok(ToolResult {
            output: "conversation time-range query not yet connected to memory store".to_string(),
        })
    }
}
