use crate::autonomy::AutonomyPolicy;
use agentzero_core::delegation::{
    filter_tools, validate_delegation, DelegateConfig, DelegateRequest,
};
use agentzero_core::{
    Agent, AgentConfig, ChatResult, MemoryEntry, MemoryStore, Provider, Tool, ToolContext,
    ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Default maximum concurrent delegations.
const DEFAULT_MAX_CONCURRENT: usize = 4;

/// Function that builds a tool set for sub-agents. The delegate tool calls this
/// when running in agentic mode, then filters the result based on each agent's
/// `allowed_tools` configuration.
pub type ToolBuilder = Arc<dyn Fn() -> anyhow::Result<Vec<Box<dyn Tool>>> + Send + Sync>;

/// Output scanner function. Returns `Ok(safe_text)` (possibly redacted) or
/// `Err(reason)` if the output should be blocked entirely.
/// Wired to `LeakGuardPolicy::process()` at the call site.
pub type OutputScanner = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

#[derive(Debug, Deserialize)]
struct Input {
    agent: String,
    prompt: String,
}

pub struct DelegateTool {
    agents: HashMap<String, DelegateConfig>,
    current_depth: usize,
    tool_builder: ToolBuilder,
    /// Parent's autonomy policy — intersected with child before execution.
    parent_policy: Option<AutonomyPolicy>,
    /// Optional output scanner (leak guard). When set, sub-agent output is
    /// scanned for credentials before being returned to the parent.
    output_scanner: Option<OutputScanner>,
    /// Semaphore limiting concurrent delegations.
    semaphore: Arc<Semaphore>,
}

impl DelegateTool {
    pub fn new(
        agents: HashMap<String, DelegateConfig>,
        current_depth: usize,
        tool_builder: ToolBuilder,
    ) -> Self {
        Self {
            agents,
            current_depth,
            tool_builder,
            parent_policy: None,
            output_scanner: None,
            semaphore: Arc::new(Semaphore::new(DEFAULT_MAX_CONCURRENT)),
        }
    }

    /// Set the parent's autonomy policy. The child's effective policy will be
    /// the intersection (most restrictive on every dimension).
    pub fn with_parent_policy(mut self, policy: AutonomyPolicy) -> Self {
        self.parent_policy = Some(policy);
        self
    }

    /// Set an output scanner (typically `LeakGuardPolicy::process`) for
    /// scanning sub-agent output for credential leaks.
    pub fn with_output_scanner(mut self, scanner: OutputScanner) -> Self {
        self.output_scanner = Some(scanner);
        self
    }

    /// Set the maximum number of concurrent delegations.
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.semaphore = Arc::new(Semaphore::new(max));
        self
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &'static str {
        "delegate"
    }

    fn description(&self) -> &'static str {
        "Delegate a subtask to a named sub-agent with its own provider, model, and tool set."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "agent": { "type": "string", "description": "Name of the sub-agent to delegate to" },
                "prompt": { "type": "string", "description": "The prompt/task to send to the sub-agent" }
            },
            "required": ["agent", "prompt"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // Check cancellation before starting.
        if ctx.is_cancelled() {
            return Ok(ToolResult {
                output: "[Delegation cancelled]".to_string(),
            });
        }

        let parsed: Input =
            serde_json::from_str(input).map_err(|e| anyhow::anyhow!("invalid input: {e}"))?;

        let config = self
            .agents
            .get(&parsed.agent)
            .ok_or_else(|| anyhow::anyhow!("unknown agent: {}", parsed.agent))?;

        let request = DelegateRequest {
            agent_name: parsed.agent.clone(),
            prompt: parsed.prompt.clone(),
            current_depth: self.current_depth,
        };
        validate_delegation(&request, config)?;

        // Acquire concurrency permit (blocks if at limit).
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("delegation semaphore closed"))?;

        let api_key = resolve_delegate_api_key(config);

        let provider = agentzero_providers::build_provider(
            &config.provider_kind,
            config.provider.clone(),
            api_key,
            config.model.clone(),
        );

        // Build child context with inherited cancellation token and fresh budget counters.
        let mut child_ctx = ctx.clone();
        child_ctx.depth = ctx.depth.saturating_add(1);

        // Fresh accumulators for the child — usage is aggregated back after completion.
        child_ctx.tokens_used = Arc::new(std::sync::atomic::AtomicU64::new(0));
        child_ctx.cost_microdollars = Arc::new(std::sync::atomic::AtomicU64::new(0));

        // Set child budget limits from delegate config (0 = inherit parent's remaining budget).
        if config.max_tokens > 0 {
            child_ctx.max_tokens = config.max_tokens;
        } else if ctx.max_tokens > 0 {
            // Inherit remaining budget from parent.
            let used = ctx.current_tokens();
            child_ctx.max_tokens = ctx.max_tokens.saturating_sub(used);
        }
        if config.max_cost_microdollars > 0 {
            child_ctx.max_cost_microdollars = config.max_cost_microdollars;
        } else if ctx.max_cost_microdollars > 0 {
            let used = ctx.current_cost();
            child_ctx.max_cost_microdollars = ctx.max_cost_microdollars.saturating_sub(used);
        }

        // Assign a unique conversation_id to the child so its transcript is
        // discoverable from the parent's event log / trace.
        let child_conversation_id = format!(
            "delegate-{}-{}-{}",
            parsed.agent,
            ctx.depth.saturating_add(1),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        child_ctx.conversation_id = Some(child_conversation_id.clone());

        let output = if config.agentic {
            run_agentic(
                provider,
                config,
                &parsed.prompt,
                &child_ctx,
                &self.tool_builder,
                self.parent_policy.as_ref(),
            )
            .await?
        } else {
            let effective_prompt = match &config.system_prompt {
                Some(sp) => format!("<system>{sp}</system>\n{}", parsed.prompt),
                None => parsed.prompt.clone(),
            };
            run_single_shot(provider.as_ref(), &effective_prompt).await?
        };

        tracing::info!(
            agent = %parsed.agent,
            child_conversation_id = %child_conversation_id,
            parent_conversation_id = ctx.conversation_id.as_deref().unwrap_or(""),
            depth = child_ctx.depth,
            "delegation completed"
        );

        // Aggregate child usage back into parent counters.
        let child_tokens = child_ctx.current_tokens();
        let child_cost = child_ctx.current_cost();
        if child_tokens > 0 {
            ctx.add_tokens(child_tokens);
        }
        if child_cost > 0 {
            ctx.add_cost(child_cost);
        }

        // Leak guard: scan sub-agent output for credentials before returning.
        let safe_output = if let Some(ref scanner) = self.output_scanner {
            scanner(&output).map_err(|blocked| {
                tracing::warn!(
                    agent = %parsed.agent,
                    "delegation output blocked by leak guard: {blocked}"
                );
                anyhow::anyhow!(
                    "delegation output blocked: credential leak detected in sub-agent response"
                )
            })?
        } else {
            output
        };

        Ok(ToolResult {
            output: safe_output,
        })
    }
}

