//! Conversation time-range query tool.

use agentzero_core::ToolResult;
use agentzero_macros::tool_fn;

/// Query conversation history within a specific time range (unix timestamps).
#[tool_fn(name = "conversation_timerange")]
async fn conversation_timerange(
    /// Start timestamp (unix seconds)
    #[serde(default)]
    since: Option<i64>,
    /// End timestamp (unix seconds)
    #[serde(default)]
    until: Option<i64>,
    /// Maximum entries to return (default: 50)
    #[serde(default)]
    limit: Option<i64>,
    #[ctx] _ctx: &agentzero_core::ToolContext,
) -> anyhow::Result<ToolResult> {
    Ok(ToolResult {
        output: "conversation time-range query not yet connected to memory store".to_string(),
    })
}
