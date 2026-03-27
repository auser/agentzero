use agentzero_core::common::local_providers::is_local_provider;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::warn;
use url::Url;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct AgentZeroConfig {
    pub provider: ProviderConfig,
    pub memory: MemoryConfig,
    pub agent: AgentSettings,
    pub security: SecurityConfig,
    pub autonomy: AutonomyConfig,
    pub observability: ObservabilityConfig,
    pub research: ResearchConfig,
    pub runtime: RuntimeConfig,
    pub browser: BrowserConfig,
    pub http_request: HttpRequestConfig,
    pub web_fetch: WebFetchConfig,
    pub web_search: WebSearchConfig,
    pub composio: ComposioConfig,
    pub pushover: PushoverConfig,
    pub cost: CostConfig,
    pub identity: IdentityConfig,
    pub multimodal: MultimodalConfig,
    pub audio: AudioConfig,
    pub skills: SkillsConfig,
    #[serde(alias = "provider_settings")]
    pub provider_options: ProviderOptionsConfig,
    pub gateway: GatewayConfig,
    pub channels_config: ChannelsGlobalConfig,
    pub query_classification: QueryClassificationConfig,
    pub model_providers: HashMap<String, ModelProviderProfile>,
    pub model_routes: Vec<ModelRoute>,
    pub embedding_routes: Vec<EmbeddingRoute>,
    pub agents: HashMap<String, DelegateAgentConfig>,
    pub privacy: PrivacyConfig,
    pub swarm: SwarmConfig,
    pub logging: LoggingConfig,
    pub code_interpreter: CodeInterpreterConfig,
    pub media_gen: MediaGenConfig,
    pub autopilot: AutopilotConfig,
    #[serde(default)]
    pub a2a: A2aConfig,
    #[serde(default)]
    pub sop: SopConfig,
    #[serde(default)]
    pub guardrails: GuardrailsConfig,
    #[serde(default)]
    pub local: LocalModelConfig,
}

