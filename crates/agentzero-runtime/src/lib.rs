use agentzero_common::local_providers::is_local_provider;
use agentzero_config::{load, load_audit_policy, load_env_var, load_tool_security_policy};
use agentzero_core::{
    Agent, AgentConfig, AuditEvent, AuditSink, HookEvent, HookFailureMode, HookSink, MemoryStore,
    Provider, RuntimeMetrics, Tool, ToolContext, UserMessage,
};
use agentzero_delegation::DelegateConfig;
use agentzero_infra::audit::FileAuditSink;
use agentzero_infra::tools::default_tools;
use agentzero_memory::SqliteMemoryStore;
#[cfg(feature = "memory-turso")]
use agentzero_memory::{TursoMemoryStore, TursoSettings};
use agentzero_providers::OpenAiCompatibleProvider;
use agentzero_routing::{ClassificationRule, EmbeddingRoute, ModelRoute, ModelRouter};
use anyhow::Context;
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Debug, Clone)]
pub struct RunAgentRequest {
    pub workspace_root: PathBuf,
    pub config_path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct RunAgentOutput {
    pub response_text: String,
    pub metrics_snapshot: serde_json::Value,
}

pub struct RuntimeExecution {
    pub config: AgentConfig,
    pub provider: Box<dyn Provider>,
    pub memory: Box<dyn MemoryStore>,
    pub tools: Vec<Box<dyn Tool>>,
    pub audit_sink: Option<Box<dyn AuditSink>>,
    pub hook_sink: Option<Box<dyn HookSink>>,
}

struct AuditHookSink {
    sink: FileAuditSink,
}

#[async_trait]
impl HookSink for AuditHookSink {
    async fn record(&self, event: HookEvent) -> anyhow::Result<()> {
        self.sink
            .record(AuditEvent {
                stage: format!("hook.{}", event.stage),
                detail: json!({ "hook": event.detail }),
            })
            .await
    }
}

pub async fn run_agent_once(req: RunAgentRequest) -> anyhow::Result<RunAgentOutput> {
    let config = load(&req.config_path)?;
    let key = resolve_api_key(&req.config_path, &config.provider.kind)?;

    let router = build_model_router(&config);
    let delegate_agents = build_delegate_agents(&config);
    let provider =
        OpenAiCompatibleProvider::new(config.provider.base_url, key, config.provider.model);
    let memory = build_memory_store(&req.config_path).await?;
    let tool_policy = load_tool_security_policy(&req.workspace_root, &req.config_path)?;
    let tools: Vec<Box<dyn Tool>> = default_tools(&tool_policy, router, delegate_agents)?;
    let audit_policy = load_audit_policy(&req.workspace_root, &req.config_path)?;
    let audit_path = audit_policy.path.clone();
    let execution = RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: config.agent.max_tool_iterations,
            request_timeout_ms: config.agent.request_timeout_ms,
            memory_window_size: config.agent.memory_window_size,
            max_prompt_chars: config.agent.max_prompt_chars,
            parallel_tools: config.agent.parallel_tools,
            gated_tools: config.autonomy.always_ask.iter().cloned().collect(),
            loop_detection_no_progress_threshold: config.agent.loop_detection_no_progress_threshold,
            loop_detection_ping_pong_cycles: config.agent.loop_detection_ping_pong_cycles,
            loop_detection_failure_streak: config.agent.loop_detection_failure_streak,
            research: agentzero_core::ResearchPolicy {
                enabled: config.research.enabled,
                trigger: match config.research.trigger.as_str() {
                    "always" => agentzero_core::ResearchTrigger::Always,
                    "keywords" => agentzero_core::ResearchTrigger::Keywords,
                    "length" => agentzero_core::ResearchTrigger::Length,
                    "question" => agentzero_core::ResearchTrigger::Question,
                    _ => agentzero_core::ResearchTrigger::Never,
                },
                keywords: config.research.keywords.clone(),
                min_message_length: config.research.min_message_length,
                max_iterations: config.research.max_iterations,
                show_progress: config.research.show_progress,
            },
            reasoning: agentzero_core::ReasoningConfig {
                enabled: config.runtime.reasoning_enabled,
                level: config.provider_options.reasoning_level.clone(),
            },
            hooks: agentzero_core::HookPolicy {
                enabled: config.agent.hooks.enabled,
                timeout_ms: config.agent.hooks.timeout_ms,
                fail_closed: config.agent.hooks.fail_closed,
                default_mode: parse_hook_mode(&config.agent.hooks.on_error_default)?,
                low_tier_mode: config
                    .agent
                    .hooks
                    .on_error_low
                    .as_deref()
                    .map(parse_hook_mode)
                    .transpose()?
                    .unwrap_or(parse_hook_mode(&config.agent.hooks.on_error_default)?),
                medium_tier_mode: config
                    .agent
                    .hooks
                    .on_error_medium
                    .as_deref()
                    .map(parse_hook_mode)
                    .transpose()?
                    .unwrap_or(parse_hook_mode(&config.agent.hooks.on_error_default)?),
                high_tier_mode: config
                    .agent
                    .hooks
                    .on_error_high
                    .as_deref()
                    .map(parse_hook_mode)
                    .transpose()?
                    .unwrap_or(parse_hook_mode(&config.agent.hooks.on_error_default)?),
            },
        },
        provider: Box::new(provider),
        memory,
        tools,
        audit_sink: if audit_policy.enabled {
            Some(Box::new(FileAuditSink::new(audit_path.clone())) as Box<dyn AuditSink>)
        } else {
            None
        },
        hook_sink: if config.agent.hooks.enabled {
            Some(Box::new(AuditHookSink {
                sink: FileAuditSink::new(audit_path),
            }) as Box<dyn HookSink>)
        } else {
            None
        },
    };

    run_agent_with_runtime(execution, req.workspace_root, req.message).await
}

