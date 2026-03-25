//! LLM-callable tool for managing persistent agents.
//!
//! Placed in `agentzero-infra` (not `agentzero-tools`) to avoid a circular
//! dependency: this module needs `AgentStore` from `agentzero-orchestrator`,
//! and `agentzero-infra` already depends on both `agentzero-tools` and
//! `agentzero-orchestrator`.

use agentzero_core::agent_store::{AgentRecord, AgentStatus, AgentStoreApi, AgentUpdate};
use agentzero_core::{Provider, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct Input {
    action: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    keywords: Option<Vec<String>>,
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    status: Option<String>,
    /// Natural language description for `create_from_description` action.
    #[serde(default)]
    nl_description: Option<String>,
}

pub struct AgentManageTool {
    store: Arc<dyn AgentStoreApi>,
    /// Optional provider for LLM-based agent derivation (`create_from_description`).
    provider: Option<Arc<dyn Provider>>,
}

impl AgentManageTool {
    pub fn new(store: Arc<dyn AgentStoreApi>) -> Self {
        Self {
            store,
            provider: None,
        }
    }

    /// Attach a provider for NL agent creation.
    pub fn with_provider(mut self, provider: Arc<dyn Provider>) -> Self {
        self.provider = Some(provider);
        self
    }
}

#[async_trait]
impl Tool for AgentManageTool {
    fn name(&self) -> &'static str {
        "agent_manage"
    }

    fn description(&self) -> &'static str {
        "Create, list, update, or delete persistent agents. Supports natural language agent \
         creation via create_from_description — describe an agent in plain English and the system \
         derives name, system prompt, keywords, and tools automatically."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "create_from_description", "list", "get", "update", "delete", "set_status"],
                    "description": "The management action to perform"
                },
                "nl_description": {
                    "type": "string",
                    "description": "Natural language description of the agent (for create_from_description). Example: 'an agent that monitors my GitHub PRs and summarizes them daily'"
                },
                "name": {
                    "type": "string",
                    "description": "Agent name (required for create)"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID (required for get/update/delete/set_status)"
                },
                "description": {
                    "type": "string",
                    "description": "What this agent does"
                },
                "model": {
                    "type": "string",
                    "description": "Model to use (e.g. claude-sonnet-4-20250514)"
                },
                "provider": {
                    "type": "string",
                    "description": "Provider (e.g. anthropic, openai, openrouter)"
                },
                "system_prompt": {
                    "type": "string",
                    "description": "System prompt / persona for the agent"
                },
                "keywords": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Keywords for routing messages to this agent"
                },
                "allowed_tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tool names this agent can use (empty = all)"
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "stopped"],
                    "description": "Agent status (for set_status action)"
                }
            },
            "required": ["action"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input =
            serde_json::from_str(input).map_err(|e| anyhow::anyhow!("invalid input: {e}"))?;

        let store = self.store.as_ref();
        let output = match parsed.action.as_str() {
            "create" => action_create(store, &parsed)?,
            "create_from_description" => {
                self.action_create_from_description(store, &parsed).await?
            }
            "list" => action_list(store),
            "get" => action_get(store, &parsed)?,
            "update" => action_update(store, &parsed)?,
            "delete" => action_delete(store, &parsed)?,
            "set_status" => action_set_status(store, &parsed)?,
            other => anyhow::bail!(
                "unknown action '{other}'. Valid: create, create_from_description, list, get, update, delete, set_status"
            ),
        };

        Ok(ToolResult { output })
    }
}

fn action_create(store: &dyn AgentStoreApi, input: &Input) -> anyhow::Result<String> {
    let name = input
        .name
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("'name' is required for create"))?;

    let record = AgentRecord {
        agent_id: String::new(), // auto-generated by store
        name: name.to_string(),
        description: input.description.clone().unwrap_or_default(),
        system_prompt: input.system_prompt.clone(),
        provider: input.provider.clone().unwrap_or_default(),
        model: input.model.clone().unwrap_or_default(),
        keywords: input.keywords.clone().unwrap_or_default(),
        allowed_tools: input.allowed_tools.clone().unwrap_or_default(),
        channels: HashMap::new(),
        created_at: 0,
        updated_at: 0,
        status: AgentStatus::Active,
    };

    let created = store.create(record)?;

    Ok(format!(
        "Agent created successfully.\n\
         - ID: {}\n\
         - Name: {}\n\
         - Provider: {}\n\
         - Model: {}\n\
         - Keywords: {}\n\
         - Status: active",
        created.agent_id,
        created.name,
        if created.provider.is_empty() {
            "(default)"
        } else {
            &created.provider
        },
        if created.model.is_empty() {
            "(default)"
        } else {
            &created.model
        },
        if created.keywords.is_empty() {
            "(none)".to_string()
        } else {
            created.keywords.join(", ")
        },
    ))
}