impl AgentZeroConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.provider.kind.trim().is_empty() {
            return Err(anyhow!("provider.kind must not be empty"));
        }
        // In-process providers run locally — no base_url needed.
        if self.provider.kind == "builtin" || self.provider.kind == "candle" {
            return Ok(());
        }
        if self.provider.base_url.trim().is_empty() {
            return Err(anyhow!("provider.base_url must not be empty"));
        }
        let provider_url = Url::parse(&self.provider.base_url)
            .map_err(|_| anyhow!("provider.base_url must be a valid URL"))?;
        if !matches!(provider_url.scheme(), "http" | "https") {
            return Err(anyhow!("provider.base_url scheme must be http or https"));
        }
        if is_local_provider(&self.provider.kind) {
            let is_localhost = matches!(
                provider_url.host_str(),
                Some("localhost") | Some("127.0.0.1") | Some("0.0.0.0") | Some("::1")
            );
            if !is_localhost {
                if self.privacy.mode == "local_only" || self.privacy.enforce_local_provider {
                    return Err(anyhow!(
                        "privacy mode '{}' requires localhost base_url for local provider '{}', \
                         but got '{}'. Use http://localhost:<port> or change your provider.",
                        self.privacy.mode,
                        self.provider.kind,
                        self.provider.base_url,
                    ));
                }
                tracing::warn!(
                    "provider '{}' is a local provider but base_url '{}' is not localhost \
                     — did you mean to use a different provider?",
                    self.provider.kind,
                    self.provider.base_url,
                );
            }
        }
        if self.provider.model.trim().is_empty() {
            return Err(anyhow!("provider.model must not be empty"));
        }
        if !(0.0..=2.0).contains(&self.provider.default_temperature) {
            return Err(anyhow!(
                "provider.default_temperature must be between 0.0 and 2.0"
            ));
        }
        if let Some(api) = &self.provider.provider_api {
            if !matches!(api.as_str(), "openai-chat-completions" | "openai-responses") {
                return Err(anyhow!(
                    "provider.provider_api must be 'openai-chat-completions' or 'openai-responses'"
                ));
            }
        }

        if self.memory.backend.trim().is_empty() {
            return Err(anyhow!("memory.backend must not be empty"));
        }
        match self.memory.backend.as_str() {
            "sqlite" | "turso" => {}
            other => {
                return Err(anyhow!(
                    "unsupported memory.backend `{other}`; expected `sqlite` or `turso`"
                ));
            }
        }
        if self.memory.sqlite_path.trim().is_empty() {
            return Err(anyhow!("memory.sqlite_path must not be empty"));
        }

        if self.agent.max_tool_iterations == 0 {
            return Err(anyhow!("agent.max_tool_iterations must be > 0"));
        }
        if self.agent.request_timeout_ms == 0 {
            return Err(anyhow!("agent.request_timeout_ms must be > 0"));
        }
        if self.agent.memory_window_size == 0 {
            return Err(anyhow!("agent.memory_window_size must be > 0"));
        }
        if self.agent.max_prompt_chars == 0 {
            return Err(anyhow!("agent.max_prompt_chars must be > 0"));
        }
        if self.agent.hooks.timeout_ms == 0 {
            return Err(anyhow!("agent.hooks.timeout_ms must be > 0"));
        }
        if !is_valid_hook_error_mode(&self.agent.hooks.on_error_default) {
            return Err(anyhow!(
                "agent.hooks.on_error_default must be one of: block, warn, ignore"
            ));
        }
        if let Some(value) = &self.agent.hooks.on_error_low {
            if !is_valid_hook_error_mode(value) {
                return Err(anyhow!(
                    "agent.hooks.on_error_low must be one of: block, warn, ignore"
                ));
            }
        }
        if let Some(value) = &self.agent.hooks.on_error_medium {
            if !is_valid_hook_error_mode(value) {
                return Err(anyhow!(
                    "agent.hooks.on_error_medium must be one of: block, warn, ignore"
                ));
            }
        }
        if let Some(value) = &self.agent.hooks.on_error_high {
            if !is_valid_hook_error_mode(value) {
                return Err(anyhow!(
                    "agent.hooks.on_error_high must be one of: block, warn, ignore"
                ));
            }
        }

        if self.security.allowed_root.trim().is_empty() {
            return Err(anyhow!("security.allowed_root must not be empty"));
        }
        if self.security.allowed_commands.is_empty() && !self.agent.is_dev_mode() {
            return Err(anyhow!("security.allowed_commands must not be empty"));
        }

        if self.security.read_file.max_read_bytes == 0 {
            return Err(anyhow!("security.read_file.max_read_bytes must be > 0"));
        }
        if self.security.write_file.max_write_bytes == 0 {
            return Err(anyhow!("security.write_file.max_write_bytes must be > 0"));
        }

        if self.security.shell.max_args == 0 {
            return Err(anyhow!("security.shell.max_args must be > 0"));
        }
        if self.security.shell.max_arg_length == 0 {
            return Err(anyhow!("security.shell.max_arg_length must be > 0"));
        }
        if self.security.shell.max_output_bytes == 0 {
            return Err(anyhow!("security.shell.max_output_bytes must be > 0"));
        }
        if self.security.shell.forbidden_chars.is_empty() {
            return Err(anyhow!("security.shell.forbidden_chars must not be empty"));
        }

        // Note: allowed_servers is now optional — servers can come from mcp.json files.
        // The config layer discovers mcp.json files and merges them into the policy.

        if self.security.audit.enabled && self.security.audit.path.trim().is_empty() {
            return Err(anyhow!(
                "security.audit.path must not be empty when audit is enabled"
            ));
        }

        // Gateway validation
        if self.gateway.host.trim().is_empty() {
            return Err(anyhow!("gateway.host must not be empty"));
        }
        if self.gateway.port == 0 {
            return Err(anyhow!("gateway.port must be > 0"));
        }
        if !self.gateway.allow_public_bind
            && self.gateway.host != "127.0.0.1"
            && self.gateway.host != "::1"
            && self.gateway.host != "localhost"
        {
            return Err(anyhow!(
                "gateway.host `{}` binds publicly but gateway.allow_public_bind is false",
                self.gateway.host
            ));
        }

        // Autonomy validation
        match self.autonomy.level.trim() {
            "supervised" | "autonomous" | "semi" | "locked" => {}
            other => {
                return Err(anyhow!(
                    "autonomy.level must be one of: supervised, autonomous, semi, locked; got `{other}`"
                ));
            }
        }
        if self.autonomy.max_actions_per_hour == 0 {
            return Err(anyhow!("autonomy.max_actions_per_hour must be > 0"));
        }
        if self.autonomy.max_cost_per_day_cents == 0 {
            return Err(anyhow!("autonomy.max_cost_per_day_cents must be > 0"));
        }

        // Privacy validation
        match self.privacy.mode.as_str() {
            "off" | "private" | "local_only" | "encrypted" | "full" => {}
            other => {
                return Err(anyhow!(
                    "privacy.mode must be one of: off, private, local_only, encrypted, full; got `{other}`"
                ));
            }
        }
        if (self.privacy.mode == "local_only" || self.privacy.enforce_local_provider)
            && !is_local_provider(&self.provider.kind)
        {
            return Err(anyhow!(
                "privacy mode '{}' requires a local provider, but '{}' is a cloud provider; \
                 use ollama, llamacpp, lmstudio, vllm, sglang, or another local provider",
                self.privacy.mode,
                self.provider.kind
            ));
        }
        if self.privacy.noise.session_timeout_secs == 0 {
            return Err(anyhow!("privacy.noise.session_timeout_secs must be > 0"));
        }
        if self.privacy.noise.max_sessions == 0 {
            return Err(anyhow!("privacy.noise.max_sessions must be > 0"));
        }
        if self.privacy.sealed_envelopes.max_envelope_bytes == 0 {
            return Err(anyhow!(
                "privacy.sealed_envelopes.max_envelope_bytes must be > 0"
            ));
        }
        match self.privacy.noise.handshake_pattern.as_str() {
            "XX" | "IK" => {}
            other => {
                return Err(anyhow!(
                    "privacy.noise.handshake_pattern must be XX or IK; got `{other}`"
                ));
            }
        }
        // Encrypted and private modes require Noise to be enabled — without an
        // encrypted transport, there is no mechanism to enforce the promise.
        // Note: in gateway startup, "private" auto-enables Noise, so this only
        // fires if someone explicitly sets noise.enabled = false with private mode.
        if matches!(self.privacy.mode.as_str(), "encrypted" | "private")
            && !self.privacy.noise.enabled
        {
            return Err(anyhow!(
                "privacy.mode '{}' requires privacy.noise.enabled = true; \
                 either enable Noise or change the privacy mode",
                self.privacy.mode
            ));
        }

        // Per-agent privacy boundary validation.
        let valid_boundaries = ["", "inherit", "local_only", "encrypted_only", "any"];
        for (name, agent) in &self.agents {
            if !agent.privacy_boundary.is_empty()
                && !valid_boundaries.contains(&agent.privacy_boundary.as_str())
            {
                return Err(anyhow!(
                    "agents.{name}.privacy_boundary must be one of: inherit, local_only, \
                     encrypted_only, any; got '{}'",
                    agent.privacy_boundary
                ));
            }
            // Agent boundary can't be more permissive than global privacy mode.
            // Map global mode → boundary string for comparison.
            let global_boundary = match self.privacy.mode.as_str() {
                "local_only" => "local_only",
                "private" | "encrypted" | "full" => "encrypted_only",
                _ => "any",
            };
            if !agent.privacy_boundary.is_empty()
                && agent.privacy_boundary != "inherit"
                && global_boundary == "local_only"
                && agent.privacy_boundary != "local_only"
            {
                return Err(anyhow!(
                    "agents.{name}.privacy_boundary '{}' is more permissive than \
                     global privacy mode '{}' (local_only)",
                    agent.privacy_boundary,
                    self.privacy.mode
                ));
            }
        }

        // Per-tool privacy boundary validation.
        for (tool_name, boundary) in &self.security.tool_boundaries {
            if !valid_boundaries.contains(&boundary.as_str()) {
                return Err(anyhow!(
                    "security.tool_boundaries.{tool_name} must be one of: inherit, local_only, \
                     encrypted_only, any; got '{boundary}'"
                ));
            }
        }

        // Production mode validation (AGENTZERO_ENV=production).
        self.validate_production_mode()?;

        // Non-fatal validation warnings for routing config.
        if self.query_classification.enabled && self.query_classification.rules.is_empty() {
            warn!(
                "query_classification is enabled but has no rules — classification will be a no-op"
            );
        }
        for route in &self.embedding_routes {
            if route.provider.trim().is_empty() {
                warn!(hint = %route.hint, "embedding route has an empty provider field");
            }
            if route.model.trim().is_empty() {
                warn!(hint = %route.hint, "embedding route has an empty model field");
            }
        }

        Ok(())
    }

    /// Return a copy with secret fields masked for safe display.
    pub fn masked(&self) -> Self {
        let mut copy = self.clone();
        let mask = |opt: &mut Option<String>| {
            if opt.as_ref().is_some_and(|v| !v.is_empty()) {
                *opt = Some("****".to_string());
            }
        };
        mask(&mut copy.browser.computer_use.api_key);
        mask(&mut copy.web_fetch.api_key);
        mask(&mut copy.web_search.api_key);
        mask(&mut copy.web_search.brave_api_key);
        mask(&mut copy.web_search.perplexity_api_key);
        mask(&mut copy.web_search.exa_api_key);
        mask(&mut copy.web_search.jina_api_key);
        mask(&mut copy.composio.api_key);
        mask(&mut copy.skills.clawhub_token);
        mask(&mut copy.gateway.node_control.auth_token);
        for profile in copy.model_providers.values_mut() {
            mask(&mut profile.api_key);
        }
        for route in &mut copy.model_routes {
            mask(&mut route.api_key);
        }
        for route in &mut copy.embedding_routes {
            mask(&mut route.api_key);
        }
        for agent in copy.agents.values_mut() {
            mask(&mut agent.api_key);
        }
        copy
    }

    /// Check if the runtime environment is production (`AGENTZERO_ENV=production`).
    pub fn is_production() -> bool {
        std::env::var("AGENTZERO_ENV")
            .map(|v| v.eq_ignore_ascii_case("production"))
            .unwrap_or(false)
    }

    /// Strict validation rules enforced only when `AGENTZERO_ENV=production`.
    ///
    /// Production mode requires:
    /// - TLS enabled **or** explicit `gateway.allow_insecure = true`
    /// - Authentication enabled (require_pairing or API key auth)
    /// - Non-localhost bind with `allow_public_bind` acknowledged
    fn validate_production_mode(&self) -> anyhow::Result<()> {
        if !Self::is_production() {
            return Ok(());
        }

        // TLS or explicit allow_insecure.
        let tls_configured = self.gateway.tls.is_some();
        if !tls_configured && !self.gateway.allow_insecure {
            return Err(anyhow!(
                "production mode requires TLS (gateway.tls) or explicit gateway.allow_insecure = true"
            ));
        }

        // Authentication must be required (pairing or API key enforcement).
        if !self.gateway.require_pairing {
            return Err(anyhow!(
                "production mode requires gateway.require_pairing = true (authentication required)"
            ));
        }

        // Warn about localhost bind in production.
        let is_localhost = matches!(
            self.gateway.host.as_str(),
            "127.0.0.1" | "::1" | "localhost"
        );
        if is_localhost {
            warn!(
                "production mode with localhost bind ({}) — gateway will not be reachable externally",
                self.gateway.host
            );
        }

        Ok(())
    }

    /// Serialize this config to TOML.
    pub fn to_toml(&self) -> anyhow::Result<String> {
        toml::to_string_pretty(self).map_err(|e| anyhow!("failed to serialize config: {e}"))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ProviderConfig {
    #[serde(alias = "name", alias = "default_provider")]
    pub kind: String,
    pub base_url: String,
    pub model: String,
    pub default_temperature: f64,
    pub provider_api: Option<String>,
    pub model_support_vision: Option<bool>,
    #[serde(default)]
    pub transport: TransportSettings,
    /// Ordered fallback providers tried when the primary fails.
    /// Empty list (default) means no fallback.
    #[serde(default)]
    pub fallback_providers: Vec<FallbackProviderEntry>,
}

/// A fallback provider entry in the provider config.
///
/// Configured as `[[provider.fallback_providers]]` in TOML:
/// ```toml
/// [[provider.fallback_providers]]
/// kind = "openai"
/// base_url = "https://api.openai.com"
/// model = "gpt-4o"
/// api_key_env = "OPENAI_API_KEY"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FallbackProviderEntry {
    pub kind: String,
    pub base_url: String,
    pub model: String,
    /// Environment variable name containing the API key for this fallback provider.
    /// If unset, the primary provider's API key is used.
    pub api_key_env: Option<String>,
}

/// Transport-level settings loaded from `[provider.transport]` in TOML.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TransportSettings {
    pub timeout_ms: u64,
    pub max_retries: usize,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_reset_ms: u64,
}

impl Default for TransportSettings {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            max_retries: 3,
            circuit_breaker_threshold: 5,
            circuit_breaker_reset_ms: 30_000,
        }
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: "openrouter".to_string(),
            base_url: "https://openrouter.ai/api".to_string(),
            model: "anthropic/claude-sonnet-4-6".to_string(),
            default_temperature: 0.7,
            provider_api: None,
            model_support_vision: None,
            transport: TransportSettings::default(),
            fallback_providers: vec![],
        }
    }
}

/// Configuration for local (in-process) LLM inference.
///
/// Shared by both the `builtin` (llama.cpp) and `candle` providers.
/// Configured under `[local]` in TOML.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LocalModelConfig {
    /// HuggingFace repo or path to a GGUF file.
    pub model: String,
    /// Specific GGUF filename within the HuggingFace repo.
    pub filename: String,
    /// Context window size in tokens.
    pub n_ctx: u32,
    /// Temperature for sampling (0.0 = greedy, higher = more random).
    pub temperature: f64,
    /// Top-p (nucleus) sampling threshold.
    pub top_p: f64,
    /// Maximum number of tokens to generate per response.
    pub max_output_tokens: u32,
    /// Random seed for reproducible generation.
    pub seed: u64,
    /// Repetition penalty factor (1.0 = no penalty).
    pub repeat_penalty: f32,
    /// Device to use: "auto", "metal", "cuda", "cpu".
    pub device: String,
}