/// Resolve an API key for a delegate agent. Checks (in order):
/// 1. Explicit `api_key` in the delegate config
/// 2. Provider-specific environment variable
/// 3. Generic `OPENAI_API_KEY` fallback
fn resolve_delegate_api_key(config: &DelegateConfig) -> String {
    if let Some(ref key) = config.api_key {
        if !key.is_empty() {
            return key.clone();
        }
    }

    let provider_env_keys: &[&str] = match config.provider_kind.as_str() {
        "anthropic" => &["ANTHROPIC_API_KEY"],
        "openrouter" => &["OPENROUTER_API_KEY"],
        "openai" => &["OPENAI_API_KEY"],
        "google" | "gemini" => &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
        "groq" => &["GROQ_API_KEY"],
        "together" | "together-ai" => &["TOGETHER_API_KEY"],
        "deepseek" => &["DEEPSEEK_API_KEY"],
        "mistral" => &["MISTRAL_API_KEY"],
        "xai" | "grok" => &["XAI_API_KEY"],
        _ => &[],
    };

    for key in provider_env_keys {
        if let Ok(val) = std::env::var(key) {
            if !val.is_empty() {
                return val;
            }
        }
    }

    std::env::var("OPENAI_API_KEY").unwrap_or_default()
}

async fn run_single_shot(provider: &dyn Provider, prompt: &str) -> anyhow::Result<String> {
    let result: ChatResult = provider.complete(prompt).await?;
    Ok(result.output_text)
}

