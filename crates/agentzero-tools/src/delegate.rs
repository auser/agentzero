use agentzero_core::{
    Agent, AgentConfig, ChatResult, MemoryEntry, MemoryStore, Provider, Tool, ToolContext,
    ToolResult,
};
use agentzero_delegation::{filter_tools, validate_delegation, DelegateConfig, DelegateRequest};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Function that builds a tool set for sub-agents. The delegate tool calls this
/// when running in agentic mode, then filters the result based on each agent's
/// `allowed_tools` configuration.
pub type ToolBuilder = Arc<dyn Fn() -> anyhow::Result<Vec<Box<dyn Tool>>> + Send + Sync>;

#[derive(Debug, Deserialize)]
struct Input {
    agent: String,
    prompt: String,
}

pub struct DelegateTool {
    agents: HashMap<String, DelegateConfig>,
    current_depth: usize,
    tool_builder: ToolBuilder,
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
        }
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

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
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

        let api_key = resolve_delegate_api_key(config);

        let provider = agentzero_providers::build_provider(
            &config.provider_kind,
            config.provider.clone(),
            api_key,
            config.model.clone(),
        );

        let effective_prompt = match &config.system_prompt {
            Some(sp) => format!("System: {sp}\n\nUser: {}", parsed.prompt),
            None => parsed.prompt.clone(),
        };

        let output = if config.agentic {
            run_agentic(provider, config, &effective_prompt, ctx, &self.tool_builder).await?
        } else {
            run_single_shot(provider.as_ref(), &effective_prompt).await?
        };

        Ok(ToolResult { output })
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
) -> anyhow::Result<String> {
    let agent_config = AgentConfig {
        max_tool_iterations: config.max_iterations,
        ..Default::default()
    };
    let memory = EphemeralMemory::default();

    // Build tools for the sub-agent. The builder creates the full set; we
    // filter to only those in the agent's allowed_tools (filter_tools also
    // excludes "delegate" to prevent infinite chains).
    let all_tools = tool_builder().unwrap_or_else(|_| vec![]);
    let all_tool_names: Vec<String> = all_tools.iter().map(|t| t.name().to_string()).collect();
    let allowed_names = filter_tools(&all_tool_names, &config.allowed_tools);
    let tools: Vec<Box<dyn Tool>> = all_tools
        .into_iter()
        .filter(|t| allowed_names.contains(&t.name().to_string()))
        .collect();

    let agent = Agent::new(agent_config, provider, Box::new(memory), tools);

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
    fn system_prompt_is_prepended_to_user_prompt() {
        let config = DelegateConfig {
            system_prompt: Some("You are a research assistant.".into()),
            ..Default::default()
        };
        let user_prompt = "Find docs about X";
        let effective = match &config.system_prompt {
            Some(sp) => format!("System: {sp}\n\nUser: {user_prompt}"),
            None => user_prompt.to_string(),
        };
        assert!(effective.starts_with("System: You are a research assistant."));
        assert!(effective.ends_with("User: Find docs about X"));
    }

    #[test]
    fn no_system_prompt_passes_user_prompt_unchanged() {
        let config = DelegateConfig::default();
        let user_prompt = "Find docs about X";
        let effective = match &config.system_prompt {
            Some(sp) => format!("System: {sp}\n\nUser: {user_prompt}"),
            None => user_prompt.to_string(),
        };
        assert_eq!(effective, "Find docs about X");
    }

    #[test]
    fn tool_builder_filters_by_allowed_tools() {
        use agentzero_core::{ToolContext, ToolResult};

        // A simple test tool.
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

        // Build with an allowlist of just "read_file".
        let all_tools = builder().unwrap();
        let all_names: Vec<String> = all_tools.iter().map(|t| t.name().to_string()).collect();
        let mut allowed = HashSet::new();
        allowed.insert("read_file".to_string());
        let filtered = filter_tools(&all_names, &allowed);
        assert_eq!(filtered, vec!["read_file".to_string()]);

        // Build with an empty allowlist (all except delegate).
        let filtered_all = filter_tools(&all_names, &HashSet::new());
        assert!(filtered_all.contains(&"read_file".to_string()));
        assert!(filtered_all.contains(&"shell".to_string()));
        assert!(filtered_all.contains(&"web_search".to_string()));
        assert!(!filtered_all.contains(&"delegate".to_string()));
    }
}
