use crate::audit::FileAuditSink;
use crate::tools::default_tools;
use agentzero_auth::AuthManager;
use agentzero_config::{load, load_audit_policy, load_env_var, load_tool_security_policy};
use agentzero_core::common::local_providers::{is_local_provider, local_provider_meta};
use agentzero_core::delegation::DelegateConfig;
use agentzero_core::routing::{ClassificationRule, EmbeddingRoute, ModelRoute, ModelRouter};
use agentzero_core::{
    Agent, AgentConfig, AuditEvent, AuditSink, HookEvent, HookFailureMode, HookSink, MemoryStore,
    Provider, RuntimeMetrics, Tool, ToolContext, UserMessage,
};
use agentzero_providers::{find_models_for_provider, find_provider, model_capabilities};
use agentzero_storage::memory::SqliteMemoryStore;
#[cfg(feature = "memory-turso")]
use agentzero_storage::memory::{TursoMemoryStore, TursoSettings};
use agentzero_storage::StorageKey;
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::info;

pub struct RunAgentRequest {
    pub workspace_root: PathBuf,
    pub config_path: PathBuf,
    pub message: String,
    /// Override the provider kind from config (e.g. "openrouter", "openai-codex").
    pub provider_override: Option<String>,
    /// Override the model name from config.
    pub model_override: Option<String>,
    /// Use a specific auth profile by name (from `auth list`).
    pub profile_override: Option<String>,
    /// Additional tools injected by the caller (e.g. FFI-registered tools).
    /// These are appended to the tools built from the security policy.
    pub extra_tools: Vec<Box<dyn Tool>>,
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

/// Build a `RuntimeExecution` from a `RunAgentRequest`. This resolves config,
/// API keys, provider, memory, tools, audit, and hooks — everything needed to
/// run the agent. Extracted so both `run_agent_once` and streaming callers can
/// share the setup logic.
pub async fn build_runtime_execution(req: RunAgentRequest) -> anyhow::Result<RuntimeExecution> {
    let mut config = load(&req.config_path)?;

    // Apply CLI overrides before resolving the API key / constructing provider.
    if let Some(ref kind) = req.provider_override {
        config.provider.kind = kind.clone();
        // Auto-resolve the base_url: local providers → localhost, cloud → catalog URL.
        if let Some(meta) = local_provider_meta(kind) {
            config.provider.base_url = meta.default_base_url.to_string();
        } else if let Some(descriptor) = find_provider(kind) {
            if let Some(url) = descriptor.default_base_url {
                config.provider.base_url = url.to_string();
            }
        }
    }
    if let Some(ref model) = req.model_override {
        config.provider.model = model.clone();
    }

    let key = resolve_api_key(
        &req.config_path,
        &mut config,
        req.profile_override.as_deref(),
        req.provider_override.is_some(),
    )?;

    let router = build_model_router(&config);
    let delegate_agents = build_delegate_agents(&config);

    // Wire transport settings from [provider.transport] in config.
    let transport_config = agentzero_providers::TransportConfig {
        timeout_ms: config.provider.transport.timeout_ms,
        max_retries: config.provider.transport.max_retries,
        circuit_breaker_threshold: config.provider.transport.circuit_breaker_threshold,
        circuit_breaker_reset_ms: config.provider.transport.circuit_breaker_reset_ms,
    };

    // Look up model capabilities for the agent loop.
    let caps = model_capabilities(&config.provider.kind, &config.provider.model);

    let provider = agentzero_providers::build_provider_with_transport(
        &config.provider.kind,
        config.provider.base_url.clone(),
        key,
        config.provider.model.clone(),
        transport_config,
    );
    let memory = build_memory_store(&req.config_path).await?;
    let tool_policy = load_tool_security_policy(&req.workspace_root, &req.config_path)?;
    let mut tools: Vec<Box<dyn Tool>> = default_tools(&tool_policy, router, delegate_agents)?;
    // Append any extra tools (e.g. FFI-registered tools).
    tools.extend(req.extra_tools);
    let audit_policy = load_audit_policy(&req.workspace_root, &req.config_path)?;
    let audit_path = audit_policy.path.clone();

    Ok(RuntimeExecution {
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
            model_supports_tool_use: caps.is_some_and(|c| c.tool_use),
            model_supports_vision: caps.is_some_and(|c| c.vision),
            system_prompt: config.agent.system_prompt.clone(),
        },
        provider,
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
    })
}

