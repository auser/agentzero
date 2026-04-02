use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct TriggerFireInput {
    /// ID of the trigger to fire
    trigger_id: String,
    /// Optional context data to pass to the trigger action
    #[serde(default)]
    context: Option<serde_json::Value>,
}

/// Tool to manually fire a trigger (for testing or agent-initiated reactions).
#[tool(
    name = "trigger_fire",
    description = "Manually fire an autopilot trigger by ID. Useful for testing trigger behavior or for agents to initiate reactions programmatically."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TriggerFireTool;

#[derive(Debug, Deserialize)]
struct TriggerFireExec {
    trigger_id: String,
    #[serde(default)]
    context: serde_json::Value,
}

#[async_trait]
impl Tool for TriggerFireTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(TriggerFireInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: TriggerFireExec = serde_json::from_str(input)
            .map_err(|e| anyhow::anyhow!("trigger_fire expects JSON: {e}"))?;

        let agent_id = ctx.agent_id.as_deref().unwrap_or("unknown");

        // Publish event to event bus if available
        if let Some(bus) = &ctx.event_bus {
            let event = agentzero_core::Event::new(
                "autopilot.trigger.fired",
                agent_id,
                serde_json::json!({
                    "trigger_id": req.trigger_id,
                    "fired_by": agent_id,
                    "context": req.context,
                })
                .to_string(),
            );
            if let Err(e) = bus.publish(event).await {
                tracing::warn!(error = %e, "failed to publish trigger fire event");
            }
        }

        Ok(ToolResult {
            output: format!("trigger {} fired by {agent_id}", req.trigger_id,),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_valid() {
        let tool = TriggerFireTool;
        assert_eq!(tool.name(), "trigger_fire");
        let schema = tool.input_schema().expect("has schema");
        let required = schema["required"].as_array().expect("required array");
        assert!(required.contains(&serde_json::Value::String("trigger_id".to_string())));
    }
}
