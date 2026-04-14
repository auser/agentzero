use crate::audio::process_audio_markers;
use crate::audit::{FileAuditSink, SequencedAuditSink};
use crate::tools::default_tools_with_store;
use agentzero_auth::AuthManager;
use agentzero_config::{
    load, load_audit_policy, load_env_var, load_tool_security_policy, AudioConfig,
};
use agentzero_core::common::local_providers::{is_local_provider, local_provider_meta};
use agentzero_core::delegation::DelegateConfig;
use agentzero_core::routing::{
    ClassificationRule, EmbeddingRoute, ModelRoute, ModelRouter, PrivacyLevel,
};
use agentzero_core::{
    Agent, AgentConfig, AuditEvent, AuditSink, HookEvent, HookFailureMode, HookSink, MemoryStore,
    Provider, RuntimeMetrics, Tool, ToolContext, ToolSource, UserMessage,
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
use tracing::{info, warn};

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
    /// Active conversation ID for memory scoping.
    pub conversation_id: Option<String>,
    /// Optional agent store for the `agent_manage` tool.
    /// When provided and `enable_agent_manage` is true in config, the tool
    /// is registered so agents can create/manage other persistent agents.
    pub agent_store: Option<std::sync::Arc<dyn agentzero_core::agent_store::AgentStoreApi>>,
    /// Optional memory override. When set, skips building the default
    /// SQLite/Turso memory store and uses this instead. Useful for ephemeral
    /// workflow agents that don't need persistent conversation memory.
    pub memory_override: Option<Box<dyn MemoryStore>>,
    /// Override memory_window_size from config. When `Some(0)`, no prior
    /// conversation history is loaded (useful for stateless one-shot commands).
    pub memory_window_override: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RunAgentOutput {
    pub response_text: String,
    pub metrics_snapshot: serde_json::Value,
    pub tool_executions: Vec<agentzero_core::ToolExecutionRecord>,
}

pub struct RuntimeExecution {
    pub config: AgentConfig,
    pub provider: Box<dyn Provider>,
    pub memory: Box<dyn MemoryStore>,
    pub tools: Vec<Box<dyn Tool>>,
    pub audit_sink: Option<Box<dyn AuditSink>>,
    pub hook_sink: Option<Box<dyn HookSink>>,
    pub conversation_id: Option<String>,
    /// Audio transcription config; `None` → audio markers are stripped.
    pub audio_config: Option<AudioConfig>,
    /// Per-run token budget (0 = unlimited).
    pub max_tokens: u64,
    /// Per-run cost budget in micro-dollars (0 = unlimited).
    pub max_cost_microdollars: u64,
    /// Cost tracking configuration from `[cost]` TOML section.
    pub cost_config: agentzero_config::CostConfig,
    /// Data directory for persistent cost tracking.
    pub data_dir: PathBuf,
    /// Optional tool selector for AI/keyword-based tool filtering.
    pub tool_selector: Option<Box<dyn agentzero_core::ToolSelector>>,
    /// Source channel name (set when invoked from a channel, e.g. "telegram").
    pub source_channel: Option<String>,
    /// Sender identity for per-sender rate limiting.
    pub sender_id: Option<String>,
    /// Optional dynamic tool registry for mid-session tool creation.
    pub dynamic_registry: Option<std::sync::Arc<crate::tools::dynamic_tool::DynamicToolRegistry>>,
    /// Optional task manager for background delegation. When present, `cancel_all()`
    /// is called on session teardown to cascade-cancel orphaned background tasks.
    pub task_manager: Option<std::sync::Arc<agentzero_tools::TaskManager>>,
    /// Optional tool evolver for auto-fixing/improving dynamic tools.
    pub tool_evolver: Option<std::sync::Arc<crate::tool_evolver::ToolEvolver>>,
    /// Optional recipe store for recording tool usage patterns.
    pub recipe_store: Option<std::sync::Arc<std::sync::Mutex<crate::tool_recipes::RecipeStore>>>,
    /// Optional pattern capture for AUTO-LEARN (novel tool combo detection).
    pub pattern_capture: Option<std::sync::Arc<crate::pattern_capture::PatternCapture>>,
    /// Optional trajectory recorder for session-level learning.
    pub trajectory_recorder: Option<std::sync::Arc<crate::trajectory::TrajectoryRecorder>>,
    /// Model name used for this run (for trajectory tagging).
    pub model_name: String,
    /// Optional local embedding provider for cosine-similarity re-ranking.
    pub embedding_provider:
        Option<std::sync::Arc<dyn agentzero_core::embedding::EmbeddingProvider>>,
}

struct AuditHookSink {
    sink: FileAuditSink,
}

#[async_trait]
impl HookSink for AuditHookSink {
    async fn record(&self, event: HookEvent) -> anyhow::Result<()> {
        self.sink
            .record(AuditEvent {
                seq: 0,
                session_id: String::new(),
                stage: format!("hook.{}", event.stage),
                detail: json!({ "hook": event.detail }).into(),
            })
            .await
    }
}

/// Adapter to use `Arc<dyn Provider>` where `Box<dyn Provider>` is expected.
/// Used when wrapping providers through the composable pipeline.
struct PipelineProviderAdapter(std::sync::Arc<dyn Provider>);

#[async_trait]
impl Provider for PipelineProviderAdapter {
    fn supports_streaming(&self) -> bool {
        self.0.supports_streaming()
    }
    async fn complete(&self, prompt: &str) -> anyhow::Result<agentzero_core::ChatResult> {
        self.0.complete(prompt).await
    }
    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        reasoning: &agentzero_core::ReasoningConfig,
    ) -> anyhow::Result<agentzero_core::ChatResult> {
        self.0.complete_with_reasoning(prompt, reasoning).await
    }
    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<agentzero_core::StreamChunk>,
    ) -> anyhow::Result<agentzero_core::ChatResult> {
        self.0.complete_streaming(prompt, sender).await
    }
    async fn complete_with_tools(
        &self,
        messages: &[agentzero_core::ConversationMessage],
        tools: &[agentzero_core::ToolDefinition],
        reasoning: &agentzero_core::ReasoningConfig,
    ) -> anyhow::Result<agentzero_core::ChatResult> {
        self.0.complete_with_tools(messages, tools, reasoning).await
    }
    async fn complete_streaming_with_tools(
        &self,
        messages: &[agentzero_core::ConversationMessage],
        tools: &[agentzero_core::ToolDefinition],
        reasoning: &agentzero_core::ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<agentzero_core::StreamChunk>,
    ) -> anyhow::Result<agentzero_core::ChatResult> {
        self.0
            .complete_streaming_with_tools(messages, tools, reasoning, sender)
            .await
    }
}