fn action_list(store: &dyn AgentStoreApi) -> String {
    let agents = store.list();
    if agents.is_empty() {
        return "No persistent agents found.".to_string();
    }

    let mut lines = vec![format!("Found {} agent(s):\n", agents.len())];
    for a in &agents {
        let status = match a.status {
            AgentStatus::Active => "active",
            AgentStatus::Stopped => "stopped",
        };
        lines.push(format!(
            "- {} (id: {}, model: {}, status: {}, keywords: [{}])",
            a.name,
            a.agent_id,
            if a.model.is_empty() {
                "(default)"
            } else {
                &a.model
            },
            status,
            a.keywords.join(", "),
        ));
    }
    lines.join("\n")
}

fn action_get(store: &dyn AgentStoreApi, input: &Input) -> anyhow::Result<String> {
    let agent_id = input
        .agent_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("'agent_id' is required for get"))?;

    let record = store
        .get(agent_id)
        .ok_or_else(|| anyhow::anyhow!("agent '{agent_id}' not found"))?;

    let status = match record.status {
        AgentStatus::Active => "active",
        AgentStatus::Stopped => "stopped",
    };

    Ok(format!(
        "Agent: {}\n\
         - ID: {}\n\
         - Description: {}\n\
         - Provider: {}\n\
         - Model: {}\n\
         - System prompt: {}\n\
         - Keywords: [{}]\n\
         - Allowed tools: [{}]\n\
         - Status: {}\n\
         - Created: {}\n\
         - Updated: {}",
        record.name,
        record.agent_id,
        if record.description.is_empty() {
            "(none)"
        } else {
            &record.description
        },
        if record.provider.is_empty() {
            "(default)"
        } else {
            &record.provider
        },
        if record.model.is_empty() {
            "(default)"
        } else {
            &record.model
        },
        record.system_prompt.as_deref().unwrap_or("(none)"),
        record.keywords.join(", "),
        if record.allowed_tools.is_empty() {
            "all".to_string()
        } else {
            record.allowed_tools.join(", ")
        },
        status,
        record.created_at,
        record.updated_at,
    ))
}

fn action_update(store: &dyn AgentStoreApi, input: &Input) -> anyhow::Result<String> {
    let agent_id = input
        .agent_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("'agent_id' is required for update"))?;

    let update = AgentUpdate {
        name: input.name.clone(),
        description: input.description.clone(),
        system_prompt: input.system_prompt.clone(),
        provider: input.provider.clone(),
        model: input.model.clone(),
        keywords: input.keywords.clone(),
        allowed_tools: input.allowed_tools.clone(),
        channels: None,
    };

    match store.update(agent_id, update)? {
        Some(updated) => Ok(format!(
            "Agent '{}' updated successfully (id: {}).",
            updated.name, updated.agent_id
        )),
        None => anyhow::bail!("agent '{agent_id}' not found"),
    }
}

fn action_delete(store: &dyn AgentStoreApi, input: &Input) -> anyhow::Result<String> {
    let agent_id = input
        .agent_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("'agent_id' is required for delete"))?;

    if store.delete(agent_id)? {
        Ok(format!("Agent '{agent_id}' deleted successfully."))
    } else {
        anyhow::bail!("agent '{agent_id}' not found")
    }
}

