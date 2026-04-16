use crate::autonomy::AutonomyPolicy;
use crate::task_manager::TaskManager;
use agentzero_core::delegation::{
    filter_tools, validate_delegation, DelegateConfig, DelegateRequest,
};
use agentzero_core::{Agent, AgentConfig, ChatResult, Provider, Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
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

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct DelegateSchema {
    /// Action to perform: 'delegate' (default), 'check_result', 'list_results', 'cancel_task'
    #[schema(enum_values = ["delegate", "check_result", "list_results", "cancel_task"])]
    #[serde(default)]
    action: Option<String>,
    /// Name of the sub-agent to delegate to (required for 'delegate' action)
    #[serde(default)]
    agent: Option<String>,
    /// The prompt/task to send to the sub-agent (required for 'delegate' action)
    #[serde(default)]
    prompt: Option<String>,
    /// Run delegation in background mode — returns task_id immediately without waiting for completion
    #[serde(default)]
    background: Option<bool>,
    /// Task ID to check or cancel (required for 'check_result' and 'cancel_task' actions)
    #[serde(default)]
    task_id: Option<String>,
}

fn default_action() -> String {
    "delegate".to_string()
}

#[derive(Debug, Deserialize)]
struct Input {
    /// Action: "delegate" (default), "check_result", "list_results", "cancel_task"
    #[serde(default = "default_action")]
    action: String,
    /// Agent name (required for delegate)
    agent: Option<String>,
    /// Prompt (required for delegate)
    prompt: Option<String>,
    /// Run in background mode — returns task_id immediately
    #[serde(default)]
    background: bool,
    /// Multiple agents to delegate to in parallel (fan-out mode).
    /// When set, each agent receives the same prompt and results are collected.
    #[serde(default)]
    agents: Option<Vec<String>>,
    /// Task ID (required for check_result, cancel_task)
    task_id: Option<String>,
}

#[tool(
    name = "delegate",
    description = "Delegate a subtask to a named sub-agent. Supports synchronous, background (fire-and-forget), and task lifecycle management (check, list, cancel)."
)]
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
    /// Optional task manager for background delegation.
    task_manager: Option<Arc<TaskManager>>,
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
            task_manager: None,
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

    /// Set the task manager for background delegation support.
    pub fn with_task_manager(mut self, tm: Arc<TaskManager>) -> Self {
        self.task_manager = Some(tm);
        self
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(DelegateSchema::schema())
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

        match parsed.action.as_str() {
            "delegate" => {
                // Parallel fan-out when `agents` is provided.
                if parsed.agents.is_some() {
                    self.execute_parallel(parsed, ctx).await
                } else {
                    self.execute_delegate(parsed, ctx).await
                }
            }
            "check_result" => self.execute_check_result(parsed).await,
            "list_results" => self.execute_list_results().await,
            "cancel_task" => self.execute_cancel_task(parsed).await,
            other => Err(anyhow::anyhow!("unknown action: {other}")),
        }
    }
}