impl Default for LocalModelConfig {
    fn default() -> Self {
        Self {
            model: "Qwen/Qwen2.5-Coder-3B-Instruct-GGUF".to_string(),
            filename: "qwen2.5-coder-3b-instruct-q4_k_m.gguf".to_string(),
            n_ctx: 8192,
            temperature: 0.7,
            top_p: 0.9,
            max_output_tokens: 2048,
            seed: 42,
            repeat_penalty: 1.1,
            device: "auto".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub backend: String,
    #[serde(alias = "path")]
    pub sqlite_path: String,
    /// Connection pool size. 1 = no pool (Mutex-based), >1 = r2d2 pool.
    pub pool_size: u32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            backend: "sqlite".to_string(),
            sqlite_path: default_sqlite_path(),
            pool_size: 1,
        }
    }
}

fn default_sqlite_path() -> String {
    agentzero_core::common::paths::default_sqlite_path()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "./agentzero.db".to_string())
}

fn is_valid_hook_error_mode(value: &str) -> bool {
    matches!(value.trim(), "block" | "warn" | "ignore")
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AgentSettings {
    pub max_tool_iterations: usize,
    pub request_timeout_ms: u64,
    #[serde(alias = "max_history_messages")]
    pub memory_window_size: usize,
    pub max_prompt_chars: usize,
    pub mode: String,
    pub hooks: HookSettings,
    pub parallel_tools: bool,
    pub tool_dispatcher: String,
    pub compact_context: bool,
    pub loop_detection_no_progress_threshold: usize,
    pub loop_detection_ping_pong_cycles: usize,
    pub loop_detection_failure_streak: usize,
    /// Optional system prompt sent to the LLM at the start of each conversation.
    pub system_prompt: Option<String>,
    /// Per-run cost limit in USD. When exceeded, the agent stops.
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    /// Per-run token limit. When exceeded, the agent stops.
    #[serde(default)]
    pub max_tokens: Option<u64>,
    /// Per-tool execution timeout in milliseconds (0 = no timeout). Default: 120_000 (2 min).
    #[serde(default = "default_tool_timeout_ms")]
    pub tool_timeout_ms: u64,
    /// Tool selection strategy: "all" (default), "keyword", or "ai".
    #[serde(default)]
    pub tool_selection: Option<String>,
    /// Optional override model for AI-based tool selection (cheaper/faster model).
    #[serde(default)]
    pub tool_selection_model: Option<String>,
    /// Context summarization settings.
    #[serde(default)]
    pub summarization: SummarizationSettings,
    /// Enable the `agent_manage` tool for creating/managing persistent agents.
    #[serde(default)]
    pub enable_agent_manage: bool,
    /// Enable domain-driven research tools (domain_create, domain_search, etc.).
    #[serde(default)]
    pub enable_domain_tools: bool,
    /// Enable self-configuration tools (config_manage, skill_manage, plugin_scaffold).
    #[serde(default)]
    pub enable_self_config: bool,
    /// Enable the Claude Code delegation tool (spawns `claude` CLI as subprocess).
    #[serde(default)]
    pub enable_claude_code: bool,
    /// Enable CLI harness tools (Codex, Gemini, OpenCode CLI delegation).
    #[serde(default)]
    pub enable_cli_harness: bool,
    /// Enable dynamic tool creation at runtime (tool_create tool).
    #[serde(default)]
    pub enable_dynamic_tools: Option<bool>,
}

fn default_tool_timeout_ms() -> u64 {
    120_000
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_tool_iterations: 20,
            request_timeout_ms: 30_000,
            memory_window_size: 50,
            max_prompt_chars: 8_000,
            mode: "development".to_string(),
            hooks: HookSettings::default(),
            parallel_tools: false,
            tool_dispatcher: "auto".to_string(),
            compact_context: true,
            loop_detection_no_progress_threshold: 3,
            loop_detection_ping_pong_cycles: 2,
            loop_detection_failure_streak: 3,
            system_prompt: None,
            max_cost_usd: None,
            max_tokens: None,
            tool_timeout_ms: default_tool_timeout_ms(),
            tool_selection: None,
            tool_selection_model: None,
            summarization: SummarizationSettings::default(),
            enable_agent_manage: false,
            enable_domain_tools: false,
            enable_self_config: false,
            enable_claude_code: false,
            enable_cli_harness: false,
            enable_dynamic_tools: None,
        }
    }
}

