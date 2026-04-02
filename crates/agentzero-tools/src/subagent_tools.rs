use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SUBAGENTS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentStatus {
    Running,
    Completed,
    Failed,
    Killed,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct SubAgentSession {
    id: String,
    agent: String,
    task: String,
    status: SubAgentStatus,
    created_at: u64,
    result: Option<String>,
}

#[derive(Default)]
pub struct SubAgentRegistry {
    sessions: Vec<SubAgentSession>,
}

// ── subagent_spawn ──

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct SubAgentSpawnInput {
    /// Name of the sub-agent to spawn
    agent: String,
    /// Task description for the sub-agent
    task: String,
    /// Optional context to pass to the sub-agent
    #[serde(default)]
    context: Option<String>,
}

#[tool(
    name = "subagent_spawn",
    description = "Spawn a sub-agent to handle a task asynchronously. Returns a session ID for tracking."
)]
pub struct SubAgentSpawnTool {
    registry: Mutex<SubAgentRegistry>,
}

impl Default for SubAgentSpawnTool {
    fn default() -> Self {
        Self {
            registry: Mutex::new(SubAgentRegistry::default()),
        }
    }
}

impl SubAgentSpawnTool {
    pub fn registry(&self) -> &Mutex<SubAgentRegistry> {
        &self.registry
    }
}

#[async_trait]
impl Tool for SubAgentSpawnTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(SubAgentSpawnInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: SubAgentSpawnInput = serde_json::from_str(input)
            .context("subagent_spawn expects JSON: {\"agent\": \"...\", \"task\": \"...\"}")?;

        if req.agent.trim().is_empty() {
            return Err(anyhow!("agent must not be empty"));
        }
        if req.task.trim().is_empty() {
            return Err(anyhow!("task must not be empty"));
        }

        let mut registry = self.registry.lock().map_err(|_| anyhow!("lock poisoned"))?;

        let active = registry
            .sessions
            .iter()
            .filter(|s| s.status == SubAgentStatus::Running)
            .count();
        if active >= MAX_SUBAGENTS {
            return Err(anyhow!(
                "max concurrent subagents reached ({MAX_SUBAGENTS})"
            ));
        }

        let session_id = format!(
            "sa-{}-{}",
            registry.sessions.len(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let _context = req.context;

        registry.sessions.push(SubAgentSession {
            id: session_id.clone(),
            agent: req.agent.clone(),
            task: req.task.clone(),
            status: SubAgentStatus::Running,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            result: None,
        });

        Ok(ToolResult {
            output: format!(
                "spawned subagent: session_id={session_id} agent={} status=running\nUse subagent_list or subagent_manage to check progress.",
                req.agent
            ),
        })
    }
}

// ── subagent_list ──

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct SubAgentListInput {}

#[tool(
    name = "subagent_list",
    description = "List all running sub-agent sessions and their statuses."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct SubAgentListTool;

#[async_trait]
impl Tool for SubAgentListTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(SubAgentListInput::schema())
    }

    async fn execute(&self, _input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            output: "subagent_list: no shared registry in standalone mode. Use subagent_spawn's registry.".to_string(),
        })
    }
}

// ── subagent_manage ──

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct SubAgentManageInput {
    /// The session ID of the sub-agent to manage
    session_id: String,
    /// The management action to perform
    #[schema(enum_values = ["status", "kill", "result"])]
    action: SubAgentManageAction,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SubAgentManageAction {
    Status,
    Kill,
    Result,
}

#[tool(
    name = "subagent_manage",
    description = "Manage a sub-agent session: cancel, get result, or check status."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct SubAgentManageTool;

#[async_trait]
impl Tool for SubAgentManageTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(SubAgentManageInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: SubAgentManageInput = serde_json::from_str(input).context(
            "subagent_manage expects JSON: {\"session_id\": \"...\", \"action\": \"...\"}",
        )?;

        if req.session_id.trim().is_empty() {
            return Err(anyhow!("session_id must not be empty"));
        }

        Ok(ToolResult {
            output: format!(
                "subagent_manage: action={:?} for session {}. Standalone mode — use shared registry for full functionality.",
                req.action, req.session_id
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subagent_spawn_creates_session() {
        let tool = SubAgentSpawnTool::default();
        let result = tool
            .execute(
                r#"{"agent": "researcher", "task": "find info"}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect("spawn should succeed");
        assert!(result.output.contains("spawned subagent"));
        assert!(result.output.contains("researcher"));
    }

    #[tokio::test]
    async fn subagent_spawn_rejects_empty_agent() {
        let tool = SubAgentSpawnTool::default();
        let err = tool
            .execute(
                r#"{"agent": "", "task": "find info"}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("empty agent should fail");
        assert!(err.to_string().contains("agent must not be empty"));
    }

    #[tokio::test]
    async fn subagent_list_standalone() {
        let tool = SubAgentListTool;
        let result = tool
            .execute("{}", &ToolContext::new(".".to_string()))
            .await
            .expect("list should succeed");
        assert!(result.output.contains("subagent_list"));
    }

    #[tokio::test]
    async fn subagent_manage_standalone() {
        let tool = SubAgentManageTool;
        let result = tool
            .execute(
                r#"{"session_id": "sa-0-123", "action": "status"}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect("manage should succeed");
        assert!(result.output.contains("subagent_manage"));
    }
}