fn action_set_status(store: &dyn AgentStoreApi, input: &Input) -> anyhow::Result<String> {
    let agent_id = input
        .agent_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("'agent_id' is required for set_status"))?;

    let status_str = input.status.as_deref().ok_or_else(|| {
        anyhow::anyhow!("'status' is required for set_status (active or stopped)")
    })?;

    let status = match status_str {
        "active" => AgentStatus::Active,
        "stopped" => AgentStatus::Stopped,
        other => anyhow::bail!("invalid status '{other}'. Must be 'active' or 'stopped'"),
    };

    if store.set_status(agent_id, status)? {
        Ok(format!("Agent '{agent_id}' status set to '{status_str}'."))
    } else {
        anyhow::bail!("agent '{agent_id}' not found")
    }
}

// ── NL Agent Creation ────────────────────────────────────────────────────────

const NL_AGENT_PROMPT: &str = r#"You are an agent definition generator. Given a natural language description of a desired agent, output a JSON definition.

Output a JSON object with this exact structure:
{
  "name": "short_snake_case_name",
  "description": "One-line description for routing and display",
  "system_prompt": "Detailed instructions for the agent's behavior and personality",
  "keywords": ["keyword1", "keyword2"],
  "allowed_tools": ["tool1", "tool2"],
  "suggested_schedule": ""
}

Rules:
- name: short snake_case identifier (alphanumeric + underscores only)
- description: one-line summary suitable for keyword routing
- system_prompt: detailed persona/instructions, include expertise areas and behavioral guidelines
- keywords: routing keywords so future messages can auto-route to this agent (3-8 keywords)
- allowed_tools: tools this agent needs (empty array = all tools). Common tools: shell, read_file, write_file, web_search, web_fetch, http_request, git_operations, content_search
- suggested_schedule: cron expression if the description implies periodicity (e.g. "0 9 * * *" for daily at 9am). Empty string if no scheduling implied.
- Output ONLY the JSON object, no markdown fences or explanation"#;

/// Create an agent from a natural language description using the LLM.
///
/// Returns the created `AgentRecord` and an optional suggested cron schedule.
pub async fn create_agent_from_nl(
    store: &dyn AgentStoreApi,
    provider: &dyn Provider,
    description: &str,
) -> anyhow::Result<(AgentRecord, String)> {
    let existing = store.list();
    let existing_summary = if existing.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = existing
            .iter()
            .map(|a| format!("- {} (keywords: [{}])", a.name, a.keywords.join(", ")))
            .collect();
        format!(
            "\n\nExisting agents (avoid creating duplicates):\n{}",
            lines.join("\n")
        )
    };

    let prompt = format!("{NL_AGENT_PROMPT}{existing_summary}\n\nAgent description: {description}");

    let result = provider.complete(&prompt).await?;
    let response = result.output_text.trim();

    let parsed = parse_agent_json(response)?;

    let name = parsed["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("LLM response missing 'name'"))?;
    let agent_desc = parsed["description"].as_str().unwrap_or(description);
    let system_prompt = parsed["system_prompt"].as_str().unwrap_or("");
    let keywords: Vec<String> = parsed["keywords"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let allowed_tools: Vec<String> = parsed["allowed_tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let suggested_schedule = parsed["suggested_schedule"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let record = AgentRecord {
        agent_id: String::new(),
        name: name.to_string(),
        description: agent_desc.to_string(),
        system_prompt: Some(system_prompt.to_string()),
        provider: String::new(),
        model: String::new(),
        keywords,
        allowed_tools,
        channels: HashMap::new(),
        created_at: 0,
        updated_at: 0,
        status: AgentStatus::Active,
    };

    let created = store.create(record)?;
    Ok((created, suggested_schedule))
}

impl AgentManageTool {
    async fn action_create_from_description(
        &self,
        store: &dyn AgentStoreApi,
        input: &Input,
    ) -> anyhow::Result<String> {
        let nl_desc = input
            .nl_description
            .as_deref()
            .or(input.description.as_deref())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "'nl_description' or 'description' is required for create_from_description"
                )
            })?;

        let provider = self.provider.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "create_from_description requires a provider (enable_dynamic_tools in config)"
            )
        })?;

        let (mut created, suggested_schedule) =
            create_agent_from_nl(store, provider.as_ref(), nl_desc).await?;

        if let Some(ref p) = input.provider {
            created.provider = p.clone();
        }
        if let Some(ref m) = input.model {
            created.model = m.clone();
        }

        let mut output = format!(
            "Agent created from description.\n\
             - ID: {}\n\
             - Name: {}\n\
             - Description: {}\n\
             - Keywords: [{}]\n\
             - Allowed tools: [{}]\n\
             - Status: active\n\
             - Persists across sessions: yes",
            created.agent_id,
            created.name,
            created.description,
            created.keywords.join(", "),
            if created.allowed_tools.is_empty() {
                "(all)".to_string()
            } else {
                created.allowed_tools.join(", ")
            },
        );

        if !suggested_schedule.is_empty() {
            output.push_str(&format!(
                "\n- Suggested schedule: {} (use cron_add to activate)",
                suggested_schedule
            ));
        }

        Ok(output)
    }
}