impl AgentSettings {
    pub fn is_dev_mode(&self) -> bool {
        matches!(self.mode.trim(), "dev" | "development")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct HookSettings {
    pub enabled: bool,
    pub timeout_ms: u64,
    pub fail_closed: bool,
    pub on_error_default: String,
    pub on_error_low: Option<String>,
    pub on_error_medium: Option<String>,
    pub on_error_high: Option<String>,
}

impl Default for HookSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_ms: 250,
            fail_closed: false,
            on_error_default: "warn".to_string(),
            on_error_low: None,
            on_error_medium: None,
            on_error_high: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SecurityConfig {
    pub allowed_root: String,
    pub allowed_commands: Vec<String>,
    pub read_file: ReadFileConfig,
    pub write_file: WriteFileConfig,
    pub shell: ShellConfig,
    pub mcp: McpConfig,
    pub plugin: PluginConfig,
    pub audit: AuditConfig,
    pub url_access: UrlAccessConfig,
    pub otp: OtpConfig,
    pub estop: EstopConfig,
    pub outbound_leak_guard: OutboundLeakGuardConfig,
    pub perplexity_filter: PerplexityFilterConfig,
    pub syscall_anomaly: SyscallAnomalyConfig,
    /// Per-tool privacy boundaries. Keys are tool names, values are boundary
    /// strings: "inherit", "local_only", "encrypted_only", "any".
    #[serde(default)]
    pub tool_boundaries: std::collections::HashMap<String, String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            allowed_root: ".".to_string(),
            allowed_commands: vec![
                "ls".to_string(),
                "pwd".to_string(),
                "cat".to_string(),
                "echo".to_string(),
                "grep".to_string(),
                "find".to_string(),
                "head".to_string(),
                "tail".to_string(),
                "wc".to_string(),
                "sort".to_string(),
                "uniq".to_string(),
                "diff".to_string(),
                "file".to_string(),
                "which".to_string(),
                "basename".to_string(),
                "dirname".to_string(),
                "mkdir".to_string(),
                "cp".to_string(),
                "mv".to_string(),
                "rm".to_string(),
                "touch".to_string(),
                "date".to_string(),
                "env".to_string(),
                "test".to_string(),
                "tr".to_string(),
                "cut".to_string(),
                "xargs".to_string(),
                "sed".to_string(),
                "awk".to_string(),
                "git".to_string(),
                "cargo".to_string(),
                "rustc".to_string(),
                "npm".to_string(),
                "node".to_string(),
                "python3".to_string(),
            ],
            read_file: ReadFileConfig::default(),
            write_file: WriteFileConfig::default(),
            shell: ShellConfig::default(),
            mcp: McpConfig::default(),
            plugin: PluginConfig::default(),
            audit: AuditConfig::default(),
            url_access: UrlAccessConfig::default(),
            otp: OtpConfig::default(),
            estop: EstopConfig::default(),
            outbound_leak_guard: OutboundLeakGuardConfig::default(),
            perplexity_filter: PerplexityFilterConfig::default(),
            syscall_anomaly: SyscallAnomalyConfig::default(),
            tool_boundaries: std::collections::HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ReadFileConfig {
    pub max_read_bytes: u64,
    pub allow_binary: bool,
}

impl Default for ReadFileConfig {
    fn default() -> Self {
        Self {
            max_read_bytes: 256 * 1024,
            allow_binary: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WriteFileConfig {
    pub enabled: bool,
    pub max_write_bytes: u64,
}

impl Default for WriteFileConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_write_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ShellConfig {
    pub max_args: usize,
    pub max_arg_length: usize,
    pub max_output_bytes: usize,
    pub forbidden_chars: String,
    pub context_aware_parsing: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            max_args: 32,
            max_arg_length: 4096,
            max_output_bytes: 65536,
            forbidden_chars: ";&|><$`\n\r".to_string(),
            context_aware_parsing: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct McpConfig {
    pub enabled: bool,
    #[serde(default)]
    pub allowed_servers: Vec<String>,
}

/// A single MCP server entry as found in `mcp.json`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerEntry {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Optional SHA-256 hash of the server binary for attestation.
    /// When set, the binary is verified before spawning.
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Top-level structure of an `mcp.json` file.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct McpServersFile {
    #[serde(default, alias = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct PluginConfig {
    /// Enable WASM plugin discovery and loading.
    pub wasm_enabled: bool,
    /// Override for the global plugin install directory.
    /// Defaults to `{data_dir}/plugins/`.
    pub global_plugin_dir: Option<String>,
    /// Override for the project-level plugin directory.
    /// Defaults to `{workspace}/.agentzero/plugins/`.
    pub project_plugin_dir: Option<String>,
    /// Override for the development plugin directory (CWD hot-reload).
    /// Defaults to `{cwd}/plugins/`.
    pub dev_plugin_dir: Option<String>,
    /// URL to a plugin registry index (JSON manifest).
    pub registry_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AuditConfig {
    pub enabled: bool,
    pub path: String,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "./agentzero-audit.log".to_string(),
        }
    }
}

// --- Phase A3: New config sections ---

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AutonomyConfig {
    pub level: String,
    pub workspace_only: bool,
    pub forbidden_paths: Vec<String>,
    pub allowed_roots: Vec<String>,
    pub auto_approve: Vec<String>,
    pub always_ask: Vec<String>,
    pub allow_sensitive_file_reads: bool,
    pub allow_sensitive_file_writes: bool,
    pub non_cli_excluded_tools: Vec<String>,
    pub non_cli_approval_approvers: Vec<String>,
    pub non_cli_natural_language_approval_mode: String,
    pub non_cli_natural_language_approval_mode_by_channel: HashMap<String, String>,
    pub max_actions_per_hour: u32,
    /// Maximum actions per sender per hour. If None, uses the global limit.
    #[serde(default)]
    pub max_actions_per_sender_per_hour: Option<u32>,
    pub max_cost_per_day_cents: u32,
    pub require_approval_for_medium_risk: bool,
    pub block_high_risk_commands: bool,
}

impl Default for AutonomyConfig {
    fn default() -> Self {
        Self {
            level: "supervised".to_string(),
            workspace_only: true,
            forbidden_paths: vec![
                "/etc".to_string(),
                "/root".to_string(),
                "/proc".to_string(),
                "/sys".to_string(),
                "~/.ssh".to_string(),
                "~/.gnupg".to_string(),
                "~/.aws".to_string(),
            ],
            allowed_roots: Vec::new(),
            auto_approve: Vec::new(),
            always_ask: Vec::new(),
            allow_sensitive_file_reads: false,
            allow_sensitive_file_writes: false,
            non_cli_excluded_tools: Vec::new(),
            non_cli_approval_approvers: Vec::new(),
            non_cli_natural_language_approval_mode: "direct".to_string(),
            non_cli_natural_language_approval_mode_by_channel: HashMap::new(),
            max_actions_per_hour: 200,
            max_actions_per_sender_per_hour: None,
            max_cost_per_day_cents: 2000,
            require_approval_for_medium_risk: true,
            block_high_risk_commands: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ObservabilityConfig {
    pub backend: String,
    pub otel_endpoint: String,
    pub otel_service_name: String,
    pub runtime_trace_mode: String,
    pub runtime_trace_path: String,
    pub runtime_trace_max_entries: usize,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            backend: "none".to_string(),
            otel_endpoint: "http://localhost:4318".to_string(),
            otel_service_name: "agentzero".to_string(),
            runtime_trace_mode: "none".to_string(),
            runtime_trace_path: "state/runtime-trace.jsonl".to_string(),
            runtime_trace_max_entries: 200,
        }
    }
}

/// Log output format.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable text output (default).
    #[default]
    Text,
    /// Structured JSON — one JSON object per line.
    /// Suitable for container log aggregation (Fluentd, Datadog, CloudWatch, Loki).
    Json,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Output format: "text" (default) or "json".
    pub format: LogFormat,
    /// Default log level: "error", "warn", "info", "debug", "trace".
    pub level: String,
    /// Per-module log level overrides.
    /// Example: `{ "agentzero_gateway" = "debug" }`
    pub modules: HashMap<String, String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            format: LogFormat::Text,
            level: "error".to_string(),
            modules: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ResearchConfig {
    pub enabled: bool,
    pub trigger: String,
    pub keywords: Vec<String>,
    pub min_message_length: usize,
    pub max_iterations: usize,
    pub show_progress: bool,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger: "never".to_string(),
            keywords: vec![
                "find".to_string(),
                "search".to_string(),
                "check".to_string(),
                "investigate".to_string(),
            ],
            min_message_length: 50,
            max_iterations: 5,
            show_progress: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub kind: String,
    pub reasoning_enabled: Option<bool>,
    pub wasm: WasmRuntimeConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            kind: "native".to_string(),
            reasoning_enabled: None,
            wasm: WasmRuntimeConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WasmRuntimeConfig {
    pub tools_dir: String,
    pub fuel_limit: u64,
    pub memory_limit_mb: u64,
    pub max_module_size_mb: u64,
    pub allow_workspace_read: bool,
    pub allow_workspace_write: bool,
    pub allowed_hosts: Vec<String>,
    pub security: WasmSecurityConfig,
}

impl Default for WasmRuntimeConfig {
    fn default() -> Self {
        Self {
            tools_dir: "tools/wasm".to_string(),
            fuel_limit: 1_000_000,
            memory_limit_mb: 64,
            max_module_size_mb: 50,
            allow_workspace_read: false,
            allow_workspace_write: false,
            allowed_hosts: Vec::new(),
            security: WasmSecurityConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WasmSecurityConfig {
    pub require_workspace_relative_tools_dir: bool,
    pub reject_symlink_modules: bool,
    pub reject_symlink_tools_dir: bool,
    pub strict_host_validation: bool,
    pub capability_escalation_mode: String,
    pub module_hash_policy: String,
    pub module_sha256: HashMap<String, String>,
}

impl Default for WasmSecurityConfig {
    fn default() -> Self {
        Self {
            require_workspace_relative_tools_dir: true,
            reject_symlink_modules: true,
            reject_symlink_tools_dir: true,
            strict_host_validation: true,
            capability_escalation_mode: "deny".to_string(),
            module_hash_policy: "warn".to_string(),
            module_sha256: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BrowserConfig {
    pub enabled: bool,
    pub allowed_domains: Vec<String>,
    pub browser_open: String,
    pub session_name: Option<String>,
    pub backend: String,
    pub auto_backend_priority: Vec<String>,
    pub agent_browser_command: String,
    pub agent_browser_extra_args: Vec<String>,
    pub agent_browser_timeout_ms: u64,
    pub native_headless: bool,
    pub native_webdriver_url: String,
    pub native_chrome_path: Option<String>,
    pub computer_use: ComputerUseConfig,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_domains: Vec::new(),
            browser_open: "default".to_string(),
            session_name: None,
            backend: "agent_browser".to_string(),
            auto_backend_priority: Vec::new(),
            agent_browser_command: "agent-browser".to_string(),
            agent_browser_extra_args: Vec::new(),
            agent_browser_timeout_ms: 30_000,
            native_headless: true,
            native_webdriver_url: "http://127.0.0.1:9515".to_string(),
            native_chrome_path: None,
            computer_use: ComputerUseConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ComputerUseConfig {
    pub endpoint: String,
    pub api_key: Option<String>,
    pub timeout_ms: u64,
    pub allow_remote_endpoint: bool,
    pub window_allowlist: Vec<String>,
    pub max_coordinate_x: Option<u32>,
    pub max_coordinate_y: Option<u32>,
}

impl Default for ComputerUseConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8787/v1/actions".to_string(),
            api_key: None,
            timeout_ms: 15_000,
            allow_remote_endpoint: false,
            window_allowlist: Vec::new(),
            max_coordinate_x: None,
            max_coordinate_y: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct HttpRequestConfig {
    pub enabled: bool,
    pub allowed_domains: Vec<String>,
    pub max_response_size: usize,
    pub timeout_secs: u64,
    pub user_agent: String,
    pub credential_profiles: HashMap<String, CredentialProfile>,
}

impl Default for HttpRequestConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_domains: Vec::new(),
            max_response_size: 1_000_000,
            timeout_secs: 30,
            user_agent: "AgentZero/1.0".to_string(),
            credential_profiles: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct CredentialProfile {
    pub header_name: String,
    pub env_var: String,
    #[serde(default)]
    pub value_prefix: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WebFetchConfig {
    pub enabled: bool,
    pub provider: String,
    pub api_key: Option<String>,
    pub api_url: Option<String>,
    pub allowed_domains: Vec<String>,
    pub blocked_domains: Vec<String>,
    pub max_response_size: usize,
    pub timeout_secs: u64,
    pub user_agent: String,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "fast_html2md".to_string(),
            api_key: None,
            api_url: None,
            allowed_domains: vec!["*".to_string()],
            blocked_domains: Vec::new(),
            max_response_size: 500_000,
            timeout_secs: 30,
            user_agent: "AgentZero/1.0".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WebSearchConfig {
    pub enabled: bool,
    pub provider: String,
    pub fallback_providers: Vec<String>,
    pub retries_per_provider: u32,
    pub retry_backoff_ms: u64,
    pub api_key: Option<String>,
    pub api_url: Option<String>,
    pub brave_api_key: Option<String>,
    pub perplexity_api_key: Option<String>,
    pub exa_api_key: Option<String>,
    pub jina_api_key: Option<String>,
    pub max_results: usize,
    pub timeout_secs: u64,
    pub user_agent: String,
    pub domain_filter: Vec<String>,
    pub language_filter: Vec<String>,
    pub country: Option<String>,
    pub recency_filter: Option<String>,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "duckduckgo".to_string(),
            fallback_providers: Vec::new(),
            retries_per_provider: 0,
            retry_backoff_ms: 250,
            api_key: None,
            api_url: None,
            brave_api_key: None,
            perplexity_api_key: None,
            exa_api_key: None,
            jina_api_key: None,
            max_results: 5,
            timeout_secs: 15,
            user_agent: "AgentZero/1.0".to_string(),
            domain_filter: Vec::new(),
            language_filter: Vec::new(),
            country: None,
            recency_filter: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ComposioConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
    pub entity_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct PushoverConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CostConfig {
    pub enabled: bool,
    pub daily_limit_usd: f64,
    pub monthly_limit_usd: f64,
    pub warn_at_percent: u32,
    pub allow_override: bool,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            daily_limit_usd: 10.0,
            monthly_limit_usd: 100.0,
            warn_at_percent: 80,
            allow_override: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct IdentityConfig {
    pub format: String,
    pub aieos_path: Option<String>,
    pub aieos_inline: Option<String>,
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            format: "markdown".to_string(),
            aieos_path: None,
            aieos_inline: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MultimodalConfig {
    pub max_images: usize,
    pub max_image_size_mb: usize,
    pub allow_remote_fetch: bool,
}

impl Default for MultimodalConfig {
    fn default() -> Self {
        Self {
            max_images: 4,
            max_image_size_mb: 5,
            allow_remote_fetch: false,
        }
    }
}

/// Configuration for audio transcription via a Whisper-compatible API.
///
/// Used to transcribe `[AUDIO:path]` markers in user messages before sending
/// them to the LLM. Compatible with Groq, OpenAI, and any Whisper-compatible
/// `/audio/transcriptions` endpoint.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Whisper-compatible transcription endpoint.
    pub api_url: String,
    /// API key for the transcription service (e.g. `GROQ_API_KEY`).
    pub api_key: Option<String>,
    /// Language hint for transcription (e.g. `"en"`). Optional.
    pub language: Option<String>,
    /// Whisper model name (e.g. `"whisper-large-v3"`).
    pub model: String,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            api_url: "https://api.groq.com/openai/v1/audio/transcriptions".to_string(),
            api_key: None,
            language: None,
            model: "whisper-large-v3".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SkillsConfig {
    pub open_skills_enabled: bool,
    pub open_skills_dir: Option<String>,
    pub prompt_injection_mode: String,
    pub clawhub_token: Option<String>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            open_skills_enabled: false,
            open_skills_dir: None,
            prompt_injection_mode: "full".to_string(),
            clawhub_token: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ProviderOptionsConfig {
    pub reasoning_level: Option<String>,
    pub transport: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub host: String,
    pub port: u16,
    pub require_pairing: bool,
    pub allow_public_bind: bool,
    pub node_control: NodeControlConfig,
    /// When true, the gateway operates as a privacy relay only.
    /// Normal agent endpoints return 503; only relay routes are active.
    pub relay_mode: bool,
    /// Relay-specific configuration.
    pub relay: RelayConfig,
    /// TLS configuration. When present, the gateway serves HTTPS.
    pub tls: Option<TlsConfig>,
    /// When true, allows running without TLS in production mode.
    /// Must be explicitly set to acknowledge the security implications.
    #[serde(default)]
    pub allow_insecure: bool,
    /// Public URL where this gateway is accessible from the internet.
    /// Used for webhook auto-registration with platforms (e.g. Telegram setWebhook).
    /// Example: "https://api.example.com"
    #[serde(default)]
    pub public_url: Option<String>,
    /// WebSocket configuration.
    #[serde(default)]
    pub websocket: WebSocketConfig,
}

/// Tunable WebSocket timeout and size parameters.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WebSocketConfig {
    /// Ping interval in seconds (default 30).
    pub heartbeat_interval_secs: u64,
    /// Close if no pong received within this many seconds (default 60).
    pub pong_timeout_secs: u64,
    /// Close if no client message within this many seconds (default 300).
    pub idle_timeout_secs: u64,
    /// Maximum message size in bytes (default 2 MB).
    pub max_message_bytes: usize,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval_secs: 30,
            pong_timeout_secs: 60,
            idle_timeout_secs: 300,
            max_message_bytes: 2 * 1024 * 1024,
        }
    }
}

/// TLS certificate configuration for the gateway.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    /// Path to the PEM-encoded certificate file (or certificate chain).
    pub cert_path: String,
    /// Path to the PEM-encoded private key file.
    pub key_path: String,
    /// Path to a PEM-encoded CA certificate for client certificate verification (mTLS).
    /// When set, clients must present a certificate signed by this CA.
    #[serde(default)]
    pub client_ca_path: Option<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 42617,
            require_pairing: true,
            allow_public_bind: false,
            node_control: NodeControlConfig::default(),
            relay_mode: false,
            relay: RelayConfig::default(),
            tls: None,
            allow_insecure: false,
            public_url: None,
            websocket: WebSocketConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct NodeControlConfig {
    pub enabled: bool,
    pub auth_token: Option<String>,
    pub allowed_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ChannelsGlobalConfig {
    pub message_timeout_secs: u64,
    pub group_reply: HashMap<String, GroupReplyConfig>,
    pub ack_reaction: HashMap<String, AckReactionConfig>,
    pub stream_mode: String,
    pub draft_update_interval_ms: u64,
    pub interrupt_on_new_message: bool,
    /// Default privacy boundary applied to all channels unless overridden.
    /// Empty string means inherit the global `privacy.mode`.
    pub default_privacy_boundary: String,
}

impl Default for ChannelsGlobalConfig {
    fn default() -> Self {
        Self {
            message_timeout_secs: 300,
            group_reply: HashMap::new(),
            ack_reaction: HashMap::new(),
            stream_mode: "off".to_string(),
            draft_update_interval_ms: 500,
            interrupt_on_new_message: false,
            default_privacy_boundary: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GroupReplyConfig {
    pub mode: String,
    pub allowed_sender_ids: Vec<String>,
    pub bot_name: Option<String>,
}

impl Default for GroupReplyConfig {
    fn default() -> Self {
        Self {
            mode: "all_messages".to_string(),
            allowed_sender_ids: Vec::new(),
            bot_name: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AckReactionConfig {
    pub enabled: bool,
    pub emoji_pool: Vec<String>,
    pub strategy: String,
    pub sample_rate: f64,
    pub rules: Vec<AckReactionRule>,
}

impl Default for AckReactionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            emoji_pool: vec!["👍".to_string(), "👀".to_string(), "🤔".to_string()],
            strategy: "random".to_string(),
            sample_rate: 1.0,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct AckReactionRule {
    pub contains_any: Vec<String>,
    pub contains_all: Vec<String>,
    pub contains_none: Vec<String>,
    pub regex: Option<String>,
    pub sender_ids: Vec<String>,
    pub chat_ids: Vec<String>,
    pub emoji_override: Vec<String>,
}

// --- Phase A4: Model provider profiles ---

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ModelProviderProfile {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub wire_api: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub requires_openai_auth: bool,
}

// --- Phase A5: Model/embedding routes and query classification ---

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ModelRoute {
    pub hint: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: Option<usize>,
    pub api_key: Option<String>,
    pub transport: Option<String>,
    /// Privacy level: "local", "cloud", or "either" (default).
    #[serde(default)]
    pub privacy_level: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct EmbeddingRoute {
    pub hint: String,
    pub provider: String,
    pub model: String,
    pub dimensions: Option<usize>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct QueryClassificationConfig {
    pub enabled: bool,
    pub rules: Vec<QueryClassificationRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct QueryClassificationRule {
    pub hint: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    #[serde(default)]
    pub priority: i32,
}

// --- Phase A6: Delegate sub-agent config ---

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DelegateAgentConfig {
    pub provider: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub api_key: Option<String>,
    pub temperature: Option<f64>,
    pub max_depth: usize,
    pub agentic: bool,
    pub allowed_tools: Vec<String>,
    pub max_iterations: usize,
    /// Per-agent privacy boundary: "inherit", "local_only", "encrypted_only", "any".
    #[serde(default)]
    pub privacy_boundary: String,
    /// Restrict this agent to only use these provider kinds.
    #[serde(default)]
    pub allowed_providers: Vec<String>,
    /// Block this agent from using these provider kinds.
    #[serde(default)]
    pub blocked_providers: Vec<String>,
    /// Per-run cost limit in USD for this sub-agent.
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    /// Per-run token limit for this sub-agent.
    #[serde(default)]
    pub max_tokens: Option<u64>,
}

impl Default for DelegateAgentConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            model: String::new(),
            system_prompt: None,
            api_key: None,
            temperature: None,
            max_depth: 3,
            agentic: false,
            allowed_tools: Vec::new(),
            max_iterations: 10,
            privacy_boundary: String::new(),
            allowed_providers: Vec::new(),
            blocked_providers: Vec::new(),
            max_cost_usd: None,
            max_tokens: None,
        }
    }
}

// --- Phase B2-B6: Security config extensions ---

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct UrlAccessConfig {
    pub block_private_ip: bool,
    pub allow_cidrs: Vec<String>,
    pub allow_domains: Vec<String>,
    pub allow_loopback: bool,
    pub require_first_visit_approval: bool,
    pub enforce_domain_allowlist: bool,
    pub domain_allowlist: Vec<String>,
    pub domain_blocklist: Vec<String>,
    pub approved_domains: Vec<String>,
}

impl Default for UrlAccessConfig {
    fn default() -> Self {
        Self {
            block_private_ip: true,
            allow_cidrs: Vec::new(),
            allow_domains: Vec::new(),
            allow_loopback: false,
            require_first_visit_approval: false,
            enforce_domain_allowlist: false,
            domain_allowlist: Vec::new(),
            domain_blocklist: Vec::new(),
            approved_domains: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OtpConfig {
    pub enabled: bool,
    pub method: String,
    pub token_ttl_secs: u64,
    pub cache_valid_secs: u64,
    pub gated_actions: Vec<String>,
    pub gated_domains: Vec<String>,
    pub gated_domain_categories: Vec<String>,
}

impl Default for OtpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            method: "totp".to_string(),
            token_ttl_secs: 30,
            cache_valid_secs: 300,
            gated_actions: vec![
                "shell".to_string(),
                "file_write".to_string(),
                "browser_open".to_string(),
                "browser".to_string(),
                "memory_forget".to_string(),
            ],
            gated_domains: Vec::new(),
            gated_domain_categories: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct EstopConfig {
    pub enabled: bool,
    pub state_file: String,
    pub require_otp_to_resume: bool,
}

impl Default for EstopConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            state_file: "~/.agentzero/estop-state.json".to_string(),
            require_otp_to_resume: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OutboundLeakGuardConfig {
    pub enabled: bool,
    pub action: String,
    pub sensitivity: f64,
    /// Additional regex patterns to detect as credential leaks.
    /// Each entry is a `{name: "pattern_name", regex: "regex_pattern"}` pair.
    #[serde(default)]
    pub extra_patterns: Vec<LeakPatternEntry>,
}

/// A user-defined leak detection pattern.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LeakPatternEntry {
    /// Human-readable name for the pattern (used in redaction markers).
    pub name: String,
    /// Regex pattern to match against outbound text.
    pub regex: String,
}

impl Default for OutboundLeakGuardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            action: "redact".to_string(),
            sensitivity: 0.7,
            extra_patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PerplexityFilterConfig {
    pub enable_perplexity_filter: bool,
    pub perplexity_threshold: f64,
    pub suffix_window_chars: usize,
    pub min_prompt_chars: usize,
    pub symbol_ratio_threshold: f64,
}

impl Default for PerplexityFilterConfig {
    fn default() -> Self {
        Self {
            enable_perplexity_filter: false,
            perplexity_threshold: 18.0,
            suffix_window_chars: 64,
            min_prompt_chars: 32,
            symbol_ratio_threshold: 0.20,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SyscallAnomalyConfig {
    pub enabled: bool,
    pub strict_mode: bool,
    pub alert_on_unknown_syscall: bool,
    pub max_denied_events_per_minute: u32,
    pub max_total_events_per_minute: u32,
    pub max_alerts_per_minute: u32,
    pub alert_cooldown_secs: u64,
    pub log_path: String,
    pub baseline_syscalls: Vec<String>,
}

impl Default for SyscallAnomalyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strict_mode: false,
            alert_on_unknown_syscall: true,
            max_denied_events_per_minute: 5,
            max_total_events_per_minute: 120,
            max_alerts_per_minute: 30,
            alert_cooldown_secs: 20,
            log_path: "syscall-anomalies.log".to_string(),
            baseline_syscalls: vec![
                "read".to_string(),
                "write".to_string(),
                "openat".to_string(),
                "close".to_string(),
                "execve".to_string(),
                "futex".to_string(),
            ],
        }
    }
}

// --- Privacy AI configuration ---

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PrivacyConfig {
    /// Privacy mode:
    /// - `"off"` — no privacy features.
    /// - `"private"` — blocks network tools (web_search, http_request, etc.) but
    ///   allows explicitly-configured cloud AI providers. Auto-enables Noise +
    ///   key rotation. Per-agent boundary defaults to `encrypted_only`.
    /// - `"local_only"` — all traffic stays on-device; cloud providers rejected.
    /// - `"encrypted"` — cloud providers allowed through Noise-encrypted transport.
    /// - `"full"` — all privacy features auto-enabled (noise, sealed envelopes,
    ///   key rotation); cloud providers allowed through encrypted transport.
    ///
    /// Simple: just set `"private"`, `"encrypted"` or `"full"` and everything auto-configures.
    /// Advanced: set individual `noise`, `sealed_envelopes`, `key_rotation` options.
    pub mode: String,
    /// When true, reject cloud providers — only local providers allowed.
    pub enforce_local_provider: bool,
    /// When true, block all outbound network calls to non-loopback destinations
    /// during agent execution (strict local-only mode).
    pub block_cloud_providers: bool,
    /// Noise Protocol settings for E2E encrypted gateway communication.
    pub noise: NoiseConfig,
    /// Sealed envelope settings for zero-knowledge packet routing.
    pub sealed_envelopes: SealedEnvelopeConfig,
    /// Automatic key rotation settings.
    pub key_rotation: KeyRotationConfig,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            mode: "off".to_string(),
            enforce_local_provider: false,
            block_cloud_providers: false,
            noise: NoiseConfig::default(),
            sealed_envelopes: SealedEnvelopeConfig::default(),
            key_rotation: KeyRotationConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct NoiseConfig {
    pub enabled: bool,
    /// Noise handshake pattern: "XX" (mutual auth) or "IK" (known server key).
    pub handshake_pattern: String,
    /// Session timeout in seconds.
    pub session_timeout_secs: u64,
    /// Maximum concurrent Noise sessions.
    pub max_sessions: usize,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            handshake_pattern: "XX".to_string(),
            session_timeout_secs: 3600,
            max_sessions: 256,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SealedEnvelopeConfig {
    pub enabled: bool,
    /// Default TTL for sealed envelopes in seconds.
    pub default_ttl_secs: u32,
    /// Maximum envelope size in bytes.
    pub max_envelope_bytes: usize,
    /// Enable randomized timing jitter on relay submit/poll responses
    /// to mitigate traffic-analysis side-channels.
    pub timing_jitter_enabled: bool,
    /// Minimum jitter delay on submit responses (milliseconds).
    pub submit_jitter_min_ms: u32,
    /// Maximum jitter delay on submit responses (milliseconds).
    pub submit_jitter_max_ms: u32,
    /// Minimum jitter delay on poll responses (milliseconds).
    pub poll_jitter_min_ms: u32,
    /// Maximum jitter delay on poll responses (milliseconds).
    pub poll_jitter_max_ms: u32,
}

impl Default for SealedEnvelopeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_ttl_secs: 86400,
            max_envelope_bytes: 1_048_576,
            timing_jitter_enabled: false,
            submit_jitter_min_ms: 10,
            submit_jitter_max_ms: 100,
            poll_jitter_min_ms: 20,
            poll_jitter_max_ms: 200,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct KeyRotationConfig {
    pub enabled: bool,
    /// Rotation interval in seconds (default: 7 days).
    pub rotation_interval_secs: u64,
    /// Overlap period where both old and new keys are valid.
    pub overlap_secs: u64,
    /// Path to the key store directory. Empty = default data dir.
    pub key_store_path: String,
}

impl Default for KeyRotationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rotation_interval_secs: 604_800,
            overlap_secs: 86_400,
            key_store_path: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RelayConfig {
    /// Random timing jitter range in milliseconds (0-N) added to relay responses
    /// to prevent timing analysis.
    pub timing_jitter_ms: u64,
    /// Maximum number of envelopes per routing_id mailbox.
    pub max_mailbox_size: usize,
    /// Garbage collection interval in seconds for expired envelopes.
    pub gc_interval_secs: u64,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            timing_jitter_ms: 500,
            max_mailbox_size: 1000,
            gc_interval_secs: 60,
        }
    }
}

// ─── Swarm (multi-agent) configuration ───────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SwarmConfig {
    pub enabled: bool,
    pub max_agents: usize,
    /// Grace period (ms) for in-flight chains on shutdown.
    pub shutdown_grace_ms: u64,
    /// Capacity of the event bus broadcast channel.
    pub event_bus_capacity: usize,
    /// Event bus backend: "memory" (default), "file", or "sqlite".
    /// - "memory": in-process broadcast only (no persistence).
    /// - "file": JSONL file-backed (requires `event_log_path`).
    /// - "sqlite": SQLite WAL-mode persistent bus (requires `event_db_path`).
    #[serde(default)]
    pub event_bus: Option<String>,
    /// Path to a JSONL file for persistent event logging (file bus backend).
    /// When set and `event_bus` is not explicitly configured, uses the file backend.
    #[serde(default)]
    pub event_log_path: Option<String>,
    /// Path to SQLite database for the SQLite event bus backend.
    /// Only used when `event_bus = "sqlite"`.
    #[serde(default)]
    pub event_db_path: Option<String>,
    /// Retention period in days for SQLite event bus GC. Default: 7.
    #[serde(default = "default_event_retention_days")]
    pub event_retention_days: u32,
    /// Port for gossip TCP listener (only used when `event_bus = "gossip"`).
    #[serde(default)]
    pub gossip_port: Option<u16>,
    /// Addresses of gossip peers (e.g. `["192.168.1.10:9100", "192.168.1.11:9100"]`).
    #[serde(default)]
    pub gossip_peers: Vec<String>,
    pub router: SwarmRouterConfig,
    /// Named agent definitions keyed by agent id.
    #[serde(default)]
    pub agents: HashMap<String, SwarmAgentConfig>,
    /// Explicit sequential pipelines for common workflows.
    #[serde(default)]
    pub pipelines: Vec<PipelineConfig>,
    /// Lane-based concurrency configuration.
    #[serde(default)]
    pub lanes: LanesConfig,
    /// Depth-gated tool policy for sub-agent nesting.
    #[serde(default)]
    pub depth_policy: Vec<DepthRuleConfig>,
}

fn default_event_retention_days() -> u32 {
    7
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_agents: 10,
            shutdown_grace_ms: 5_000,
            event_bus_capacity: 256,
            event_bus: None,
            event_log_path: None,
            event_db_path: None,
            event_retention_days: 7,
            gossip_port: None,
            gossip_peers: Vec::new(),
            router: SwarmRouterConfig::default(),
            agents: HashMap::new(),
            pipelines: Vec::new(),
            lanes: LanesConfig::default(),
            depth_policy: Vec::new(),
        }
    }
}

/// Lane-based concurrency configuration for the swarm coordinator.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LanesConfig {
    /// Max concurrent interactive requests (default: 1 = serialized).
    pub main_concurrency: usize,
    /// Max concurrent cron/scheduled jobs.
    pub cron_concurrency: usize,
    /// Max concurrent sub-agent executions.
    pub subagent_concurrency: usize,
    /// Max queued items per lane before backpressure.
    pub queue_capacity: usize,
}

impl Default for LanesConfig {
    fn default() -> Self {
        Self {
            main_concurrency: 1,
            cron_concurrency: 3,
            subagent_concurrency: 5,
            queue_capacity: 64,
        }
    }
}

/// Depth-gated tool policy rule (config representation).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
#[derive(Default)]
pub struct DepthRuleConfig {
    pub max_depth: u8,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub denied_tools: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SwarmRouterConfig {
    /// Provider kind for the routing LLM (e.g. "anthropic", "ollama").
    pub provider: String,
    /// Model to use for classification (fast + cheap, e.g. "claude-haiku-4.5").
    pub model: String,
    /// Base URL override for the router provider.
    pub base_url: String,
    /// API key override for the router provider.
    pub api_key: String,
    /// Fall back to keyword matching if the AI router fails.
    pub fallback_to_keywords: bool,
}

impl Default for SwarmRouterConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            model: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            fallback_to_keywords: true,
        }
    }
}

/// Settings for the `converse` tool — bidirectional agent-to-agent (or
/// agent-to-human) multi-turn conversations.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ConversationConfig {
    /// Maximum turns allowed per conversation (0 = default of 10).
    pub max_turns: usize,
    /// Per-turn timeout in seconds (0 = default of 120).
    pub turn_timeout_secs: u64,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            max_turns: 10,
            turn_timeout_secs: 120,
        }
    }
}

/// Configuration for a single agent in the swarm.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SwarmAgentConfig {
    /// Human-readable agent name (e.g. "Image Generator").
    pub name: String,
    /// What this agent does — used by the AI router for classification.
    pub description: String,
    /// Keywords for fallback keyword-based routing.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// LLM provider kind (e.g. "anthropic", "openai", "ollama").
    pub provider: String,
    /// Model identifier.
    pub model: String,
    /// Base URL override.
    pub base_url: String,
    /// API key override.
    pub api_key: String,
    /// Privacy boundary: "local_only", "encrypted_only", "any", or "" (inherit).
    #[serde(default)]
    pub privacy_boundary: String,
    /// Tools this agent is allowed to use.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Topic patterns this agent reacts to (e.g. ["channel.*.message", "task.image.*"]).
    #[serde(default)]
    pub subscribes_to: Vec<String>,
    /// Topics this agent publishes when it produces output.
    #[serde(default)]
    pub produces: Vec<String>,
    /// Optional system prompt for this agent.
    pub system_prompt: Option<String>,
    /// Maximum tool iterations per request.
    pub max_iterations: usize,
    /// Conversation settings for bidirectional agent-to-agent interactions.
    #[serde(default)]
    pub conversation: ConversationConfig,
}

impl Default for SwarmAgentConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            keywords: Vec::new(),
            provider: String::new(),
            model: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            privacy_boundary: String::new(),
            allowed_tools: Vec::new(),
            subscribes_to: Vec::new(),
            produces: Vec::new(),
            system_prompt: None,
            max_iterations: 20,
            conversation: ConversationConfig::default(),
        }
    }
}

impl SwarmAgentConfig {
    /// Create a new swarm agent config with the given name and description.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            ..Default::default()
        }
    }

    /// Set the provider and model.
    pub fn with_provider(mut self, provider: impl Into<String>, model: impl Into<String>) -> Self {
        self.provider = provider.into();
        self.model = model.into();
        self
    }

    /// Set the system prompt.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set routing keywords.
    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = keywords;
        self
    }

    /// Set allowed tools.
    pub fn with_allowed_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = tools;
        self
    }

    /// Set event bus subscription topics.
    pub fn with_subscriptions(mut self, topics: Vec<String>) -> Self {
        self.subscribes_to = topics;
        self
    }

    /// Set output topics.
    pub fn with_produces(mut self, topics: Vec<String>) -> Self {
        self.produces = topics;
        self
    }
}

/// An explicit sequential pipeline of agents.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PipelineConfig {
    pub name: String,
    pub trigger: PipelineTriggerConfig,
    /// Ordered list of agent ids to execute sequentially.
    pub steps: Vec<String>,
    /// Send the final output back to the originating channel.
    pub channel_reply: bool,
    /// What to do when a step fails: "abort", "skip", or "retry".
    pub on_step_error: String,
    /// Max retry attempts (only used when on_step_error = "retry").
    pub max_retries: u8,
    /// Per-step timeout in seconds.
    pub step_timeout_secs: u64,
    /// Execution mode: "sequential" (default), "fanout", or "mixed".
    #[serde(default = "default_execution_mode")]
    pub execution_mode: String,
    /// Fan-out steps: groups of agents to run in parallel.
    #[serde(default)]
    pub fanout_steps: Vec<FanOutStepConfig>,
    /// Publish an AnnounceMessage when the pipeline completes.
    #[serde(default)]
    pub announce_on_complete: bool,
}

fn default_execution_mode() -> String {
    "sequential".to_string()
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            trigger: PipelineTriggerConfig::default(),
            steps: Vec::new(),
            channel_reply: true,
            on_step_error: "abort".to_string(),
            max_retries: 3,
            step_timeout_secs: 120,
            execution_mode: default_execution_mode(),
            fanout_steps: Vec::new(),
            announce_on_complete: false,
        }
    }
}

/// A fan-out step: multiple agents run in parallel, results merged.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct FanOutStepConfig {
    /// Agent ids to run in parallel.
    pub agents: Vec<String>,
    /// Merge strategy: "wait_all" (default), "wait_any", "wait_quorum".
    pub merge: String,
    /// Minimum agents required for quorum (only with "wait_quorum").
    pub quorum_min: usize,
}

impl Default for FanOutStepConfig {
    fn default() -> Self {
        Self {
            agents: Vec::new(),
            merge: "wait_all".to_string(),
            quorum_min: 1,
        }
    }
}

/// How a pipeline is triggered.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct PipelineTriggerConfig {
    /// Trigger when the message contains any of these keywords.
    pub keywords: Vec<String>,
    /// Trigger when the message matches this regex.
    pub regex: String,
    /// Trigger when an event matches this topic.
    pub topic: String,
    /// Trigger when the AI router classifies the message with this label.
    pub ai_classified: String,
}

// ---------------------------------------------------------------------------
// Code Interpreter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CodeInterpreterConfig {
    pub enabled: bool,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
    pub allowed_languages: Vec<String>,
}

impl Default for CodeInterpreterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_ms: 30_000,
            max_output_bytes: 65536,
            allowed_languages: vec!["python".into(), "javascript".into()],
        }
    }
}

// ---------------------------------------------------------------------------
// Context Summarization
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SummarizationSettings {
    pub enabled: bool,
    pub keep_recent: usize,
    pub min_entries_for_summarization: usize,
    pub max_summary_chars: usize,
}

impl Default for SummarizationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            keep_recent: 10,
            min_entries_for_summarization: 20,
            max_summary_chars: 2000,
        }
    }
}