/// Build a `RuntimeExecution` from a `RunAgentRequest`. This resolves config,
/// API keys, provider, memory, tools, audit, and hooks — everything needed to
/// run the agent. Extracted so both `run_agent_once` and streaming callers can
/// share the setup logic.
pub async fn build_runtime_execution(req: RunAgentRequest) -> anyhow::Result<RuntimeExecution> {
    let mut config = load(&req.config_path)?;

    // Initialize the codegen kill-switch from the freshly-loaded config. The
    // AGENTZERO_CODEGEN_ENABLED env var still wins as an emergency operational
    // override — set_codegen_enabled only stores the value, and any subsequent
    // is_codegen_enabled() call that runs before this function is called will
    // have already consulted the env var on the first-read init path.
    crate::tools::tool_create::set_codegen_enabled(config.runtime.codegen_enabled);

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

    // If the config specifies a non-default provider kind but the base_url is
    // still the default (openrouter), resolve the correct URL from the catalog.
    // This handles configs like `kind = "anthropic"` without an explicit base_url.
    resolve_base_url_from_catalog(&mut config);

    let key = resolve_api_key(
        &req.config_path,
        &mut config,
        req.profile_override.as_deref(),
        req.provider_override.is_some(),
    )
    .await?;

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

    // Build cost calculator closure if pricing is available for this model.
    let cost_calculator = agentzero_providers::model_pricing(
        &config.provider.kind,
        &config.provider.model,
    )
    .map(|pricing| {
        std::sync::Arc::new(move |input_tokens: u64, output_tokens: u64| -> u64 {
            agentzero_providers::compute_cost_microdollars(&pricing, input_tokens, output_tokens)
        }) as std::sync::Arc<dyn Fn(u64, u64) -> u64 + Send + Sync>
    });

    // Clone the key before it's moved into the primary provider.
    let key_for_dynamic_tools = key.clone();

    let primary_provider: Box<dyn agentzero_core::Provider> = if config.provider.kind == "candle" {
        build_candle_from_config(&config)?
    } else if config.privacy.block_cloud_providers || config.privacy.mode == "local_only" {
        agentzero_providers::build_provider_with_privacy(
            &config.provider.kind,
            config.provider.base_url.clone(),
            key,
            config.provider.model.clone(),
            transport_config.clone(),
            &config.privacy.mode,
        )?
    } else {
        agentzero_providers::build_provider_with_transport(
            &config.provider.kind,
            config.provider.base_url.clone(),
            key,
            config.provider.model.clone(),
            transport_config.clone(),
        )
    };

    // Wrap with fallback chain if configured.
    let provider: Box<dyn agentzero_core::Provider> =
        if config.provider.fallback_providers.is_empty() {
            primary_provider
        } else {
            let mut chain: Vec<(String, Box<dyn agentzero_core::Provider>)> =
                Vec::with_capacity(1 + config.provider.fallback_providers.len());
            chain.push((config.provider.kind.clone(), primary_provider));

            for entry in &config.provider.fallback_providers {
                let fb_key = entry
                    .api_key_env
                    .as_ref()
                    .and_then(|env_var| std::env::var(env_var).ok())
                    .unwrap_or_default();
                let fb_provider = agentzero_providers::build_provider_with_transport(
                    &entry.kind,
                    entry.base_url.clone(),
                    fb_key,
                    entry.model.clone(),
                    transport_config.clone(),
                );
                let label = format!("{}:{}", entry.kind, entry.model);
                chain.push((label, fb_provider));
            }

            // Filter out providers whose model is incompatible with their
            // provider kind (e.g. an OpenAI model routed to Anthropic).
            let chain: Vec<_> = chain
                .into_iter()
                .filter(|(label, _)| {
                    if let Some((kind, model)) = label.split_once(':') {
                        let supported = agentzero_providers::provider_supports_model(kind, model);
                        if !supported {
                            tracing::warn!(
                                provider = kind,
                                model = model,
                                "filtering incompatible model from fallback chain"
                            );
                        }
                        supported
                    } else {
                        true // Primary provider or unknown format — allow
                    }
                })
                .collect();

            tracing::info!(
                chain_len = chain.len(),
                "provider fallback chain configured"
            );
            Box::new(agentzero_providers::FallbackProvider::new(chain))
        };

    // Wrap provider with composable pipeline layers (metrics, cost cap).
    let provider: Box<dyn agentzero_core::Provider> = {
        let provider_arc: std::sync::Arc<dyn agentzero_core::Provider> = provider.into();
        let mut pipeline = agentzero_providers::PipelineBuilder::new().layer(
            agentzero_providers::MetricsLayer::new(&config.provider.kind, &config.provider.model),
        );

        // Add cost cap layer if a per-run budget is configured.
        let cost_budget = config
            .agent
            .max_cost_usd
            .map(|usd| (usd * 1_000_000.0) as u64)
            .unwrap_or(0);
        if cost_budget > 0 {
            pipeline = pipeline.layer(agentzero_providers::CostCapLayer::new(
                cost_budget,
                &config.provider.kind,
                &config.provider.model,
            ));
        }

        // Add guardrails layer (default: audit mode for both PII and injection).
        let guard_entries = build_guard_entries(&config.guardrails);
        if !guard_entries.is_empty() {
            pipeline = pipeline.layer(agentzero_providers::GuardrailsLayer::new(guard_entries));
        }

        // PrivacyFirstLayer is the outermost layer — it runs PII redaction
        // on every prompt before any other layer or the provider sees it.
        // It cannot be disabled. This is a core project safety guarantee:
        // no PII reaches a remote LLM provider, ever.
        if !agentzero_core::common::local_providers::is_local_provider(&config.provider.kind) {
            pipeline = pipeline.layer(agentzero_providers::privacy_layer::PrivacyFirstLayer);
        }

        let wrapped = pipeline.build(provider_arc);
        // Convert Arc<dyn Provider> back to Box<dyn Provider> for RuntimeExecution.
        Box::new(PipelineProviderAdapter(wrapped))
    };

    let memory = match req.memory_override {
        Some(m) => m,
        None => build_memory_store(&req.config_path).await?,
    };
    let tool_policy = load_tool_security_policy(&req.workspace_root, &req.config_path)?;
    let mut tools: Vec<Box<dyn Tool>> =
        default_tools_with_store(&tool_policy, router, delegate_agents, req.agent_store)?;
    // Append any extra tools (e.g. FFI-registered tools).
    tools.extend(req.extra_tools);

    // Pre-build a shared audit sink for codegen lifecycle events. The same
    // Arc is later moved into the RuntimeExecution output (as a Box) so only
    // one sink instance exists per agent session.
    let audit_policy = load_audit_policy(&req.workspace_root, &req.config_path)?;
    let audit_path = audit_policy.path.clone();
    let shared_audit_sink: Option<std::sync::Arc<SequencedAuditSink>> = if audit_policy.enabled {
        let session_id = format!(
            "ses-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            std::process::id()
        );
        Some(std::sync::Arc::new(SequencedAuditSink::new(
            Box::new(FileAuditSink::new(audit_path.clone())),
            session_id,
        )))
    } else {
        None
    };

    // Load persisted dynamic tools and register the tool_create tool.
    let dynamic_registry = if tool_policy.enable_dynamic_tools {
        let data_dir = req.workspace_root.join(".agentzero");
        match crate::tools::dynamic_tool::DynamicToolRegistry::open(&data_dir) {
            Ok(registry) => {
                let registry = std::sync::Arc::new(registry);
                // Load all previously created dynamic tools.
                let dynamic_tools = registry.additional_tools();
                let count = dynamic_tools.len();
                tools.extend(dynamic_tools);
                if count > 0 {
                    tracing::info!(count, "loaded persisted dynamic tools");
                }
                // Register the tool_create tool so agents can create new ones.
                // Reuse the already-constructed primary provider via a clone-friendly wrapper.
                let provider_for_create: std::sync::Arc<dyn agentzero_core::Provider> =
                    std::sync::Arc::from(agentzero_providers::build_provider_with_transport(
                        &config.provider.kind,
                        config.provider.base_url.clone(),
                        key_for_dynamic_tools.clone(),
                        config.provider.model.clone(),
                        transport_config.clone(),
                    ));
                // Wire audit sink into tool_create so codegen lifecycle events
                // (blocked, compile_start, compile_success, compile_failed) are
                // recorded alongside the agent's regular audit trail.
                let tool_create = if let Some(ref sink) = shared_audit_sink {
                    crate::tools::tool_create::ToolCreateTool::new_with_audit(
                        std::sync::Arc::clone(&registry),
                        provider_for_create,
                        std::sync::Arc::clone(sink) as std::sync::Arc<dyn AuditSink>,
                    )
                } else {
                    crate::tools::tool_create::ToolCreateTool::new(
                        std::sync::Arc::clone(&registry),
                        provider_for_create,
                    )
                };
                tools.push(Box::new(tool_create));
                Some(registry)
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to open dynamic tool registry");
                None
            }
        }
    } else {
        None
    };

    // Build tool evolver for auto-fix/improve of dynamic tools.
    let tool_evolver = if let Some(ref registry) = dynamic_registry {
        let provider_for_evolver: std::sync::Arc<dyn agentzero_core::Provider> =
            std::sync::Arc::from(agentzero_providers::build_provider_with_transport(
                &config.provider.kind,
                config.provider.base_url.clone(),
                key_for_dynamic_tools.clone(),
                config.provider.model.clone(),
                transport_config.clone(),
            ));
        Some(std::sync::Arc::new(crate::tool_evolver::ToolEvolver::new(
            provider_for_evolver,
            std::sync::Arc::clone(registry),
        )))
    } else {
        None
    };

    // Build recipe store for tool catalog learning.
    let recipe_store = {
        let data_dir = req.workspace_root.join(".agentzero");
        match crate::tool_recipes::RecipeStore::open(&data_dir) {
            Ok(store) => Some(std::sync::Arc::new(std::sync::Mutex::new(store))),
            Err(e) => {
                tracing::warn!(error = %e, "failed to open recipe store");
                None
            }
        }
    };

    Ok(RuntimeExecution {
        config: AgentConfig {
            max_tool_iterations: config.agent.max_tool_iterations,
            request_timeout_ms: config.agent.request_timeout_ms,
            memory_window_size: req
                .memory_window_override
                .unwrap_or(config.agent.memory_window_size),
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
                adaptive: config.runtime.adaptive_reasoning.unwrap_or(false),
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
            model_supports_tool_use: caps.map_or(true, |c| c.tool_use),
            model_supports_vision: caps.is_some_and(|c| c.vision),
            system_prompt: config.agent.system_prompt.clone().or_else(|| {
                if is_local_provider(&config.provider.kind) {
                    Some(
                        "You are a helpful assistant running inside a workspace directory. \
                         You have full access to the project files through your tools. \
                         ALWAYS use your tools to answer questions — never say you cannot \
                         access files or ask the user to provide information you can look up. \
                         For any task involving the project, start by using glob_search to \
                         discover files, content_search to find patterns, and read_file to \
                         read contents. Act autonomously."
                            .to_string(),
                    )
                } else {
                    None
                }
            }),
            privacy_boundary: config.privacy.mode.clone(),
            tool_boundaries: config.security.tool_boundaries.clone(),
            cost_calculator,
            tool_timeout_ms: config.agent.tool_timeout_ms,
            tool_selection: {
                let explicit = config.agent.tool_selection.clone().unwrap_or_default();
                if explicit.is_empty() && is_local_provider(&config.provider.kind) {
                    // Local models struggle with large tool sets — auto-enable
                    // keyword-based tool selection to keep the prompt manageable.
                    agentzero_core::ToolSelectionMode::Keyword
                } else {
                    explicit
                        .parse()
                        .unwrap_or(agentzero_core::ToolSelectionMode::All)
                }
            },
            tool_selection_model: config.agent.tool_selection_model.clone(),
            summarization: agentzero_core::SummarizationConfig {
                enabled: config.agent.summarization.enabled,
                keep_recent: config.agent.summarization.keep_recent,
                min_entries_for_summarization: config
                    .agent
                    .summarization
                    .min_entries_for_summarization,
                max_summary_chars: config.agent.summarization.max_summary_chars,
                compression_enabled: config.agent.summarization.compression_enabled,
                max_tool_result_chars: config.agent.summarization.max_tool_result_chars,
                protect_head: config.agent.summarization.protect_head,
                protect_tail: config.agent.summarization.protect_tail,
            },
            skill_prompt_fragments: {
                let skills_dir = config
                    .skills
                    .bundles_dir
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| req.workspace_root.join(".agentzero").join("skills"));
                load_skill_prompt_fragments(&skills_dir, &config.skills.auto_activate)
            },
        },
        provider,
        memory,
        tools,
        // Reuse the shared audit sink (already created earlier for codegen
        // events). SequencedAuditSink implements AuditSink, so the Arc wraps
        // cleanly into a Box<dyn AuditSink>.
        audit_sink: shared_audit_sink.map(|arc| Box::new(arc) as Box<dyn AuditSink>),
        hook_sink: if config.agent.hooks.enabled {
            Some(Box::new(AuditHookSink {
                sink: FileAuditSink::new(audit_path),
            }) as Box<dyn HookSink>)
        } else {
            None
        },
        conversation_id: req.conversation_id,
        audio_config: if config.audio.api_key.is_some() {
            Some(config.audio.clone())
        } else {
            None
        },
        max_tokens: config.agent.max_tokens.unwrap_or(0),
        max_cost_microdollars: config
            .agent
            .max_cost_usd
            .map(|usd| (usd * 1_000_000.0) as u64)
            .unwrap_or(0),
        cost_config: config.cost.clone(),
        data_dir: req
            .config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
        tool_selector: {
            let mode = config.agent.tool_selection.as_deref().unwrap_or("");
            if mode == "keyword" || (mode.is_empty() && is_local_provider(&config.provider.kind)) {
                Some(Box::new(
                    crate::tool_selection::KeywordToolSelector::default(),
                ))
            } else {
                // "ai" selector requires a provider — wired at a higher level.
                None
            }
        },
        source_channel: None,
        sender_id: None,
        dynamic_registry: dynamic_registry.clone(),
        task_manager: None,
        tool_evolver,
        recipe_store: recipe_store.clone(),
        pattern_capture: match (&dynamic_registry, &recipe_store) {
            (Some(reg), Some(store)) => Some(std::sync::Arc::new(
                crate::pattern_capture::PatternCapture::new(
                    std::sync::Arc::clone(reg),
                    std::sync::Arc::clone(store),
                ),
            )),
            _ => None,
        },
        embedding_provider: build_embedding_provider(),
        trajectory_recorder: {
            let data_dir = req.config_path.parent().unwrap_or_else(|| Path::new("."));
            match crate::trajectory::TrajectoryRecorder::new(data_dir) {
                Ok(rec) => Some(std::sync::Arc::new(rec)),
                Err(e) => {
                    warn!(error = %e, "failed to create trajectory recorder");
                    None
                }
            }
        },
        model_name: config.provider.model.clone(),
    })
}

