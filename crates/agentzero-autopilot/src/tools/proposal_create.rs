use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct ProposalCreateSchema {
    /// Short title for the proposal
    title: String,
    /// Detailed description of what should be done
    description: String,
    /// Type of proposal (default: task_request)
    #[serde(default)]
    #[schema(enum_values = ["content_idea", "task_request", "resource_request", "system_change"])]
    proposal_type: Option<String>,
    /// Priority level (default: medium)
    #[serde(default)]
    #[schema(enum_values = ["low", "medium", "high", "critical"])]
    priority: Option<String>,
    /// Estimated cost in microdollars (1 cent = 10000 microdollars)
    #[serde(default)]
    estimated_cost_microdollars: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ProposalCreateInput {
    title: String,
    description: String,
    #[serde(default = "default_proposal_type")]
    proposal_type: String,
    #[serde(default = "default_priority")]
    priority: String,
    #[serde(default)]
    estimated_cost_microdollars: u64,
}

fn default_proposal_type() -> String {
    "task_request".to_string()
}

fn default_priority() -> String {
    "medium".to_string()
}

/// Tool that allows agents to create autopilot proposals.
#[tool(
    name = "proposal_create",
    description = "Create a new proposal for work to be done. The proposal will be evaluated by the cap gate system and, if approved, converted into an executable mission. Use this to suggest new tasks, content ideas, or system changes."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct ProposalCreateTool;

#[async_trait]
impl Tool for ProposalCreateTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(ProposalCreateSchema::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: ProposalCreateInput = serde_json::from_str(input)
            .map_err(|e| anyhow::anyhow!("proposal_create expects JSON: {e}"))?;

        let proposal_type = match req.proposal_type.as_str() {
            "content_idea" => crate::types::ProposalType::ContentIdea,
            "task_request" => crate::types::ProposalType::TaskRequest,
            "resource_request" => crate::types::ProposalType::ResourceRequest,
            "system_change" => crate::types::ProposalType::SystemChange,
            other => {
                return Ok(ToolResult {
                    output: format!("invalid proposal_type: {other}"),
                });
            }
        };

        let priority = match req.priority.as_str() {
            "low" => crate::types::Priority::Low,
            "medium" => crate::types::Priority::Medium,
            "high" => crate::types::Priority::High,
            "critical" => crate::types::Priority::Critical,
            other => {
                return Ok(ToolResult {
                    output: format!("invalid priority: {other}"),
                });
            }
        };

        let agent_id = ctx.agent_id.as_deref().unwrap_or("unknown");

        let proposal = crate::types::Proposal::new(
            agent_id,
            &req.title,
            &req.description,
            proposal_type,
            priority,
            req.estimated_cost_microdollars,
        );

        // Publish event to event bus if available
        if let Some(bus) = &ctx.event_bus {
            let event = agentzero_core::Event::new(
                "autopilot.proposal.created",
                agent_id,
                serde_json::to_string(&proposal).unwrap_or_default(),
            );
            if let Err(e) = bus.publish(event).await {
                tracing::warn!(error = %e, "failed to publish proposal event to bus");
            }
        }

        Ok(ToolResult {
            output: format!(
                "proposal created: id={}, title={}, type={}, priority={}, \
                 estimated_cost={} microdollars, status=pending",
                proposal.id,
                proposal.title,
                proposal.proposal_type,
                proposal.priority,
                proposal.estimated_cost_microdollars,
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_valid() {
        let tool = ProposalCreateTool;
        assert_eq!(tool.name(), "proposal_create");
        let schema = tool.input_schema().expect("has schema");
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().expect("required array");
        assert!(required.contains(&serde_json::Value::String("title".to_string())));
    }
}