// ---------------------------------------------------------------------------
// Media Generation (TTS, Image, Video)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct MediaGenConfig {
    pub tts: TtsToolConfig,
    pub image_gen: ImageGenToolConfig,
    pub video_gen: VideoGenToolConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TtsToolConfig {
    pub enabled: bool,
    pub api_url: String,
    pub api_key_env: String,
    pub model: String,
    pub default_voice: String,
    pub timeout_ms: u64,
}

impl Default for TtsToolConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_url: "https://api.openai.com/v1/audio/speech".into(),
            api_key_env: "OPENAI_API_KEY".into(),
            model: "tts-1".into(),
            default_voice: "alloy".into(),
            timeout_ms: 60_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ImageGenToolConfig {
    pub enabled: bool,
    pub api_url: String,
    pub api_key_env: String,
    pub model: String,
    pub default_size: String,
    pub timeout_ms: u64,
}

impl Default for ImageGenToolConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_url: "https://api.openai.com/v1/images/generations".into(),
            api_key_env: "OPENAI_API_KEY".into(),
            model: "dall-e-3".into(),
            default_size: "1024x1024".into(),
            timeout_ms: 60_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct VideoGenToolConfig {
    pub enabled: bool,
    pub api_url: String,
    pub api_key_env: String,
    pub model: String,
    pub timeout_ms: u64,
}

