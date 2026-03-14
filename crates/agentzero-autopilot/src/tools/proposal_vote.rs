use agentzero_core::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ProposalVoteInput {
    proposal_id: String,
    vote: String,
    #[serde(default)]
    reason: String,
}

/// Tool that allows agents (or the system) to approve or reject proposals.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProposalVoteTool;

#[async_trait]
impl Tool for ProposalVoteTool {
    fn name(&self) -> &'static str {
        "proposal_vote"
    }

    fn description(&self) -> &'static str {
        "Vote to approve or reject a pending proposal. Approved proposals \
         are automatically converted into executable missions."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "proposal_id": {
                    "type": "string",
                    "description": "ID of the proposal to vote on"
                },
                "vote": {
                    "type": "string",
                    "enum": ["approve", "reject"],
                    "description": "Vote to approve or reject"
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason for the vote"
                }
            },
            "required": ["proposal_id", "vote"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: ProposalVoteInput = serde_json::from_str(input)
            .map_err(|e| anyhow::anyhow!("proposal_vote expects JSON: {e}"))?;

        let status = match req.vote.as_str() {
            "approve" => "approved",
            "reject" => "rejected",
            other => {
                return Ok(ToolResult {
                    output: format!("invalid vote: {other} (must be 'approve' or 'reject')"),
                });
            }
        };

        let agent_id = ctx.agent_id.as_deref().unwrap_or("unknown");

        // Publish event to event bus if available
        if let Some(bus) = &ctx.event_bus {
            let event = agentzero_core::Event::new(
                format!("autopilot.proposal.{status}"),
                agent_id,
                serde_json::json!({
                    "proposal_id": req.proposal_id,
                    "vote": req.vote,
                    "reason": req.reason,
                    "voter": agent_id,
                })
                .to_string(),
            );
            if let Err(e) = bus.publish(event).await {
                tracing::warn!(error = %e, "failed to publish vote event to bus");
            }
        }

        let reason_msg = if req.reason.is_empty() {
            String::new()
        } else {
            format!(", reason: {}", req.reason)
        };

        Ok(ToolResult {
            output: format!("proposal {} {}{reason_msg}", req.proposal_id, status),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_valid() {
        let tool = ProposalVoteTool;
        assert_eq!(tool.name(), "proposal_vote");
        let schema = tool.input_schema().expect("has schema");
        let required = schema["required"].as_array().expect("required array");
        assert!(required.contains(&serde_json::Value::String("proposal_id".to_string())));
        assert!(required.contains(&serde_json::Value::String("vote".to_string())));
    }
}