fn parse_hook_mode(input: &str) -> anyhow::Result<HookFailureMode> {
    match input.trim() {
        "block" => Ok(HookFailureMode::Block),
        "warn" => Ok(HookFailureMode::Warn),
        "ignore" => Ok(HookFailureMode::Ignore),
        other => {
            anyhow::bail!("invalid hook error mode `{other}`; expected block, warn, or ignore")
        }
    }
}

pub async fn run_agent_with_runtime(
    execution: RuntimeExecution,
    workspace_root: PathBuf,
    message: String,
) -> anyhow::Result<RunAgentOutput> {
    let mut agent = Agent::new(
        execution.config,
        execution.provider,
        execution.memory,
        execution.tools,
    );

    let runtime_metrics = RuntimeMetrics::new();
    agent = agent.with_metrics(Box::new(runtime_metrics.clone()));
    if let Some(audit) = execution.audit_sink {
        agent = agent.with_audit(audit);
    }
    if let Some(hooks) = execution.hook_sink {
        agent = agent.with_hooks(hooks);
    }

    let response = agent
        .respond(
            UserMessage { text: message },
            &ToolContext::new(workspace_root.to_string_lossy().to_string()),
        )
        .await?;
    let metrics_snapshot = runtime_metrics.export_json();
    info!(metrics = %metrics_snapshot, "runtime metrics snapshot");

    Ok(RunAgentOutput {
        response_text: response.text,
        metrics_snapshot,
    })
}

fn resolve_api_key(config_path: &Path, provider_kind: &str) -> anyhow::Result<String> {
    if is_local_provider(provider_kind) {
        return Ok(load_env_var(config_path, "OPENAI_API_KEY")?.unwrap_or_default());
    }
    require_openai_api_key(config_path)
}

fn require_openai_api_key(config_path: &Path) -> anyhow::Result<String> {
    load_env_var(config_path, "OPENAI_API_KEY")?
        .context("missing OPENAI_API_KEY (set env var or .env/.env.local/.env.<environment>)")
}

async fn build_memory_store(config_path: &Path) -> anyhow::Result<Box<dyn MemoryStore>> {
    let config = load(config_path)?;
    match config.memory.backend.as_str() {
        "sqlite" => Ok(Box::new(SqliteMemoryStore::open(
            &config.memory.sqlite_path,
        )?)),
        "turso" => {
            #[cfg(feature = "memory-turso")]
            {
                let settings = TursoSettings::from_env()?;
                let store = TursoMemoryStore::connect(settings).await?;
                Ok(Box::new(store))
            }
            #[cfg(not(feature = "memory-turso"))]
            {
                anyhow::bail!(
                    "memory backend `turso` requested but this binary was built without feature `memory-turso`"
                )
            }
        }
        backend => anyhow::bail!("unsupported memory backend: {backend}"),
    }
}