impl Default for VideoGenToolConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_url: "https://api.minimax.chat/v1/video_generation".into(),
            api_key_env: "MINIMAX_API_KEY".into(),
            model: "MiniMax-Hailuo-2.3".into(),
            timeout_ms: 300_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Autopilot configuration
// ---------------------------------------------------------------------------

/// Condition for a trigger rule (used in TOML config).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutopilotTriggerCondition {
    EventMatch { event_type: String },
    Cron { schedule: String },
    MetricThreshold { metric: String, threshold: f64 },
}

/// Action for a trigger rule (used in TOML config).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutopilotTriggerAction {
    ProposeTask { agent: String, prompt: String },
    NotifyAgent { agent: String, message: String },
    RunPipeline { pipeline: String },
}

/// A trigger rule defined in TOML config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutopilotTriggerConfig {
    pub name: String,
    pub condition: AutopilotTriggerCondition,
    pub action: AutopilotTriggerAction,
    #[serde(default = "default_autopilot_cooldown")]
    pub cooldown_secs: u64,
    #[serde(default = "default_autopilot_enabled")]
    pub enabled: bool,
}

fn default_autopilot_cooldown() -> u64 {
    3600
}

fn default_autopilot_enabled() -> bool {
    true
}

/// Configuration for the autonomous company loop.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AutopilotConfig {
    pub enabled: bool,
    pub supabase_url: String,
    #[serde(default)]
    pub supabase_service_role_key: String,
    #[serde(default = "default_autopilot_max_daily_spend")]
    pub max_daily_spend_cents: u64,
    #[serde(default = "default_autopilot_max_concurrent")]
    pub max_concurrent_missions: usize,
    #[serde(default = "default_autopilot_max_proposals")]
    pub max_proposals_per_hour: usize,
    #[serde(default = "default_autopilot_max_missions_agent")]
    pub max_missions_per_agent_per_day: usize,
    #[serde(default = "default_autopilot_stale_threshold")]
    pub stale_threshold_minutes: u32,
    pub reaction_matrix_path: Option<String>,
    #[serde(default)]
    pub triggers: Vec<AutopilotTriggerConfig>,
}