pub async fn run_agent_once(req: RunAgentRequest) -> anyhow::Result<RunAgentOutput> {
    let message = req.message.clone();
    let workspace_root = req.workspace_root.clone();
    let execution = build_runtime_execution(req).await?;
    run_agent_with_runtime(execution, workspace_root, message).await
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

/// Streaming variant of `run_agent_once`. Returns a receiver for incremental
/// `StreamChunk`s and a join handle that resolves to the final `RunAgentOutput`.
pub fn run_agent_streaming(
    execution: RuntimeExecution,
    workspace_root: PathBuf,
    message: String,
) -> (
    tokio::sync::mpsc::UnboundedReceiver<agentzero_core::StreamChunk>,
    tokio::task::JoinHandle<anyhow::Result<RunAgentOutput>>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
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
            .respond_streaming(
                UserMessage { text: message },
                &ToolContext::new(workspace_root.to_string_lossy().to_string()),
                tx,
            )
            .await?;
        let metrics_snapshot = runtime_metrics.export_json();
        info!(metrics = %metrics_snapshot, "streaming runtime metrics snapshot");

        Ok(RunAgentOutput {
            response_text: response.text,
            metrics_snapshot,
        })
    });
    (rx, handle)
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

/// Unified API key resolution. Priority:
///   1. If `profile_override` is set, use that profile's token directly.
///   2. `OPENAI_API_KEY` env var / `.env` files (local providers: optional).
///   3. Auth profile (provider match, then any active profile).
///   4. Error.
///
/// When an auth profile provides credentials and `--provider` was not given,
/// the config's provider kind, base URL, and model are updated to match.
fn resolve_api_key(
    config_path: &Path,
    config: &mut agentzero_config::AgentZeroConfig,
    profile_override: Option<&str>,
    provider_was_overridden: bool,
) -> anyhow::Result<String> {
    use agentzero_auth::CredentialSource;

    // --- explicit --profile flag: handled by resolve_credential ---
    if profile_override.is_some() {
        let config_dir = config_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("config path has no parent directory"))?;
        let manager = AuthManager::in_config_dir(config_dir)?;
        let cred = manager
            .resolve_credential(profile_override, &config.provider.kind)?
            .ok_or_else(|| anyhow::anyhow!("no auth credential found"))?;
        if !provider_was_overridden {
            apply_provider_defaults(config, &cred.provider);
        }
        info!("using auth profile (provider '{}')", cred.provider);
        return Ok(cred.token);
    }

    // --- no explicit profile ---
    // For local providers, a key is optional.
    if is_local_provider(&config.provider.kind) {
        return Ok(load_env_var(config_path, "OPENAI_API_KEY")?.unwrap_or_default());
    }

    // Env var takes highest priority for cloud providers.
    if let Some(key) = load_env_var(config_path, "OPENAI_API_KEY")? {
        return Ok(key);
    }

    // Auth profiles: provider match then any active profile.
    // Before resolving, attempt auto-refresh if the token is expired.
    if let Some(config_dir) = config_path.parent() {
        if let Ok(manager) = AuthManager::in_config_dir(config_dir) {
            // Attempt auto-refresh for expired OAuth tokens.
            if let Ok(Some(refresh_result)) =
                manager.refresh_for_provider(&config.provider.kind, None)
            {
                if refresh_result.status == agentzero_auth::RefreshStatus::Refreshed {
                    info!(
                        "auto-refreshed token for profile '{}'",
                        refresh_result.profile
                    );
                }
            }
            if let Ok(Some(cred)) = manager.resolve_credential(None, &config.provider.kind) {
                if matches!(cred.source, CredentialSource::ActiveProfile(_))
                    && !provider_was_overridden
                {
                    apply_provider_defaults(config, &cred.provider);
                }
                info!("using auth credential for provider '{}'", cred.provider);
                return Ok(cred.token);
            }
        }
    }

    anyhow::bail!(
        "missing API key for provider '{}': \
         set OPENAI_API_KEY (env var or .env) or run `agentzero auth login`",
        config.provider.kind
    )
}