fn build_model_router(config: &agentzero_config::AgentZeroConfig) -> Option<ModelRouter> {
    if config.model_routes.is_empty()
        && config.embedding_routes.is_empty()
        && config.query_classification.rules.is_empty()
    {
        return None;
    }

    Some(ModelRouter {
        model_routes: config
            .model_routes
            .iter()
            .map(|r| ModelRoute {
                hint: r.hint.clone(),
                provider: r.provider.clone(),
                model: r.model.clone(),
                max_tokens: r.max_tokens,
                api_key: r.api_key.clone(),
                transport: r.transport.clone(),
            })
            .collect(),
        embedding_routes: config
            .embedding_routes
            .iter()
            .map(|r| EmbeddingRoute {
                hint: r.hint.clone(),
                provider: r.provider.clone(),
                model: r.model.clone(),
                dimensions: r.dimensions,
                api_key: r.api_key.clone(),
            })
            .collect(),
        classification_rules: config
            .query_classification
            .rules
            .iter()
            .map(|r| ClassificationRule {
                hint: r.hint.clone(),
                keywords: r.keywords.clone(),
                patterns: r.patterns.clone(),
                min_length: r.min_length,
                max_length: r.max_length,
                priority: r.priority,
            })
            .collect(),
        classification_enabled: config.query_classification.enabled,
    })
}

fn build_delegate_agents(
    config: &agentzero_config::AgentZeroConfig,
) -> Option<HashMap<String, DelegateConfig>> {
    if config.agents.is_empty() {
        return None;
    }

    let map: HashMap<String, DelegateConfig> = config
        .agents
        .iter()
        .map(|(name, agent)| {
            (
                name.clone(),
                DelegateConfig {
                    name: name.clone(),
                    provider: agent.provider.clone(),
                    model: agent.model.clone(),
                    system_prompt: agent.system_prompt.clone(),
                    api_key: agent.api_key.clone(),
                    temperature: agent.temperature,
                    max_depth: agent.max_depth,
                    agentic: agent.agentic,
                    allowed_tools: agent.allowed_tools.iter().cloned().collect(),
                    max_iterations: agent.max_iterations,
                },
            )
        })
        .collect();

    Some(map)
}

#[cfg(test)]
mod tests {
    use super::{parse_hook_mode, require_openai_api_key, resolve_api_key};
    use agentzero_core::HookFailureMode;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agentzero-runtime-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn require_openai_api_key_reads_from_dotenv() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("config file should exist");
        fs::write(dir.join(".env"), "OPENAI_API_KEY=sk-test\n").expect("dotenv should be written");

        temp_env::with_var_unset("OPENAI_API_KEY", || {
            let key = require_openai_api_key(&config_path).expect("api key should resolve");
            assert_eq!(key, "sk-test");
        });

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn require_openai_api_key_fails_when_missing() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("config file should exist");

        temp_env::with_var_unset("OPENAI_API_KEY", || {
            let err = require_openai_api_key(&config_path).expect_err("missing key should fail");
            assert!(err.to_string().contains("missing OPENAI_API_KEY"));
        });

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_api_key_returns_empty_for_local_provider_without_key() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("config file should exist");

        temp_env::with_var_unset("OPENAI_API_KEY", || {
            let key = resolve_api_key(&config_path, "ollama")
                .expect("local provider should not require key");
            assert!(key.is_empty(), "local provider key should be empty string");
        });

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_api_key_returns_key_for_local_provider_when_set() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("config file should exist");
        fs::write(dir.join(".env"), "OPENAI_API_KEY=sk-local\n").expect("dotenv should be written");

        temp_env::with_var_unset("OPENAI_API_KEY", || {
            let key = resolve_api_key(&config_path, "llamacpp")
                .expect("local provider should resolve key");
            assert_eq!(key, "sk-local");
        });

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_api_key_fails_for_cloud_provider_without_key() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("config file should exist");

        temp_env::with_var_unset("OPENAI_API_KEY", || {
            let err = resolve_api_key(&config_path, "openrouter")
                .expect_err("cloud provider should require key");
            assert!(err.to_string().contains("missing OPENAI_API_KEY"));
        });

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_api_key_succeeds_for_all_local_providers() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("config file should exist");

        temp_env::with_var_unset("OPENAI_API_KEY", || {
            for provider in &["ollama", "llamacpp", "lmstudio", "vllm", "sglang"] {
                let result = resolve_api_key(&config_path, provider);
                assert!(
                    result.is_ok(),
                    "resolve_api_key should succeed for local provider '{provider}'"
                );
            }
        });

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn parse_hook_mode_accepts_known_modes_success_path() {
        assert!(matches!(
            parse_hook_mode("block").expect("block should parse"),
            HookFailureMode::Block
        ));
        assert!(matches!(
            parse_hook_mode("warn").expect("warn should parse"),
            HookFailureMode::Warn
        ));
        assert!(matches!(
            parse_hook_mode("ignore").expect("ignore should parse"),
            HookFailureMode::Ignore
        ));
    }

    #[test]
    fn parse_hook_mode_rejects_unknown_mode_negative_path() {
        let err = parse_hook_mode("panic").expect_err("unknown mode should fail");
        assert!(err.to_string().contains("invalid hook error mode"));
    }
}