/// Build a local embedding provider when the `candle` feature is active.
fn build_embedding_provider(
) -> Option<std::sync::Arc<dyn agentzero_core::embedding::EmbeddingProvider>> {
    #[cfg(feature = "candle")]
    {
        Some(std::sync::Arc::new(
            agentzero_providers::candle_embedding::CandleEmbeddingProvider::new(),
        ))
    }
    #[cfg(not(feature = "candle"))]
    {
        None
    }
}

/// Convert a guardrails config mode string to [`GuardEntry`] entries.
fn build_guard_entries(
    gc: &agentzero_config::GuardrailsConfig,
) -> Vec<agentzero_providers::GuardEntry> {
    fn to_enforcement(mode: &str) -> Option<agentzero_providers::Enforcement> {
        match mode {
            "block" => Some(agentzero_providers::Enforcement::Block),
            "sanitize" => Some(agentzero_providers::Enforcement::Sanitize),
            "audit" => Some(agentzero_providers::Enforcement::Audit),
            _ => None, // "off" or unrecognised
        }
    }

    let mut entries = Vec::new();
    if let Some(e) = to_enforcement(&gc.pii_mode) {
        entries.push(agentzero_providers::GuardEntry::new(
            agentzero_providers::PiiRedactionGuard,
            e,
        ));
    }
    if let Some(e) = to_enforcement(&gc.injection_mode) {
        entries.push(agentzero_providers::GuardEntry::new(
            agentzero_providers::PromptInjectionGuard::default(),
            e,
        ));
    }
    entries
}

