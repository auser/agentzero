use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct MissionStatusInput {
    /// Optional specific mission ID to query
    #[serde(default)]
    mission_id: Option<String>,
    /// Optional status filter for listing missions
    #[serde(default)]
    #[schema(enum_values = ["pending", "in_progress", "completed", "failed", "stalled"])]
    status_filter: Option<String>,
}

/// Tool for querying the status of autopilot missions.
#[tool(
    name = "mission_status",
    description = "Query the status of autopilot missions. Can query a specific mission by ID or list all missions optionally filtered by status."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct MissionStatusTool;

#[async_trait]
impl Tool for MissionStatusTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(MissionStatusInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: MissionStatusInput = serde_json::from_str(input)
            .map_err(|e| anyhow::anyhow!("mission_status expects JSON: {e}"))?;

        // Without a live Supabase connection, return a placeholder.
        // The actual implementation will query Supabase via the client
        // stored in the tool's state (injected at construction time).
        if let Some(id) = &req.mission_id {
            Ok(ToolResult {
                output: format!("mission query: id={id} (connect Supabase for live data)"),
            })
        } else {
            let filter_msg = req.status_filter.as_deref().unwrap_or("all");
            Ok(ToolResult {
                output: format!(
                    "mission list: filter={filter_msg} (connect Supabase for live data)"
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_valid() {
        let tool = MissionStatusTool;
        assert_eq!(tool.name(), "mission_status");
        let schema = tool.input_schema().expect("has schema");
        assert_eq!(schema["type"], "object");
    }
}