impl DelegateTool {
    /// Execute the delegate action (synchronous or background).
    async fn execute_delegate(
        &self,
        parsed: Input,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let agent_name = parsed
            .agent
            .ok_or_else(|| anyhow::anyhow!("'agent' is required for delegate action"))?;
        let prompt = parsed
            .prompt
            .ok_or_else(|| anyhow::anyhow!("'prompt' is required for delegate action"))?;

        let config = self
            .agents
            .get(&agent_name)
            .ok_or_else(|| anyhow::anyhow!("unknown agent: {agent_name}"))?;

        let request = DelegateRequest {
            agent_name: agent_name.clone(),
            prompt: prompt.clone(),
            current_depth: self.current_depth,
        };
        validate_delegation(&request, config)?;

        if parsed.background {
            // Background mode: spawn via task manager and return task_id immediately.
            let tm = self
                .task_manager
                .as_ref()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "background mode requires a TaskManager — use with_task_manager()"
                    )
                })?
                .clone();

            // Capture everything needed for the background future.
            let config = config.clone();
            let tool_builder = self.tool_builder.clone();
            let parent_policy = self.parent_policy.clone();
            let output_scanner = self.output_scanner.clone();
            let semaphore = self.semaphore.clone();
            let child_ctx = build_child_ctx(ctx, &config, &agent_name);

            let bg_agent_name = agent_name.clone();
            let task_id = tm
                .spawn_background(agent_name, async move {
                    // Acquire concurrency permit.
                    let _permit = semaphore
                        .acquire()
                        .await
                        .map_err(|_| anyhow::anyhow!("delegation semaphore closed"))?;

                    let output = run_delegation(
                        &config,
                        &prompt,
                        &child_ctx,
                        &tool_builder,
                        parent_policy.as_ref(),
                    )
                    .await?;

                    // Leak guard: scan output.
                    let safe_output =
                        apply_output_scanner(output_scanner.as_ref(), &output, &bg_agent_name)?;
                    Ok(safe_output)
                })
                .await;

            Ok(ToolResult {
                output: serde_json::json!({
                    "status": "spawned",
                    "task_id": task_id,
                    "message": "Task started in background. Use action='check_result' with the task_id to poll for results."
                })
                .to_string(),
            })
        } else {
            // Synchronous mode: existing behavior.
            let _permit = self
                .semaphore
                .acquire()
                .await
                .map_err(|_| anyhow::anyhow!("delegation semaphore closed"))?;

            let child_ctx = build_child_ctx(ctx, config, &agent_name);

            let output = run_delegation(
                config,
                &prompt,
                &child_ctx,
                &self.tool_builder,
                self.parent_policy.as_ref(),
            )
            .await?;

            tracing::info!(
                agent = %agent_name,
                child_conversation_id = child_ctx.conversation_id.as_deref().unwrap_or(""),
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

            // Leak guard: scan sub-agent output.
            let safe_output =
                apply_output_scanner(self.output_scanner.as_ref(), &output, &agent_name)?;

            Ok(ToolResult {
                output: safe_output,
            })
        }
    }

    /// Execute parallel fan-out: delegate the same prompt to multiple agents concurrently.
    async fn execute_parallel(
        &self,
        parsed: Input,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let agent_names = parsed
            .agents
            .ok_or_else(|| anyhow::anyhow!("'agents' is required for parallel delegation"))?;
        let prompt = parsed
            .prompt
            .ok_or_else(|| anyhow::anyhow!("'prompt' is required for delegate action"))?;

        if agent_names.is_empty() {
            return Err(anyhow::anyhow!("'agents' must not be empty"));
        }

        // Validate all agents exist and pass delegation checks.
        let mut configs = Vec::with_capacity(agent_names.len());
        for name in &agent_names {
            let config = self
                .agents
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("unknown agent: {name}"))?;
            let request = DelegateRequest {
                agent_name: name.clone(),
                prompt: prompt.clone(),
                current_depth: self.current_depth,
            };
            validate_delegation(&request, config)?;
            configs.push((name.clone(), config.clone()));
        }

        // Fan-out: spawn each agent concurrently, respect semaphore.
        let mut set = tokio::task::JoinSet::new();
        for (name, config) in configs {
            let prompt = prompt.clone();
            let tool_builder = self.tool_builder.clone();
            let parent_policy = self.parent_policy.clone();
            let output_scanner = self.output_scanner.clone();
            let semaphore = self.semaphore.clone();
            let child_ctx = build_child_ctx(ctx, &config, &name);
            let agent_name = name.clone();

            set.spawn(async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|_| anyhow::anyhow!("delegation semaphore closed"))?;

                let output = run_delegation(
                    &config,
                    &prompt,
                    &child_ctx,
                    &tool_builder,
                    parent_policy.as_ref(),
                )
                .await?;

                let safe_output =
                    apply_output_scanner(output_scanner.as_ref(), &output, &agent_name)?;
                Ok::<(String, String, u64, u64), anyhow::Error>((
                    agent_name,
                    safe_output,
                    child_ctx.current_tokens(),
                    child_ctx.current_cost(),
                ))
            });
        }

        // Collect results.
        let mut results = Vec::new();
        while let Some(join_result) = set.join_next().await {
            match join_result {
                Ok(Ok((name, output, tokens, cost))) => {
                    ctx.add_tokens(tokens);
                    ctx.add_cost(cost);
                    results.push(serde_json::json!({
                        "agent": name,
                        "status": "completed",
                        "output": output,
                    }));
                }
                Ok(Err(e)) => {
                    results.push(serde_json::json!({
                        "agent": "unknown",
                        "status": "failed",
                        "error": e.to_string(),
                    }));
                }
                Err(e) => {
                    results.push(serde_json::json!({
                        "agent": "unknown",
                        "status": "failed",
                        "error": format!("join error: {e}"),
                    }));
                }
            }
        }

        Ok(ToolResult {
            output: serde_json::to_string(&results).unwrap_or_default(),
        })
    }

    /// Check the status of a background task.
    async fn execute_check_result(&self, parsed: Input) -> anyhow::Result<ToolResult> {
        let tm = self
            .task_manager
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no TaskManager configured"))?;
        let task_id = parsed
            .task_id
            .ok_or_else(|| anyhow::anyhow!("'task_id' is required for check_result action"))?;

        match tm.check_result(&task_id).await {
            Some(status) => Ok(ToolResult {
                output: serde_json::to_string(&serde_json::json!({
                    "task_id": task_id,
                    "result": status,
                }))
                .unwrap_or_default(),
            }),
            None => Ok(ToolResult {
                output: serde_json::json!({
                    "task_id": task_id,
                    "error": "task not found"
                })
                .to_string(),
            }),
        }
    }

    /// List all background tasks.
    async fn execute_list_results(&self) -> anyhow::Result<ToolResult> {
        let tm = self
            .task_manager
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no TaskManager configured"))?;

        let results = tm.list_results().await;
        let entries: Vec<serde_json::Value> = results
            .into_iter()
            .map(|(id, agent, status, created_at)| {
                serde_json::json!({
                    "task_id": id,
                    "agent": agent,
                    "status": status,
                    "created_at": created_at,
                })
            })
            .collect();

        Ok(ToolResult {
            output: serde_json::to_string(&entries).unwrap_or_default(),
        })
    }

    /// Cancel a background task.
    async fn execute_cancel_task(&self, parsed: Input) -> anyhow::Result<ToolResult> {
        let tm = self
            .task_manager
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no TaskManager configured"))?;
        let task_id = parsed
            .task_id
            .ok_or_else(|| anyhow::anyhow!("'task_id' is required for cancel_task action"))?;

        let cancelled = tm.cancel_task(&task_id).await;
        Ok(ToolResult {
            output: serde_json::json!({
                "task_id": task_id,
                "cancelled": cancelled,
            })
            .to_string(),
        })
    }
}