/// Load prompt fragments from active skill bundles.
///
/// Scans the skills directory for bundles whose trigger is `Always` or whose
/// name appears in `auto_activate`. Returns prompt fragments sorted by
/// priority (lower = earlier).
fn load_skill_prompt_fragments(skills_dir: &Path, auto_activate: &[String]) -> Vec<String> {
    use agentzero_core::SkillTrigger;

    let dirs = if !skills_dir.is_dir() {
        return Vec::new();
    } else {
        // Use a blocking read since we're in a sync context within the config builder.
        let entries = match std::fs::read_dir(skills_dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.path().is_dir() && e.path().join("skill.toml").exists())
            .map(|e| e.path())
            .collect();
        dirs.sort();
        dirs
    };

    let mut fragments: Vec<(i32, String)> = Vec::new();
    for dir in &dirs {
        let name = match dir.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        let manifest_path = dir.join("skill.toml");
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                warn!(skill = %name, "failed to read skill.toml: {e}");
                continue;
            }
        };
        let bundle: agentzero_core::SkillBundle = match toml::from_str(&content) {
            Ok(b) => b,
            Err(e) => {
                warn!(skill = %name, "failed to parse skill.toml: {e}");
                continue;
            }
        };

        let should_activate = match &bundle.trigger {
            SkillTrigger::Always => true,
            SkillTrigger::Manual => auto_activate.contains(&name.to_string()),
            SkillTrigger::Keyword { .. } => auto_activate.contains(&name.to_string()),
        };

        if !should_activate {
            continue;
        }

        // Read prompt.md if it exists.
        let prompt_path = dir.join("prompt.md");
        let prompt = match std::fs::read_to_string(&prompt_path) {
            Ok(p) if !p.trim().is_empty() => p,
            _ => bundle.prompt_template.clone(),
        };

        if !prompt.is_empty() {
            info!(skill = %name, priority = bundle.priority, "loading skill prompt fragment");
            fragments.push((bundle.priority, prompt));
        }
    }

    // Sort by priority (lower = earlier).
    fragments.sort_by_key(|(priority, _)| *priority);
    fragments.into_iter().map(|(_, prompt)| prompt).collect()
}

