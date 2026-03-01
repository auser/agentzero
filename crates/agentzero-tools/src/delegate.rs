use agentzero_core::{
    Agent, AgentConfig, ChatResult, MemoryEntry, MemoryStore, Provider, Tool, ToolContext,
    ToolResult,
};
use agentzero_delegation::{validate_delegation, DelegateConfig, DelegateRequest};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct Input {
    agent: String,
    prompt: String,
}

pub struct DelegateTool {
    agents: HashMap<String, DelegateConfig>,
    current_depth: usize,
}

impl DelegateTool {
    pub fn new(agents: HashMap<String, DelegateConfig>, current_depth: usize) -> Self {
        Self {
            agents,
            current_depth,
        }
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &'static str {
        "delegate"
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

        let api_key = config
            .api_key
            .clone()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .unwrap_or_default();

        let provider = agentzero_providers::OpenAiCompatibleProvider::new(
            config.provider.clone(),
            api_key,
            config.model.clone(),
        );

        let output = if config.agentic {
            run_agentic(provider, config, &parsed.prompt, ctx).await?
        } else {
            run_single_shot(&provider, &parsed.prompt).await?
        };

        Ok(ToolResult { output })
    }
}

async fn run_single_shot(provider: &dyn Provider, prompt: &str) -> anyhow::Result<String> {
    let result: ChatResult = provider.complete(prompt).await?;
    Ok(result.output_text)
}

async fn run_agentic(
    provider: agentzero_providers::OpenAiCompatibleProvider,
    config: &DelegateConfig,
    prompt: &str,
    ctx: &ToolContext,
) -> anyhow::Result<String> {
    let agent_config = AgentConfig {
        max_tool_iterations: config.max_iterations,
        ..Default::default()
    };
    let memory = EphemeralMemory::default();
    let agent = Agent::new(
        agent_config,
        Box::new(provider),
        Box::new(memory),
        vec![], // Sub-agent tools can be populated in a future enhancement.
    );

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

    fn test_agents() -> HashMap<String, DelegateConfig> {
        let mut map = HashMap::new();
        map.insert(
            "researcher".to_string(),
            DelegateConfig {
                name: "researcher".into(),
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
        let tool = DelegateTool::new(test_agents(), 0);
        let result = tool
            .execute(r#"{"agent":"nonexistent","prompt":"hello"}"#, &test_ctx())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown agent"));
    }

    #[tokio::test]
    async fn delegate_depth_exceeded_returns_error() {
        let tool = DelegateTool::new(test_agents(), 3);
        let result = tool
            .execute(r#"{"agent":"researcher","prompt":"hello"}"#, &test_ctx())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("depth limit"));
    }

    #[tokio::test]
    async fn delegate_invalid_input_returns_error() {
        let tool = DelegateTool::new(test_agents(), 0);
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
                provider: "https://api.example.invalid/v1".into(),
                model: "gpt-4o".into(),
                max_depth: 3,
                agentic: true,
                allowed_tools: allowed,
                ..Default::default()
            },
        );
        let tool = DelegateTool::new(agents, 0);
        let result = tool
            .execute(r#"{"agent":"bad","prompt":"hello"}"#, &test_ctx())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("delegate"));
    }
}