/// Build a child `ToolContext` with incremented depth and fresh budget counters.
fn build_child_ctx(ctx: &ToolContext, config: &DelegateConfig, agent_name: &str) -> ToolContext {
    let mut child_ctx = ctx.clone();
    child_ctx.depth = ctx.depth.saturating_add(1);

    // Fresh accumulators for the child — usage is aggregated back after completion.
    child_ctx.tokens_used = Arc::new(std::sync::atomic::AtomicU64::new(0));
    child_ctx.cost_microdollars = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Set child budget limits from delegate config (0 = inherit parent's remaining budget).
    if config.max_tokens > 0 {
        child_ctx.max_tokens = config.max_tokens;
    } else if ctx.max_tokens > 0 {
        let used = ctx.current_tokens();
        child_ctx.max_tokens = ctx.max_tokens.saturating_sub(used);
    }
    if config.max_cost_microdollars > 0 {
        child_ctx.max_cost_microdollars = config.max_cost_microdollars;
    } else if ctx.max_cost_microdollars > 0 {
        let used = ctx.current_cost();
        child_ctx.max_cost_microdollars = ctx.max_cost_microdollars.saturating_sub(used);
    }

    // Assign a unique conversation_id to the child.
    let child_conversation_id = format!(
        "delegate-{}-{}-{}",
        agent_name,
        ctx.depth.saturating_add(1),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    child_ctx.conversation_id = Some(child_conversation_id);

    // Phase K — Sprint 90: propagate the child's capability set (already computed
    // as root ∩ agent_caps in build_delegate_agents) into the child ToolContext.
    // This ensures memory tools and nested delegations enforce the child's scope.
    child_ctx.capability_set = config.capability_set.clone();

    child_ctx
}

/// Run the actual delegation (single-shot or agentic) and return the raw output.
async fn run_delegation(
    config: &DelegateConfig,
    prompt: &str,
    child_ctx: &ToolContext,
    tool_builder: &ToolBuilder,
    parent_policy: Option<&AutonomyPolicy>,
) -> anyhow::Result<String> {
    let api_key = resolve_delegate_api_key(config);

    let provider = agentzero_providers::build_provider(
        &config.provider_kind,
        config.provider.clone(),
        api_key,
        config.model.clone(),
    );

    // Apply instruction method to prepare the system prompt.
    let (prepared_prompt, _extra_tool_def) = agentzero_core::delegation::prepare_instructions(
        config.system_prompt.as_deref(),
        &config.instruction_method,
    );

    if config.agentic {
        // Override the system_prompt in config with the prepared version.
        let mut effective_config = config.clone();
        effective_config.system_prompt = prepared_prompt;
        run_agentic(
            provider,
            &effective_config,
            prompt,
            child_ctx,
            tool_builder,
            parent_policy,
        )
        .await
    } else {
        let effective_prompt = match &prepared_prompt {
            Some(sp) => format!("<system>{sp}</system>\n{prompt}"),
            None => prompt.to_string(),
        };
        run_single_shot(provider.as_ref(), &effective_prompt).await
    }
}

/// Apply the output scanner (leak guard) if configured.
fn apply_output_scanner(
    scanner: Option<&OutputScanner>,
    output: &str,
    agent_name: &str,
) -> anyhow::Result<String> {
    if let Some(scanner) = scanner {
        scanner(output).map_err(|blocked| {
            tracing::warn!(
                agent = %agent_name,
                "delegation output blocked by leak guard: {blocked}"
            );
            anyhow::anyhow!(
                "delegation output blocked: credential leak detected in sub-agent response"
            )
        })
    } else {
        Ok(output.to_string())
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
    let memory = agentzero_core::EphemeralMemory::default();

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

    // Phase B: apply the delegate's capability set if non-empty.
    // When capability_set.is_empty() (the default), this is a no-op and the
    // sub-agent inherits the full tool set built above. When non-empty (set by
    // build_delegate_agents as root ∩ per-agent caps), tools not permitted by
    // the capability set are removed so the sub-agent can never exceed its
    // assigned capability scope.
    let effective_tools: Vec<Box<dyn Tool>> = if !config.capability_set.is_empty() {
        effective_tools
            .into_iter()
            .filter(|t| config.capability_set.allows_tool(t.name()))
            .collect()
    } else {
        effective_tools
    };

    // Phase K — Sprint 90: apply Delegate { max_capabilities } ceiling from the
    // parent's capability set. If the parent's CapabilitySet contains any
    // Capability::Delegate { max_capabilities } grants, those form an additional
    // ceiling on the tools the sub-agent may receive.
    let effective_tools: Vec<Box<dyn Tool>> = {
        let ceiling = ctx.capability_set.delegate_ceiling();
        if !ceiling.is_empty() {
            effective_tools
                .into_iter()
                .filter(|t| ceiling.allows_tool(t.name()))
                .collect()
        } else {
            effective_tools
        }
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

        let all_tools = builder().expect("builder should not fail");
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
            .expect("cancelled should return Ok");
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
        assert!(result
            .expect("scanner should succeed")
            .contains("[REDACTED]"));
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
        assert_eq!(
            tool.parent_policy.expect("should have policy").level,
            AutonomyLevel::ReadOnly
        );
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
            .unwrap_or_default()
            .as_nanos();
        let cid = format!("delegate-{agent_name}-{depth}-{nanos}");
        assert!(cid.starts_with("delegate-researcher-1-"));
        // Should be unique — different nanos each time.
        std::thread::sleep(std::time::Duration::from_millis(1));
        let cid2 = format!(
            "delegate-{agent_name}-{depth}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        assert_ne!(cid, cid2);
    }

    #[test]
    fn with_task_manager_sets_manager() {
        let tm = Arc::new(TaskManager::new());
        let tool =
            DelegateTool::new(HashMap::new(), 0, noop_builder()).with_task_manager(tm.clone());
        assert!(tool.task_manager.is_some());
    }

    #[tokio::test]
    async fn delegate_background_requires_task_manager() {
        let tool = DelegateTool::new(test_agents(), 0, noop_builder());
        let result = tool
            .execute(
                r#"{"agent":"researcher","prompt":"hello","background":true}"#,
                &test_ctx(),
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("TaskManager"));
    }

    #[tokio::test]
    async fn check_result_requires_task_manager() {
        let tool = DelegateTool::new(test_agents(), 0, noop_builder());
        let result = tool
            .execute(
                r#"{"action":"check_result","task_id":"task-123"}"#,
                &test_ctx(),
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("TaskManager"));
    }

    #[tokio::test]
    async fn list_results_requires_task_manager() {
        let tool = DelegateTool::new(test_agents(), 0, noop_builder());
        let result = tool
            .execute(r#"{"action":"list_results"}"#, &test_ctx())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("TaskManager"));
    }

    #[tokio::test]
    async fn cancel_task_requires_task_id() {
        let tm = Arc::new(TaskManager::new());
        let tool = DelegateTool::new(test_agents(), 0, noop_builder()).with_task_manager(tm);
        let result = tool
            .execute(r#"{"action":"cancel_task"}"#, &test_ctx())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("task_id"));
    }

    #[tokio::test]
    async fn delegate_action_requires_agent_field() {
        let tool = DelegateTool::new(test_agents(), 0, noop_builder());
        let result = tool.execute(r#"{"prompt":"hello"}"#, &test_ctx()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("agent"));
    }

    #[tokio::test]
    async fn delegate_action_requires_prompt_field() {
        let tool = DelegateTool::new(test_agents(), 0, noop_builder());
        let result = tool.execute(r#"{"agent":"researcher"}"#, &test_ctx()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("prompt"));
    }

    #[tokio::test]
    async fn unknown_action_returns_error() {
        let tool = DelegateTool::new(test_agents(), 0, noop_builder());
        let result = tool.execute(r#"{"action":"explode"}"#, &test_ctx()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown action"));
    }

    #[tokio::test]
    async fn backward_compat_old_input_format() {
        // The old format { "agent": "...", "prompt": "..." } should still work.
        let tool = DelegateTool::new(test_agents(), 0, noop_builder());
        // This will fail at the provider level (no real API), but it should parse
        // the input correctly and get past validation. We check it fails with
        // a provider error, not a parsing error.
        let result = tool
            .execute(r#"{"agent":"researcher","prompt":"hello"}"#, &test_ctx())
            .await;
        // It should fail because there's no real provider, but NOT with "invalid input"
        // or "agent is required" errors — proving backward compatibility.
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            !err.contains("invalid input"),
            "should not be a parsing error: {err}"
        );
        assert!(
            !err.contains("agent"),
            "should not be a missing-agent error: {err}"
        );
    }

    #[test]
    fn delegate_ceiling_filters_child_tools() {
        use agentzero_core::security::capability::{Capability, CapabilitySet};
        // Simulate: parent has Delegate { max_capabilities: [web_search] }
        // Child should only get web_search, even if its config would allow more.
        let parent_cap_set = CapabilitySet::new(
            vec![Capability::Delegate {
                max_capabilities: vec![
                    Capability::Tool { name: "web_search".to_string() },
                ],
            }],
            vec![],
        );
        let ceiling = parent_cap_set.delegate_ceiling();
        assert!(!ceiling.is_empty());
        assert!(ceiling.allows_tool("web_search"));
        assert!(!ceiling.allows_tool("shell"));
        assert!(!ceiling.allows_tool("memory_store"));
    }

    #[test]
    fn build_child_ctx_propagates_capability_set() {
        use agentzero_core::security::capability::{Capability, CapabilitySet};
        use agentzero_core::delegation::DelegateConfig;
        use agentzero_core::ToolContext;

        let cap = CapabilitySet::new(
            vec![Capability::Tool { name: "web_search".to_string() }],
            vec![],
        );
        let mut config = DelegateConfig::default();
        config.capability_set = cap.clone();

        let parent_ctx = ToolContext::new("/tmp".to_string());
        let child_ctx = super::build_child_ctx(&parent_ctx, &config, "test-agent");

        assert!(child_ctx.capability_set.allows_tool("web_search"));
        assert!(!child_ctx.capability_set.allows_tool("shell"));
    }

}