/// Core key resolution by provider kind (env var -> auth profile -> error).
/// Used by unit tests; the main path uses `resolve_api_key` which also falls
/// back to any active profile.
#[cfg(test)]
fn resolve_api_key_for_provider(config_path: &Path, provider_kind: &str) -> anyhow::Result<String> {
    if is_local_provider(provider_kind) {
        return Ok(load_env_var(config_path, "OPENAI_API_KEY")?.unwrap_or_default());
    }

    if let Some(key) = load_env_var(config_path, "OPENAI_API_KEY")? {
        return Ok(key);
    }

    if let Some(config_dir) = config_path.parent() {
        if let Ok(manager) = AuthManager::in_config_dir(config_dir) {
            if let Ok(Some(token)) = manager.active_token_for_provider(provider_kind) {
                if !token.trim().is_empty() {
                    info!("using auth profile token for provider '{provider_kind}'");
                    return Ok(token);
                }
            }
        }
    }

    anyhow::bail!(
        "missing API key for provider '{provider_kind}': \
         set OPENAI_API_KEY (env var or .env) or run `agentzero auth login`"
    )
}

/// Update config's provider base URL and default model to match a provider kind.
fn apply_provider_defaults(config: &mut agentzero_config::AgentZeroConfig, provider_kind: &str) {
    if config.provider.kind != provider_kind {
        config.provider.kind = provider_kind.to_string();
    }
    if let Some(meta) = local_provider_meta(provider_kind) {
        config.provider.base_url = meta.default_base_url.to_string();
    } else if let Some(descriptor) = find_provider(provider_kind) {
        if let Some(url) = descriptor.default_base_url {
            config.provider.base_url = url.to_string();
        }
    }
    // If the current model doesn't belong to this provider, pick the default model.
    if let Some((_, models)) = find_models_for_provider(provider_kind) {
        let model_matches = models
            .iter()
            .any(|m| m.id == config.provider.model.as_str());
        if !model_matches {
            if let Some(default_model) = models.iter().find(|m| m.is_default) {
                info!(
                    "switching model from '{}' to '{}' for provider '{provider_kind}'",
                    config.provider.model, default_model.id,
                );
                config.provider.model = default_model.id.to_string();
            }
        }
    }
}