fn default_autopilot_max_daily_spend() -> u64 {
    500
}

fn default_autopilot_max_concurrent() -> usize {
    5
}

fn default_autopilot_max_proposals() -> usize {
    20
}

fn default_autopilot_max_missions_agent() -> usize {
    10
}

fn default_autopilot_stale_threshold() -> u32 {
    30
}

// --- SOP (Standard Operating Procedure) configuration ---

/// Configuration for SOP execution.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SopConfig {
    /// Directory for SOP definitions.
    pub sops_dir: String,
    /// Default execution mode: "supervised" or "deterministic".
    pub default_execution_mode: String,
    /// Maximum concurrent SOP runs.
    pub max_concurrent_total: u32,
    /// Approval checkpoint timeout in seconds.
    pub approval_timeout_secs: u64,
    /// Maximum finished runs to retain.
    pub max_finished_runs: u32,
}

impl Default for SopConfig {
    fn default() -> Self {
        Self {
            sops_dir: "./sops".to_string(),
            default_execution_mode: "supervised".to_string(),
            max_concurrent_total: 4,
            approval_timeout_secs: 300,
            max_finished_runs: 100,
        }
    }
}

// --- A2A (Agent-to-Agent) protocol configuration ---

/// Configuration for external A2A agents that this instance can call.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct A2aConfig {
    /// Enable A2A protocol endpoints (/.well-known/agent.json and /a2a).
    pub enabled: bool,
    /// Optional bearer token for authenticating incoming A2A requests.
    pub bearer_token: Option<String>,
    /// External A2A agents to register as swarm participants.
    pub agents: HashMap<String, A2aAgentConfig>,
}