/// Build just a [`Provider`] from config, without tools/memory/audit.
///
/// Useful for lightweight LLM callers like the [`GoalPlanner`] that need a
/// provider but don't need the full agent runtime.
pub async fn build_provider_from_config(
    config_path: &std::path::Path,
    provider_override: Option<&str>,
    model_override: Option<&str>,
    profile_override: Option<&str>,
) -> anyhow::Result<Box<dyn Provider>> {
    let mut config = load(config_path)?;

    if let Some(kind) = provider_override {
        config.provider.kind = kind.to_string();
        if let Some(meta) = local_provider_meta(kind) {
            config.provider.base_url = meta.default_base_url.to_string();
        } else if let Some(descriptor) = find_provider(kind) {
            if let Some(url) = descriptor.default_base_url {
                config.provider.base_url = url.to_string();
            }
        }
    }
    if let Some(model) = model_override {
        config.provider.model = model.to_string();
    }

    resolve_base_url_from_catalog(&mut config);

    let key = resolve_api_key(
        config_path,
        &mut config,
        profile_override,
        provider_override.is_some(),
    )
    .await?;

    let transport_config = agentzero_providers::TransportConfig {
        timeout_ms: config.provider.transport.timeout_ms,
        max_retries: config.provider.transport.max_retries,
        circuit_breaker_threshold: config.provider.transport.circuit_breaker_threshold,
        circuit_breaker_reset_ms: config.provider.transport.circuit_breaker_reset_ms,
    };

    let provider: Box<dyn Provider> =
        if config.privacy.block_cloud_providers || config.privacy.mode == "local_only" {
            agentzero_providers::build_provider_with_privacy(
                &config.provider.kind,
                config.provider.base_url,
                key,
                config.provider.model,
                transport_config,
                &config.privacy.mode,
            )?
        } else {
            agentzero_providers::build_provider_with_transport(
                &config.provider.kind,
                config.provider.base_url,
                key,
                config.provider.model,
                transport_config,
            )
        };

    Ok(provider)
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
        // Pre-run cost limit check.
        if execution.cost_config.enabled {
            if let Ok(tracker) = crate::cost_tracker::CostTracker::load(&execution.data_dir) {
                if let Some(reason) = tracker.check_limits(&execution.cost_config) {
                    anyhow::bail!("{reason}");
                }
                if let Some(warning) = tracker.check_warnings(&execution.cost_config) {
                    warn!("{warning}");
                }
            }
        }

        let privacy_boundary = execution.config.privacy_boundary.clone();
        let cost_config = execution.cost_config.clone();
        let data_dir = execution.data_dir.clone();
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
        if let Some(selector) = execution.tool_selector {
            agent = agent.with_tool_selector(selector);
        }
        if let Some(registry) = execution.dynamic_registry {
            agent = agent.with_tool_source(registry);
        }

        let mut ctx = ToolContext::new(workspace_root.to_string_lossy().to_string());
        ctx.privacy_boundary = privacy_boundary;
        ctx.conversation_id = execution.conversation_id.clone();
        ctx.max_tokens = execution.max_tokens;
        ctx.max_cost_microdollars = execution.max_cost_microdollars;
        ctx.source_channel = execution.source_channel.clone();
        ctx.sender_id = execution.sender_id.clone();

        let response = agent
            .respond_streaming(UserMessage { text: message }, &ctx, tx)
            .await?;
        let metrics_snapshot = runtime_metrics.export_json();
        info!(metrics = %metrics_snapshot, "streaming runtime metrics snapshot");

        // Post-run: persist cost for daily/monthly tracking.
        let run_cost = ctx.current_cost();
        if cost_config.enabled && run_cost > 0 {
            if let Ok(mut tracker) = crate::cost_tracker::CostTracker::load(&data_dir) {
                if let Err(e) = tracker.record_cost(run_cost) {
                    warn!(error = %e, "failed to persist cost tracking");
                }
            }
        }

        let tool_executions = ctx
            .tool_executions
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default();
        persist_execution_history(&data_dir, &tool_executions);
        Ok(RunAgentOutput {
            response_text: response.text,
            metrics_snapshot,
            tool_executions,
        })
    });
    (rx, handle)
}