async fn build_memory_store(config_path: &Path) -> anyhow::Result<Box<dyn MemoryStore>> {
    let config = load(config_path)?;
    match config.memory.backend.as_str() {
        "sqlite" => {
            let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
            let key = StorageKey::from_config_dir(config_dir)?;
            Ok(Box::new(SqliteMemoryStore::open(
                &config.memory.sqlite_path,
                Some(&key),
            )?))
        }
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
            let provider_kind = agent.provider.clone();
            let base_url = resolve_delegate_base_url(&provider_kind);

            (
                name.clone(),
                DelegateConfig {
                    name: name.clone(),
                    provider_kind,
                    provider: base_url,
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

/// Resolve a provider kind string to a base URL. If the kind looks like a URL
/// already (starts with `http://` or `https://`), return it as-is. Otherwise
/// look it up in the provider catalog.
fn resolve_delegate_base_url(provider_kind: &str) -> String {
    if provider_kind.starts_with("http://") || provider_kind.starts_with("https://") {
        return provider_kind.to_string();
    }
    find_provider(provider_kind)
        .and_then(|desc| desc.default_base_url)
        .map(|url| url.to_string())
        .unwrap_or_else(|| provider_kind.to_string())
}

#[cfg(test)]
mod tests {
    use super::{parse_hook_mode, resolve_api_key_for_provider};
    use agentzero_auth::AuthManager;
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
        let dir = std::env::temp_dir().join(format!(
            "agentzero-runtime-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn resolve_api_key_reads_from_dotenv_for_cloud_provider() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("config file should exist");
        fs::write(dir.join(".env"), "OPENAI_API_KEY=sk-test\n").expect("dotenv should be written");

        temp_env::with_var_unset("OPENAI_API_KEY", || {
            let key = resolve_api_key_for_provider(&config_path, "openrouter")
                .expect("cloud provider api key should resolve from dotenv");
            assert_eq!(key, "sk-test");
        });

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_api_key_falls_back_to_auth_profile() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("config file should exist");

        let manager = AuthManager::in_config_dir(&dir).expect("auth manager should construct");
        manager
            .login("default", "openrouter", "tok-from-profile", true)
            .expect("auth login should succeed");

        temp_env::with_var_unset("OPENAI_API_KEY", || {
            let key = resolve_api_key_for_provider(&config_path, "openrouter")
                .expect("should fall back to auth profile token");
            assert_eq!(key, "tok-from-profile");
        });

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_api_key_prefers_env_over_auth_profile() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("config file should exist");
        fs::write(dir.join(".env"), "OPENAI_API_KEY=sk-env\n").expect("dotenv should be written");

        let manager = AuthManager::in_config_dir(&dir).expect("auth manager should construct");
        manager
            .login("default", "openrouter", "tok-from-profile", true)
            .expect("auth login should succeed");

        temp_env::with_var_unset("OPENAI_API_KEY", || {
            let key = resolve_api_key_for_provider(&config_path, "openrouter")
                .expect("env var should take priority over auth profile");
            assert_eq!(key, "sk-env");
        });

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_api_key_returns_empty_for_local_provider_without_key() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("config file should exist");

        temp_env::with_var_unset("OPENAI_API_KEY", || {
            let key = resolve_api_key_for_provider(&config_path, "ollama")
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
            let key = resolve_api_key_for_provider(&config_path, "llamacpp")
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
            let err = resolve_api_key_for_provider(&config_path, "openrouter")
                .expect_err("cloud provider should require key");
            assert!(err.to_string().contains("missing API key"));
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
                let result = resolve_api_key_for_provider(&config_path, provider);
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

    #[test]
    fn resolve_delegate_base_url_resolves_known_provider() {
        let url = super::resolve_delegate_base_url("openrouter");
        assert!(
            url.contains("openrouter.ai"),
            "openrouter should resolve to openrouter.ai URL, got: {url}"
        );
    }

    #[test]
    fn resolve_delegate_base_url_resolves_anthropic() {
        let url = super::resolve_delegate_base_url("anthropic");
        assert!(
            url.contains("anthropic.com"),
            "anthropic should resolve to anthropic.com URL, got: {url}"
        );
    }

    #[test]
    fn resolve_delegate_base_url_passes_through_urls() {
        let custom_url = "https://my-proxy.example.com/v1";
        let url = super::resolve_delegate_base_url(custom_url);
        assert_eq!(url, custom_url);
    }

    #[test]
    fn resolve_delegate_base_url_passes_through_unknown_kind() {
        let url = super::resolve_delegate_base_url("unknown-provider-xyz");
        assert_eq!(url, "unknown-provider-xyz");
    }

    // --- Streaming runtime tests ---

    use agentzero_core::{
        AgentConfig, ChatResult, MemoryEntry, MemoryStore, Provider, ReasoningConfig, StreamChunk,
        ToolDefinition,
    };
    use async_trait::async_trait;

    #[derive(Default)]
    struct FakeMemory;

    #[async_trait]
    impl MemoryStore for FakeMemory {
        async fn append(&self, _entry: MemoryEntry) -> anyhow::Result<()> {
            Ok(())
        }
        async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(vec![])
        }
    }

    struct FakeStreamProvider {
        response: String,
    }

    #[async_trait]
    impl Provider for FakeStreamProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            Ok(ChatResult {
                output_text: self.response.clone(),
                ..Default::default()
            })
        }

        async fn complete_streaming(
            &self,
            _prompt: &str,
            sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
        ) -> anyhow::Result<ChatResult> {
            for ch in self.response.chars() {
                let _ = sender.send(StreamChunk {
                    delta: ch.to_string(),
                    done: false,
                    tool_call_delta: None,
                });
            }
            let _ = sender.send(StreamChunk {
                delta: String::new(),
                done: true,
                tool_call_delta: None,
            });
            Ok(ChatResult {
                output_text: self.response.clone(),
                ..Default::default()
            })
        }

        async fn complete_streaming_with_tools(
            &self,
            _messages: &[agentzero_core::ConversationMessage],
            _tools: &[ToolDefinition],
            _reasoning: &ReasoningConfig,
            sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
        ) -> anyhow::Result<ChatResult> {
            for ch in self.response.chars() {
                let _ = sender.send(StreamChunk {
                    delta: ch.to_string(),
                    done: false,
                    tool_call_delta: None,
                });
            }
            let _ = sender.send(StreamChunk {
                delta: String::new(),
                done: true,
                tool_call_delta: None,
            });
            Ok(ChatResult {
                output_text: self.response.clone(),
                ..Default::default()
            })
        }
    }

    fn fake_streaming_execution(response: &str) -> super::RuntimeExecution {
        super::RuntimeExecution {
            config: AgentConfig {
                model_supports_tool_use: false,
                ..Default::default()
            },
            provider: Box::new(FakeStreamProvider {
                response: response.to_string(),
            }),
            memory: Box::new(FakeMemory),
            tools: vec![],
            audit_sink: None,
            hook_sink: None,
        }
    }

    #[tokio::test]
    async fn streaming_receiver_delivers_chunks() {
        let execution = fake_streaming_execution("Hello");
        let (mut rx, handle) =
            super::run_agent_streaming(execution, PathBuf::from("/tmp"), "hi".to_string());

        let mut chunks = Vec::new();
        while let Some(chunk) = rx.recv().await {
            chunks.push(chunk);
        }
        handle
            .await
            .expect("task should not panic")
            .expect("should succeed");

        assert!(chunks.len() >= 2, "should have content + done chunks");
        assert!(chunks.last().unwrap().done);
    }

    #[tokio::test]
    async fn streaming_handle_resolves_to_output() {
        let execution = fake_streaming_execution("World");
        let (mut rx, handle) =
            super::run_agent_streaming(execution, PathBuf::from("/tmp"), "hi".to_string());

        // Drain the receiver.
        while rx.recv().await.is_some() {}

        let output = handle
            .await
            .expect("task should not panic")
            .expect("should succeed");
        assert_eq!(output.response_text, "World");
    }

    #[tokio::test]
    async fn streaming_output_matches_accumulated_chunks() {
        let execution = fake_streaming_execution("abc");
        let (mut rx, handle) =
            super::run_agent_streaming(execution, PathBuf::from("/tmp"), "hi".to_string());

        let mut accumulated = String::new();
        while let Some(chunk) = rx.recv().await {
            if !chunk.done {
                accumulated.push_str(&chunk.delta);
            }
        }
        let output = handle
            .await
            .expect("task should not panic")
            .expect("should succeed");
        assert_eq!(accumulated, output.response_text);
    }

    #[test]
    fn build_delegate_agents_resolves_provider_url() {
        let mut agents = std::collections::HashMap::new();
        agents.insert(
            "researcher".to_string(),
            agentzero_config::DelegateAgentConfig {
                provider: "openrouter".to_string(),
                model: "gpt-4o".to_string(),
                ..Default::default()
            },
        );

        let config = agentzero_config::AgentZeroConfig {
            agents,
            ..Default::default()
        };

        let result = super::build_delegate_agents(&config).expect("should build delegate agents");
        let researcher = result.get("researcher").expect("researcher should exist");
        assert_eq!(researcher.provider_kind, "openrouter");
        assert!(
            researcher.provider.contains("openrouter.ai"),
            "provider URL should be resolved, got: {}",
            researcher.provider
        );
    }
}