async fn run_agentic(
    provider: Box<dyn Provider>,
    config: &DelegateConfig,
    prompt: &str,
    ctx: &ToolContext,
    tool_builder: &ToolBuilder,
    parent_policy: Option<&AutonomyPolicy>,
) -> anyhow::Result<String> {
    let agent_config = AgentConfig {
        max_tool_iterations: config.max_iterations,
        system_prompt: config.system_prompt.clone(),
        ..Default::default()
    };
    let memory = EphemeralMemory::default();

    // Build tools for the sub-agent. The builder creates the full set; we
    // filter to only those in the agent's allowed_tools (filter_tools also
    // excludes "delegate" to prevent infinite chains).
    let all_tools = tool_builder().unwrap_or_else(|_| vec![]);
    let all_tool_names: Vec<String> = all_tools.iter().map(|t| t.name().to_string()).collect();
    let allowed_names = filter_tools(&all_tool_names, &config.allowed_tools);

    // If parent has an autonomy policy, intersect with a permissive child
    // default so the child can never exceed the parent's restrictions.
    // Tools blocked by the intersected policy are removed from the tool set.
    let effective_tools: Vec<Box<dyn Tool>> = if let Some(parent) = parent_policy {
        let child_policy = AutonomyPolicy::default();
        let intersected = parent.intersect(&child_policy);
        all_tools
            .into_iter()
            .filter(|t| {
                let name = t.name().to_string();
                if !allowed_names.contains(&name) {
                    return false;
                }
                // Check if the intersected policy blocks this tool.
                !matches!(
                    intersected.check_tool(&name),
                    crate::autonomy::ApprovalDecision::Blocked { .. }
                )
            })
            .collect()
    } else {
        all_tools
            .into_iter()
            .filter(|t| allowed_names.contains(&t.name().to_string()))
            .collect()
    };

    let agent = Agent::new(agent_config, provider, Box::new(memory), effective_tools);

    let response = agent
        .respond(
            agentzero_core::UserMessage {
                text: prompt.to_string(),
            },
            ctx,
        )
        .await?;
    Ok(response.text)
}

#[derive(Default)]
struct EphemeralMemory {
    entries: std::sync::Mutex<Vec<MemoryEntry>>,
}