/// Configuration for a single external A2A agent.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct A2aAgentConfig {
    /// Base URL of the external agent (e.g., "https://agent.example.com").
    pub url: String,
    /// Optional bearer token for authentication.
    pub auth_token: Option<String>,
    /// Timeout in seconds for A2A calls (default: 120).
    pub timeout_secs: u64,
}

impl Default for A2aAgentConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            auth_token: None,
            timeout_secs: 120,
        }
    }
}

// --- Guardrails configuration ---

/// Configuration for LLM input/output guardrails.
///
/// Guards are enabled in `audit` mode by default so that violations are logged
/// even when the user hasn't explicitly configured guardrails.  Set `mode` to
/// `"off"` to disable, `"sanitize"` to redact, or `"block"` to reject.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GuardrailsConfig {
    /// Enforcement mode for PII redaction: "off", "audit", "sanitize", "block".
    pub pii_mode: String,
    /// Enforcement mode for prompt injection detection: "off", "audit", "sanitize", "block".
    pub injection_mode: String,
}

impl Default for GuardrailsConfig {
    fn default() -> Self {
        Self {
            pii_mode: "audit".to_string(),
            injection_mode: "audit".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_config_defaults() {
        let cfg = CostConfig::default();
        assert!(!cfg.enabled);
        assert!((cfg.daily_limit_usd - 10.0).abs() < f64::EPSILON);
        assert!((cfg.monthly_limit_usd - 100.0).abs() < f64::EPSILON);
        assert_eq!(cfg.warn_at_percent, 80);
        assert!(!cfg.allow_override);
    }

    #[test]
    fn cost_config_deserialize_from_toml() {
        let toml_str = r#"
            enabled = true
            daily_limit_usd = 25.0
            monthly_limit_usd = 250.0
            warn_at_percent = 90
            allow_override = true
        "#;
        let cfg: CostConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.enabled);
        assert!((cfg.daily_limit_usd - 25.0).abs() < f64::EPSILON);
        assert!((cfg.monthly_limit_usd - 250.0).abs() < f64::EPSILON);
        assert_eq!(cfg.warn_at_percent, 90);
        assert!(cfg.allow_override);
    }

    #[test]
    fn cost_config_partial_override() {
        let toml_str = r#"
            enabled = true
        "#;
        let cfg: CostConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.enabled);
        // Rest should be defaults
        assert!((cfg.daily_limit_usd - 10.0).abs() < f64::EPSILON);
        assert!((cfg.monthly_limit_usd - 100.0).abs() < f64::EPSILON);
        assert_eq!(cfg.warn_at_percent, 80);
        assert!(!cfg.allow_override);
    }

    // --- Production mode validation tests ---
    //
    // These tests manipulate the `AGENTZERO_ENV` env var, so they must run
    // sequentially.  We use a static mutex instead of `serial_test` to avoid
    // an extra dev-dependency.

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Run `body` while `AGENTZERO_ENV` is set to `value` (or removed if `None`).
    /// The env var is always cleaned up afterwards, even on panic.
    fn with_env(value: Option<&str>, body: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                std::env::remove_var("AGENTZERO_ENV");
            }
        }
        let _cleanup = Cleanup;
        match value {
            Some(v) => std::env::set_var("AGENTZERO_ENV", v),
            None => std::env::remove_var("AGENTZERO_ENV"),
        }
        body();
    }

    fn prod_config() -> AgentZeroConfig {
        AgentZeroConfig::default()
    }

    #[test]
    fn production_rejects_no_tls_and_no_allow_insecure() {
        with_env(Some("production"), || {
            let config = prod_config();
            let result = config.validate_production_mode();
            assert!(result.is_err());
            let err = result.expect_err("should fail");
            assert!(
                err.to_string().contains("TLS"),
                "expected TLS error, got: {err}"
            );
        });
    }

    #[test]
    fn production_allows_explicit_allow_insecure() {
        with_env(Some("production"), || {
            let mut config = prod_config();
            config.gateway.allow_insecure = true;
            let result = config.validate_production_mode();
            assert!(
                result.is_ok(),
                "allow_insecure should bypass TLS requirement"
            );
        });
    }

    #[test]
    fn production_rejects_no_pairing() {
        with_env(Some("production"), || {
            let mut config = prod_config();
            config.gateway.allow_insecure = true;
            config.gateway.require_pairing = false;
            let result = config.validate_production_mode();
            assert!(result.is_err());
            let err = result.expect_err("should fail");
            assert!(
                err.to_string().contains("require_pairing"),
                "expected pairing error, got: {err}"
            );
        });
    }

    #[test]
    fn dev_mode_permissive() {
        with_env(None, || {
            let config = prod_config();
            let result = config.validate_production_mode();
            assert!(
                result.is_ok(),
                "dev mode should not enforce production rules"
            );
        });
    }

    #[test]
    fn production_allows_tls_configured() {
        with_env(Some("production"), || {
            let mut config = prod_config();
            config.gateway.tls = Some(TlsConfig {
                cert_path: "/etc/ssl/cert.pem".to_string(),
                key_path: "/etc/ssl/key.pem".to_string(),
                client_ca_path: None,
            });
            let result = config.validate_production_mode();
            assert!(
                result.is_ok(),
                "TLS configured should satisfy production mode"
            );
        });
    }
}
