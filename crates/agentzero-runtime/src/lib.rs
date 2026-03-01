use agentzero_config::{load, load_audit_policy, load_env_var, load_tool_security_policy};
use agentzero_core::{
    Agent, AgentConfig, AuditEvent, AuditSink, HookEvent, HookFailureMode, HookSink, MemoryStore,
    Provider, RuntimeMetrics, Tool, ToolContext, UserMessage,
};
use agentzero_infra::audit::FileAuditSink;
use agentzero_infra::tools::default_tools;
use agentzero_memory::SqliteMemoryStore;
#[cfg(feature = "memory-turso")]
use agentzero_memory::{TursoMemoryStore, TursoSettings};
use agentzero_providers::OpenAiCompatibleProvider;
use anyhow::Context;
use async_trait::async_trait;
use serde_json::json;
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
    let key = require_openai_api_key(&req.config_path)?;
    let config = load(&req.config_path)?;

    let provider =
        OpenAiCompatibleProvider::new(config.provider.base_url, key, config.provider.model);
    let memory = build_memory_store(&req.config_path).await?;
    let tool_policy = load_tool_security_policy(&req.workspace_root, &req.config_path)?;
    let tools: Vec<Box<dyn Tool>> = default_tools(&tool_policy)?;
    let audit_policy = load_audit_policy(&req.workspace_root, &req.config_path)?;
    let audit_path = audit_policy.path.clone();
    let execution = RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: config.agent.max_tool_iterations,
            request_timeout_ms: config.agent.request_timeout_ms,
            memory_window_size: config.agent.memory_window_size,
            max_prompt_chars: config.agent.max_prompt_chars,
            parallel_tools: config.agent.parallel_tools,
            loop_detection_no_progress_threshold: config.agent.loop_detection_no_progress_threshold,
            loop_detection_ping_pong_cycles: config.agent.loop_detection_ping_pong_cycles,
            loop_detection_failure_streak: config.agent.loop_detection_failure_streak,
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

#[cfg(test)]
mod tests {
    use super::{parse_hook_mode, require_openai_api_key};
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