pub async fn run_agent_with_runtime(
    execution: RuntimeExecution,
    workspace_root: PathBuf,
    message: String,
) -> anyhow::Result<RunAgentOutput> {
    // Pre-run cost limit check.
    if execution.cost_config.enabled {
        if let Ok(tracker) = crate::cost_tracker::CostTracker::load(&execution.data_dir) {
            if let Some(reason) = tracker.check_limits(&execution.cost_config) {
                anyhow::bail!("{reason}");
            }
            if let Some(warning) = tracker.check_warnings(&execution.cost_config) {
                warn!("{warning}");
            }
        }
    }

    let privacy_boundary = execution.config.privacy_boundary.clone();
    let cost_config = execution.cost_config.clone();
    let data_dir = execution.data_dir.clone();
    let task_manager = execution.task_manager.clone();
    let dynamic_registry = execution.dynamic_registry.clone();
    let tool_evolver = execution.tool_evolver.clone();
    let recipe_store = execution.recipe_store.clone();
    let pattern_capture = execution.pattern_capture.clone();
    let trajectory_recorder = execution.trajectory_recorder.clone();
    let model_name = execution.model_name.clone();
    let run_started = std::time::Instant::now();
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
    if let Some(selector) = execution.tool_selector {
        agent = agent.with_tool_selector(selector);
    }
    if let Some(ref registry) = dynamic_registry {
        agent = agent.with_tool_source(registry.clone());
    }

    let mut ctx = ToolContext::new(workspace_root.to_string_lossy().to_string());
    ctx.privacy_boundary = privacy_boundary;
    ctx.max_tokens = execution.max_tokens;
    ctx.max_cost_microdollars = execution.max_cost_microdollars;
    ctx.source_channel = execution.source_channel.clone();
    ctx.sender_id = execution.sender_id.clone();

    // Transcribe [AUDIO:path] markers before the message reaches the LLM.
    let goal_summary = message.clone();
    let message = process_audio_markers(&message, execution.audio_config.as_ref()).await?;

    let response = agent.respond(UserMessage { text: message }, &ctx).await?;
    let metrics_snapshot = runtime_metrics.export_json();
    info!(metrics = %metrics_snapshot, "runtime metrics snapshot");

    // Post-run: persist cost for daily/monthly tracking.
    let run_cost = ctx.current_cost();
    if cost_config.enabled && run_cost > 0 {
        if let Ok(mut tracker) = crate::cost_tracker::CostTracker::load(&data_dir) {
            if let Err(e) = tracker.record_cost(run_cost) {
                warn!(error = %e, "failed to persist cost tracking");
            }
        }
    }

    // Session teardown: cancel all background delegation tasks to prevent orphans.
    if let Some(ref tm) = task_manager {
        tm.cancel_all().await;
    }

    // Extract tool execution records from the shared context.
    let tool_executions = ctx
        .tool_executions
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();

    // Persist execution records for quality tracking.
    persist_execution_history(&data_dir, &tool_executions);

    // Update quality counters on dynamic tools.
    if let Some(ref registry) = dynamic_registry {
        for record in &tool_executions {
            if registry.is_dynamic(&record.tool_name).await {
                if let Err(e) = registry
                    .record_outcome(&record.tool_name, record.success, record.error.as_deref())
                    .await
                {
                    warn!(error = %e, tool = %record.tool_name, "failed to update tool quality counters");
                }
            }
        }
    }

    // Auto-fix failing / auto-improve successful dynamic tools.
    if let Some(ref evolver) = tool_evolver {
        let failed: std::collections::HashSet<String> = tool_executions
            .iter()
            .filter(|r| !r.success)
            .map(|r| r.tool_name.clone())
            .collect();
        for tool_name in &failed {
            match evolver.maybe_fix(tool_name).await {
                Ok(true) => info!(tool = %tool_name, "auto-fixed failing dynamic tool"),
                Ok(false) => {}
                Err(e) => warn!(tool = %tool_name, error = %e, "auto-fix check failed"),
            }
        }
        if let Err(e) = evolver.evolve_candidates().await {
            warn!(error = %e, "auto-improve pass failed");
        }
    }

    // Record tool usage as a recipe for catalog learning.
    if let Some(ref store) = recipe_store {
        let tools_used: Vec<String> = tool_executions
            .iter()
            .filter(|r| r.success)
            .map(|r| r.tool_name.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let any_failures = tool_executions.iter().any(|r| !r.success);
        let success = !tools_used.is_empty() && !any_failures;
        if let Ok(mut store) = store.lock() {
            if let Err(e) = store.record(&goal_summary, &tools_used, success) {
                warn!(error = %e, "failed to record tool recipe");
            }
        }
    }

    // AUTO-LEARN: capture novel multi-tool patterns as composite tools.
    if let Some(ref capture) = pattern_capture {
        if let Err(e) = capture
            .capture_if_novel(&goal_summary, &tool_executions)
            .await
        {
            warn!(error = %e, "pattern capture failed");
        }
    }

    // Record session trajectory for self-improving learning.
    if let Some(ref recorder) = trajectory_recorder {
        let any_failures = tool_executions.iter().any(|r| !r.success);
        let has_output = !response.text.is_empty();
        let outcome = if !any_failures && has_output {
            crate::trajectory::Outcome::Success
        } else if has_output {
            crate::trajectory::Outcome::Partial {
                reason: "some tool executions failed".to_string(),
            }
        } else {
            crate::trajectory::Outcome::Failure {
                reason: "empty response".to_string(),
            }
        };
        let session_id = format!("ses-{}", std::process::id());
        let run_id = format!(
            "run-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let record = crate::trajectory::build_record(crate::trajectory::TrajectoryInput {
            session_id: &session_id,
            run_id: &run_id,
            outcome,
            goal_summary: &goal_summary,
            response_text: &response.text,
            tool_executions: &tool_executions,
            input_tokens: ctx.current_tokens(),
            output_tokens: 0, // output tokens tracked at provider level, not separately in ctx
            cost_microdollars: ctx.current_cost(),
            model: &model_name,
            latency_ms: run_started.elapsed().as_millis() as u64,
        });
        if let Err(e) = recorder.record(record).await {
            warn!(error = %e, "failed to record trajectory");
        }
    }

    // Periodic recipe evolution: promote winners, retire losers.
    if let Some(ref store) = recipe_store {
        if let Ok(mut store) = store.lock() {
            store.increment_run_counter();
            if store.should_evolve() {
                match store.evolve_recipes() {
                    Ok(changes) if changes > 0 => {
                        info!(changes, "recipe evolution applied");
                    }
                    Ok(_) => {}
                    Err(e) => warn!(error = %e, "recipe evolution failed"),
                }
            }
        }
    }

    Ok(RunAgentOutput {
        response_text: response.text,
        metrics_snapshot,
        tool_executions,
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
async fn resolve_api_key(
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
    // Check provider-specific env vars first, then the generic OPENAI_API_KEY.
    let provider_env_var = match config.provider.kind.as_str() {
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        _ => None,
    };
    if let Some(env_var) = provider_env_var {
        if let Some(key) = load_env_var(config_path, env_var)? {
            return Ok(key);
        }
    }
    if let Some(key) = load_env_var(config_path, "OPENAI_API_KEY")? {
        return Ok(key);
    }

    // Auth profiles: provider match then any active profile.
    // Before resolving, attempt auto-refresh if the token is expired.
    // Try the config file's parent directory first, then fall back to the
    // default data directory (where `auth login` stores credentials).
    let mut auth_dirs: Vec<PathBuf> = Vec::new();
    if let Some(config_dir) = config_path.parent() {
        auth_dirs.push(config_dir.to_path_buf());
    }
    if let Some(default_dir) = agentzero_core::common::paths::default_data_dir() {
        if !auth_dirs.iter().any(|d| d == &default_dir) {
            auth_dirs.push(default_dir);
        }
    }
    for auth_dir in &auth_dirs {
        if let Ok(manager) = AuthManager::in_config_dir(auth_dir) {
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

    let env_hint = match config.provider.kind.as_str() {
        "anthropic" => "ANTHROPIC_API_KEY",
        _ => "OPENAI_API_KEY",
    };
    anyhow::bail!(
        "missing API key for provider '{}': \
         set {env_hint} (env var or .env) or use a provider that supports your auth method",
        config.provider.kind
    )
}

/// Append tool execution records to `<data_dir>/execution-history.jsonl`.
/// Best-effort: failures are logged but do not propagate. Caps file at 10,000 lines.
fn persist_execution_history(
    data_dir: &std::path::Path,
    records: &[agentzero_core::ToolExecutionRecord],
) {
    if records.is_empty() {
        return;
    }
    let path = data_dir.join("execution-history.jsonl");
    let mut lines: Vec<String> = if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => content.lines().map(String::from).collect(),
            Err(e) => {
                warn!(error = %e, "failed to read execution history");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    for record in records {
        match serde_json::to_string(record) {
            Ok(json) => lines.push(json),
            Err(e) => warn!(error = %e, "failed to serialize execution record"),
        }
    }
    const MAX_LINES: usize = 10_000;
    if lines.len() > MAX_LINES {
        lines = lines.split_off(lines.len() - MAX_LINES);
    }
    let content = lines.join("\n") + "\n";
    if let Err(e) = std::fs::write(&path, content) {
        warn!(error = %e, "failed to persist execution history");
    }
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

/// Build a Candle provider from the `[local]` TOML section.
///
/// When compiled without the `candle` feature, prints an error and exits.
fn build_candle_from_config(
    config: &agentzero_config::AgentZeroConfig,
) -> anyhow::Result<Box<dyn agentzero_core::Provider>> {
    #[cfg(feature = "candle")]
    {
        let local = &config.local;
        Ok(agentzero_providers::build_candle_provider(
            agentzero_providers::candle_provider::CandleConfig {
                model: local.model.clone(),
                filename: local.filename.clone(),
                n_ctx: local.n_ctx,
                temperature: local.temperature,
                top_p: local.top_p,
                max_output_tokens: local.max_output_tokens,
                seed: local.seed,
                repeat_penalty: local.repeat_penalty,
                device: local.device.clone(),
                chat_template: local.chat_template.clone(),
            },
        ))
    }
    #[cfg(not(feature = "candle"))]
    {
        let _ = config;
        anyhow::bail!(
            "provider 'candle' requires the 'candle' feature. \
             Rebuild with: cargo build --features candle"
        );
    }
}

const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api";

/// If the provider kind is not "openrouter" but the base_url is still the
/// openrouter default, resolve the correct base URL from the provider catalog.
fn resolve_base_url_from_catalog(config: &mut agentzero_config::AgentZeroConfig) {
    if config.provider.kind == "openrouter" {
        return;
    }
    let url = config.provider.base_url.trim();
    if !url.is_empty() && url != DEFAULT_OPENROUTER_BASE_URL {
        return; // user explicitly set a custom base_url
    }
    if let Some(meta) = local_provider_meta(&config.provider.kind) {
        config.provider.base_url = meta.default_base_url.to_string();
    } else if let Some(descriptor) = find_provider(&config.provider.kind) {
        if let Some(catalog_url) = descriptor.default_base_url {
            config.provider.base_url = catalog_url.to_string();
        }
    }
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

/// Build a [`MemoryStore`] from the configuration at `config_path`.
///
/// When running multiple agents that share the same database, callers should
/// build the store **once**, wrap it in `Arc`, and pass clones via
/// `RunAgentRequest::memory_override` to avoid redundant connections and
/// file-level lock contention.
pub async fn build_memory_store(config_path: &Path) -> anyhow::Result<Box<dyn MemoryStore>> {
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
                privacy_level: match r.privacy_level.as_deref() {
                    Some("local") => PrivacyLevel::Local,
                    Some("cloud") => PrivacyLevel::Cloud,
                    _ => PrivacyLevel::Either,
                },
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

    let global_boundary = &config.privacy.mode;

    let map: HashMap<String, DelegateConfig> = config
        .agents
        .iter()
        .map(|(name, agent)| {
            let provider_kind = agent.provider.clone();
            let base_url = resolve_delegate_base_url(&provider_kind);

            // Resolve agent's privacy boundary against global mode.
            let resolved_boundary = agentzero_core::common::privacy_helpers::resolve_boundary(
                &agent.privacy_boundary,
                global_boundary,
            );

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
                    privacy_boundary: resolved_boundary,
                    max_tokens: agent.max_tokens.unwrap_or(0),
                    max_cost_microdollars: agent
                        .max_cost_usd
                        .map(|usd| (usd * 1_000_000.0) as u64)
                        .unwrap_or(0),
                    system_prompt_hash: None,
                    instruction_method: agent.instruction_method.clone(),
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

        let _ = fs::remove_dir_all(dir);
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

        let _ = fs::remove_dir_all(dir);
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

        let _ = fs::remove_dir_all(dir);
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

        let _ = fs::remove_dir_all(dir);
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

        let _ = fs::remove_dir_all(dir);
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

        let _ = fs::remove_dir_all(dir);
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

        let _ = fs::remove_dir_all(dir);
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

    #[test]
    fn parse_hook_mode_trims_whitespace() {
        assert!(matches!(
            parse_hook_mode("  block  ").expect("block with whitespace should parse"),
            HookFailureMode::Block
        ));
        assert!(matches!(
            parse_hook_mode("\twarn\n").expect("warn with tabs/newlines should parse"),
            HookFailureMode::Warn
        ));
    }

    #[test]
    fn parse_hook_mode_rejects_wrong_case() {
        assert!(
            parse_hook_mode("Block").is_err(),
            "uppercase should be rejected"
        );
        assert!(
            parse_hook_mode("WARN").is_err(),
            "all-caps should be rejected"
        );
        assert!(
            parse_hook_mode("Ignore").is_err(),
            "title-case should be rejected"
        );
    }

    #[test]
    fn parse_hook_mode_rejects_empty_string() {
        let err = parse_hook_mode("").expect_err("empty string should fail");
        assert!(err.to_string().contains("invalid hook error mode"));
    }

    #[test]
    fn parse_hook_mode_rejects_whitespace_only() {
        let err = parse_hook_mode("   ").expect_err("whitespace-only should fail");
        assert!(err.to_string().contains("invalid hook error mode"));
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
            conversation_id: None,
            audio_config: None,
            max_tokens: 0,
            max_cost_microdollars: 0,
            cost_config: Default::default(),
            data_dir: std::path::PathBuf::from("/tmp"),
            tool_selector: None,
            source_channel: None,
            sender_id: None,
            dynamic_registry: None,
            task_manager: None,
            tool_evolver: None,
            recipe_store: None,
            pattern_capture: None,
            embedding_provider: None,
            trajectory_recorder: None,
            model_name: String::new(),
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