/// Parse JSON from an LLM response (handles markdown fences, leading text).
fn parse_agent_json(response: &str) -> anyhow::Result<serde_json::Value> {
    let trimmed = response.trim();

    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            if let Ok(v) = serde_json::from_str(after[..end].trim()) {
                return Ok(v);
            }
        }
    }

    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        if let Some(end) = after.find("```") {
            if let Ok(v) = serde_json::from_str(after[..end].trim()) {
                return Ok(v);
            }
        }
    }

    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if let Ok(v) = serde_json::from_str(&trimmed[start..=end]) {
                return Ok(v);
            }
        }
    }

    serde_json::from_str(trimmed)
        .map_err(|e| anyhow::anyhow!("failed to parse agent definition from LLM: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;
    use std::sync::RwLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    /// Minimal in-memory `AgentStoreApi` for tests.
    struct MemoryStore {
        agents: RwLock<Vec<AgentRecord>>,
        counter: std::sync::atomic::AtomicU64,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self {
                agents: RwLock::new(Vec::new()),
                counter: std::sync::atomic::AtomicU64::new(0),
            }
        }
    }

    impl AgentStoreApi for MemoryStore {
        fn create(&self, mut record: AgentRecord) -> anyhow::Result<AgentRecord> {
            let seq = self
                .counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_secs();
            if record.agent_id.is_empty() {
                record.agent_id = format!("agent_test_{seq}");
            }
            record.created_at = now;
            record.updated_at = now;
            record.status = AgentStatus::Active;
            let mut agents = self.agents.write().expect("lock");
            agents.push(record.clone());
            Ok(record)
        }

        fn get(&self, agent_id: &str) -> Option<AgentRecord> {
            self.agents
                .read()
                .expect("lock")
                .iter()
                .find(|a| a.agent_id == agent_id)
                .cloned()
        }

        fn list(&self) -> Vec<AgentRecord> {
            self.agents.read().expect("lock").clone()
        }

        fn update(
            &self,
            agent_id: &str,
            update: AgentUpdate,
        ) -> anyhow::Result<Option<AgentRecord>> {
            let mut agents = self.agents.write().expect("lock");
            let Some(record) = agents.iter_mut().find(|a| a.agent_id == agent_id) else {
                return Ok(None);
            };
            if let Some(name) = update.name {
                record.name = name;
            }
            if let Some(desc) = update.description {
                record.description = desc;
            }
            if let Some(sp) = update.system_prompt {
                record.system_prompt = Some(sp);
            }
            if let Some(p) = update.provider {
                record.provider = p;
            }
            if let Some(m) = update.model {
                record.model = m;
            }
            if let Some(kw) = update.keywords {
                record.keywords = kw;
            }
            if let Some(at) = update.allowed_tools {
                record.allowed_tools = at;
            }
            Ok(Some(record.clone()))
        }

        fn delete(&self, agent_id: &str) -> anyhow::Result<bool> {
            let mut agents = self.agents.write().expect("lock");
            let before = agents.len();
            agents.retain(|a| a.agent_id != agent_id);
            Ok(agents.len() < before)
        }

        fn set_status(&self, agent_id: &str, status: AgentStatus) -> anyhow::Result<bool> {
            let mut agents = self.agents.write().expect("lock");
            let Some(record) = agents.iter_mut().find(|a| a.agent_id == agent_id) else {
                return Ok(false);
            };
            record.status = status;
            Ok(true)
        }

        fn count(&self) -> usize {
            self.agents.read().expect("lock").len()
        }
    }

    fn test_store() -> Arc<dyn AgentStoreApi> {
        Arc::new(MemoryStore::new())
    }

    #[tokio::test]
    async fn create_and_list() {
        let store = test_store();
        let tool = AgentManageTool::new(store.clone());
        let ctx = test_ctx();

        let result = tool
            .execute(
                r#"{"action":"create","name":"Aria","description":"Travel planner","model":"claude-sonnet-4-20250514","provider":"anthropic","keywords":["travel","booking"]}"#,
                &ctx,
            )
            .await
            .expect("create should succeed");
        assert!(result.output.contains("Agent created successfully"));
        assert!(result.output.contains("Aria"));

        let result = tool
            .execute(r#"{"action":"list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("1 agent(s)"));
        assert!(result.output.contains("Aria"));
        assert!(result.output.contains("travel, booking"));
    }

    #[tokio::test]
    async fn get_agent() {
        let store = test_store();
        let tool = AgentManageTool::new(store.clone());
        let ctx = test_ctx();

        tool.execute(
            r#"{"action":"create","name":"Bot","model":"gpt-4o","provider":"openai"}"#,
            &ctx,
        )
        .await
        .expect("create");

        let agents = store.list();
        let id = &agents[0].agent_id;

        let result = tool
            .execute(&format!(r#"{{"action":"get","agent_id":"{id}"}}"#), &ctx)
            .await
            .expect("get should succeed");
        assert!(result.output.contains("Bot"));
        assert!(result.output.contains("gpt-4o"));
    }

    #[tokio::test]
    async fn update_agent() {
        let store = test_store();
        let tool = AgentManageTool::new(store.clone());
        let ctx = test_ctx();

        tool.execute(
            r#"{"action":"create","name":"Old","model":"old-model"}"#,
            &ctx,
        )
        .await
        .expect("create");

        let id = store.list()[0].agent_id.clone();

        let result = tool
            .execute(
                &format!(
                    r#"{{"action":"update","agent_id":"{id}","name":"New","model":"new-model"}}"#
                ),
                &ctx,
            )
            .await
            .expect("update should succeed");
        assert!(result.output.contains("updated successfully"));

        let updated = store.get(&id).expect("should exist");
        assert_eq!(updated.name, "New");
        assert_eq!(updated.model, "new-model");
    }

    #[tokio::test]
    async fn delete_agent() {
        let store = test_store();
        let tool = AgentManageTool::new(store.clone());
        let ctx = test_ctx();

        tool.execute(r#"{"action":"create","name":"Temp"}"#, &ctx)
            .await
            .expect("create");

        let id = store.list()[0].agent_id.clone();
        assert_eq!(store.count(), 1);

        let result = tool
            .execute(&format!(r#"{{"action":"delete","agent_id":"{id}"}}"#), &ctx)
            .await
            .expect("delete should succeed");
        assert!(result.output.contains("deleted successfully"));
        assert_eq!(store.count(), 0);
    }

    #[tokio::test]
    async fn set_status() {
        let store = test_store();
        let tool = AgentManageTool::new(store.clone());
        let ctx = test_ctx();

        tool.execute(r#"{"action":"create","name":"Runner"}"#, &ctx)
            .await
            .expect("create");

        let id = store.list()[0].agent_id.clone();

        let result = tool
            .execute(
                &format!(r#"{{"action":"set_status","agent_id":"{id}","status":"stopped"}}"#),
                &ctx,
            )
            .await
            .expect("set_status should succeed");
        assert!(result.output.contains("stopped"));

        let record = store.get(&id).expect("should exist");
        assert_eq!(record.status, AgentStatus::Stopped);
    }

    #[tokio::test]
    async fn create_without_name_fails() {
        let store = test_store();
        let tool = AgentManageTool::new(store);
        let ctx = test_ctx();

        let result = tool.execute(r#"{"action":"create"}"#, &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_empty() {
        let store = test_store();
        let tool = AgentManageTool::new(store);
        let ctx = test_ctx();

        let result = tool
            .execute(r#"{"action":"list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("No persistent agents found"));
    }

    // ── NL Agent Creation tests ─────────────────────────────────────

    struct MockNlProvider {
        response: String,
    }

    #[async_trait]
    impl Provider for MockNlProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<agentzero_core::ChatResult> {
            Ok(agentzero_core::ChatResult {
                output_text: self.response.clone(),
                tool_calls: vec![],
                stop_reason: None,
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    #[tokio::test]
    async fn create_from_description_derives_agent_fields() {
        let store = test_store();
        let provider = Arc::new(MockNlProvider {
            response: r#"{
                "name": "pr_reviewer",
                "description": "Reviews GitHub PRs and provides feedback",
                "system_prompt": "You are an expert code reviewer. Review PRs thoroughly.",
                "keywords": ["pr", "review", "github", "code review"],
                "allowed_tools": ["shell", "read_file", "web_fetch", "git_operations"],
                "suggested_schedule": "0 9 * * *"
            }"#
            .to_string(),
        });

        let tool = AgentManageTool::new(store.clone()).with_provider(provider);
        let ctx = test_ctx();

        let result = tool
            .execute(
                r#"{"action":"create_from_description","nl_description":"an agent that reviews my GitHub PRs daily"}"#,
                &ctx,
            )
            .await
            .expect("create_from_description should succeed");

        assert!(result.output.contains("pr_reviewer"));
        assert!(result.output.contains("Persists across sessions: yes"));
        assert!(result.output.contains("Suggested schedule"));

        // Verify the agent was persisted.
        let agents = store.list();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "pr_reviewer");
        assert_eq!(
            agents[0].keywords,
            vec!["pr", "review", "github", "code review"]
        );
        assert_eq!(
            agents[0].allowed_tools,
            vec!["shell", "read_file", "web_fetch", "git_operations"]
        );
        assert!(agents[0]
            .system_prompt
            .as_deref()
            .unwrap_or("")
            .contains("expert code reviewer"));
    }

    #[tokio::test]
    async fn create_from_description_without_provider_fails() {
        let store = test_store();
        // No provider attached — should fail gracefully.
        let tool = AgentManageTool::new(store);
        let ctx = test_ctx();

        let err = tool
            .execute(
                r#"{"action":"create_from_description","nl_description":"test agent"}"#,
                &ctx,
            )
            .await;
        assert!(err.is_err(), "should fail without provider");
    }

    #[tokio::test]
    async fn create_from_description_includes_existing_agents_in_prompt() {
        use std::sync::Mutex;

        struct CapturingProvider {
            captured: Mutex<String>,
        }
        #[async_trait]
        impl Provider for CapturingProvider {
            async fn complete(&self, prompt: &str) -> anyhow::Result<agentzero_core::ChatResult> {
                *self.captured.lock().expect("lock") = prompt.to_string();
                Ok(agentzero_core::ChatResult {
                    output_text: r#"{"name":"new_agent","description":"test","system_prompt":"test","keywords":[],"allowed_tools":[],"suggested_schedule":""}"#.to_string(),
                    tool_calls: vec![],
                    stop_reason: None,
                    input_tokens: 0,
                    output_tokens: 0,
                })
            }
        }

        let store = test_store();
        let ctx = test_ctx();

        // Pre-create an agent so it shows in the prompt.
        let tool_pre = AgentManageTool::new(store.clone());
        tool_pre
            .execute(
                r#"{"action":"create","name":"existing_bot","keywords":["existing"]}"#,
                &ctx,
            )
            .await
            .expect("pre-create");

        let provider = Arc::new(CapturingProvider {
            captured: Mutex::new(String::new()),
        });
        let provider_dyn: Arc<dyn Provider> = Arc::clone(&provider) as Arc<dyn Provider>;
        let tool = AgentManageTool::new(store).with_provider(provider_dyn);

        tool.execute(
            r#"{"action":"create_from_description","nl_description":"a new agent"}"#,
            &ctx,
        )
        .await
        .expect("create_from_description");

        let prompt = provider.captured.lock().expect("lock").clone();
        assert!(
            prompt.contains("existing_bot"),
            "prompt should include existing agents for dedup"
        );
    }
}