#[async_trait]
impl MemoryStore for EphemeralMemory {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        self.entries
            .lock()
            .expect("ephemeral memory lock poisoned")
            .push(entry);
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let entries = self.entries.lock().expect("ephemeral memory lock poisoned");
        Ok(entries.iter().rev().take(limit).cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autonomy::{ApprovalDecision, AutonomyLevel};
    use std::collections::HashSet;

    fn noop_builder() -> ToolBuilder {
        Arc::new(|| Ok(vec![]))
    }

    fn test_agents() -> HashMap<String, DelegateConfig> {
        let mut map = HashMap::new();
        map.insert(
            "researcher".to_string(),
            DelegateConfig {
                name: "researcher".into(),
                provider_kind: "openai".into(),
                provider: "https://api.example.invalid/v1".into(),
                model: "gpt-4o-mini".into(),
                max_depth: 3,
                agentic: false,
                max_iterations: 10,
                ..Default::default()
            },
        );
        map.insert(
            "coder".to_string(),
            DelegateConfig {
                name: "coder".into(),
                provider_kind: "openai".into(),
                provider: "https://api.example.invalid/v1".into(),
                model: "gpt-4o".into(),
                max_depth: 2,
                agentic: true,
                max_iterations: 5,
                ..Default::default()
            },
        );
        map
    }

    fn test_ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    #[tokio::test]
    async fn delegate_unknown_agent_returns_error() {
        let tool = DelegateTool::new(test_agents(), 0, noop_builder());
        let result = tool
            .execute(r#"{"agent":"nonexistent","prompt":"hello"}"#, &test_ctx())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown agent"));
    }

    #[tokio::test]
    async fn delegate_depth_exceeded_returns_error() {
        let tool = DelegateTool::new(test_agents(), 3, noop_builder());
        let result = tool
            .execute(r#"{"agent":"researcher","prompt":"hello"}"#, &test_ctx())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("depth limit"));
    }

    #[tokio::test]
    async fn delegate_invalid_input_returns_error() {
        let tool = DelegateTool::new(test_agents(), 0, noop_builder());
        let result = tool.execute(r#"not json"#, &test_ctx()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid input"));
    }

    #[tokio::test]
    async fn delegate_rejects_agent_with_delegate_in_allowlist() {
        let mut agents = HashMap::new();
        let mut allowed = HashSet::new();
        allowed.insert("delegate".to_string());
        agents.insert(
            "bad".to_string(),
            DelegateConfig {
                name: "bad".into(),
                provider_kind: "openai".into(),
                provider: "https://api.example.invalid/v1".into(),
                model: "gpt-4o".into(),
                max_depth: 3,
                agentic: true,
                allowed_tools: allowed,
                ..Default::default()
            },
        );
        let tool = DelegateTool::new(agents, 0, noop_builder());
        let result = tool
            .execute(r#"{"agent":"bad","prompt":"hello"}"#, &test_ctx())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("delegate"));
    }

    #[test]
    fn resolve_api_key_prefers_explicit_config() {
        let config = DelegateConfig {
            api_key: Some("explicit-key".into()),
            provider_kind: "openai".into(),
            ..Default::default()
        };
        assert_eq!(resolve_delegate_api_key(&config), "explicit-key");
    }

    #[test]
    fn resolve_api_key_uses_provider_specific_env_var() {
        let config = DelegateConfig {
            provider_kind: "anthropic".into(),
            ..Default::default()
        };
        temp_env::with_vars(
            [
                ("ANTHROPIC_API_KEY", Some("ant-key")),
                ("OPENAI_API_KEY", Some("oai-key")),
            ],
            || {
                assert_eq!(resolve_delegate_api_key(&config), "ant-key");
            },
        );
    }

    #[test]
    fn resolve_api_key_falls_back_to_openai_env() {
        let config = DelegateConfig {
            provider_kind: "custom".into(),
            ..Default::default()
        };
        temp_env::with_vars(
            [
                ("OPENAI_API_KEY", Some("oai-fallback")),
                ("ANTHROPIC_API_KEY", None),
            ],
            || {
                assert_eq!(resolve_delegate_api_key(&config), "oai-fallback");
            },
        );
    }

    #[test]
    fn system_prompt_uses_xml_tags_for_single_shot() {
        let config = DelegateConfig {
            system_prompt: Some("You are a research assistant.".into()),
            ..Default::default()
        };
        let user_prompt = "Find docs about X";
        let effective = match &config.system_prompt {
            Some(sp) => format!("<system>{sp}</system>\n{user_prompt}"),
            None => user_prompt.to_string(),
        };
        assert!(effective.starts_with("<system>You are a research assistant.</system>"));
        assert!(effective.ends_with("Find docs about X"));
    }

    #[test]
    fn no_system_prompt_passes_user_prompt_unchanged() {
        let config = DelegateConfig::default();
        let user_prompt = "Find docs about X";
        let effective = match &config.system_prompt {
            Some(sp) => format!("<system>{sp}</system>\n{user_prompt}"),
            None => user_prompt.to_string(),
        };
        assert_eq!(effective, "Find docs about X");
    }

    #[test]
    fn agentic_delegate_passes_system_prompt_via_config() {
        let config = DelegateConfig {
            system_prompt: Some("Be concise.".into()),
            max_iterations: 5,
            ..Default::default()
        };
        let agent_config = AgentConfig {
            max_tool_iterations: config.max_iterations,
            system_prompt: config.system_prompt.clone(),
            ..Default::default()
        };
        assert_eq!(agent_config.system_prompt.as_deref(), Some("Be concise."));
        assert_eq!(agent_config.max_tool_iterations, 5);
    }

    #[test]
    fn tool_builder_filters_by_allowed_tools() {
        use agentzero_core::{ToolContext, ToolResult};

        struct FakeTool(&'static str);
        #[async_trait]
        impl Tool for FakeTool {
            fn name(&self) -> &'static str {
                self.0
            }
            async fn execute(
                &self,
                _input: &str,
                _ctx: &ToolContext,
            ) -> anyhow::Result<ToolResult> {
                Ok(ToolResult {
                    output: "ok".into(),
                })
            }
        }

        let builder: ToolBuilder = Arc::new(|| {
            Ok(vec![
                Box::new(FakeTool("read_file")) as Box<dyn Tool>,
                Box::new(FakeTool("shell")),
                Box::new(FakeTool("delegate")),
                Box::new(FakeTool("web_search")),
            ])
        });

        let all_tools = builder().unwrap();
        let all_names: Vec<String> = all_tools.iter().map(|t| t.name().to_string()).collect();
        let mut allowed = HashSet::new();
        allowed.insert("read_file".to_string());
        let filtered = filter_tools(&all_names, &allowed);
        assert_eq!(filtered, vec!["read_file".to_string()]);

        let filtered_all = filter_tools(&all_names, &HashSet::new());
        assert!(filtered_all.contains(&"read_file".to_string()));
        assert!(filtered_all.contains(&"shell".to_string()));
        assert!(filtered_all.contains(&"web_search".to_string()));
        assert!(!filtered_all.contains(&"delegate".to_string()));
    }

    // ─── Security mitigation tests ──────────────────────────────────────

    #[tokio::test]
    async fn delegate_cancelled_returns_early() {
        let tool = DelegateTool::new(test_agents(), 0, noop_builder());
        let ctx = test_ctx();
        ctx.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let result = tool
            .execute(r#"{"agent":"researcher","prompt":"hello"}"#, &ctx)
            .await
            .unwrap();
        assert_eq!(result.output, "[Delegation cancelled]");
    }

    #[test]
    fn output_scanner_redacts_credentials() {
        let scanner: OutputScanner = Arc::new(|text| {
            if text.contains("sk-") {
                Ok(text.replace("sk-abc123", "[REDACTED]"))
            } else {
                Ok(text.to_string())
            }
        });
        let result = scanner("Here is the API key: sk-abc123def456");
        assert!(result.unwrap().contains("[REDACTED]"));
    }

    #[test]
    fn output_scanner_blocks_on_error() {
        let scanner: OutputScanner = Arc::new(|text| {
            if text.contains("sk-") {
                Err("credential leak detected".to_string())
            } else {
                Ok(text.to_string())
            }
        });
        let result = scanner("Key: sk-abc123def456");
        assert!(result.is_err());
    }

    #[test]
    fn with_output_scanner_sets_scanner() {
        let scanner: OutputScanner = Arc::new(|t| Ok(t.to_string()));
        let tool =
            DelegateTool::new(HashMap::new(), 0, noop_builder()).with_output_scanner(scanner);
        assert!(tool.output_scanner.is_some());
    }

    #[test]
    fn with_parent_policy_sets_policy() {
        let policy = AutonomyPolicy {
            level: AutonomyLevel::ReadOnly,
            ..AutonomyPolicy::default()
        };
        let tool =
            DelegateTool::new(HashMap::new(), 0, noop_builder()).with_parent_policy(policy.clone());
        assert!(tool.parent_policy.is_some());
        assert_eq!(tool.parent_policy.unwrap().level, AutonomyLevel::ReadOnly);
    }

    #[test]
    fn with_max_concurrent_sets_semaphore() {
        let tool = DelegateTool::new(HashMap::new(), 0, noop_builder()).with_max_concurrent(2);
        assert_eq!(tool.semaphore.available_permits(), 2);
    }

    #[test]
    fn parent_read_only_blocks_write_tools_in_child() {
        let parent = AutonomyPolicy {
            level: AutonomyLevel::ReadOnly,
            ..AutonomyPolicy::default()
        };
        let child = AutonomyPolicy::default(); // Supervised
        let intersected = parent.intersect(&child);
        // shell should be blocked (write tool in read_only)
        assert!(matches!(
            intersected.check_tool("shell"),
            ApprovalDecision::Blocked { .. }
        ));
        // file_read should be approved
        assert_eq!(
            intersected.check_tool("file_read"),
            ApprovalDecision::Approved
        );
    }

    #[test]
    fn child_depth_incremented() {
        let ctx = test_ctx();
        let mut child_ctx = ctx.clone();
        child_ctx.depth = ctx.depth.saturating_add(1);
        assert_eq!(child_ctx.depth, 1);
    }

    #[test]
    fn child_conversation_id_format() {
        let agent_name = "researcher";
        let depth: u8 = 1;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let cid = format!("delegate-{agent_name}-{depth}-{nanos}");
        assert!(cid.starts_with("delegate-researcher-1-"));
        // Should be unique — different nanos each time.
        let cid2 = format!(
            "delegate-{agent_name}-{depth}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        assert_ne!(cid, cid2);
    }
}
