use agentzero_common::local_providers::is_local_provider;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    pub cost: CostConfig,
    pub identity: IdentityConfig,
    pub multimodal: MultimodalConfig,
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
}

impl AgentZeroConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.provider.kind.trim().is_empty() {
            return Err(anyhow!("provider.kind must not be empty"));
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

        if self.security.mcp.enabled && self.security.mcp.allowed_servers.is_empty() {
            return Err(anyhow!(
                "security.mcp.allowed_servers must not be empty when MCP is enabled"
            ));
        }

        if self.security.audit.enabled && self.security.audit.path.trim().is_empty() {
            return Err(anyhow!(
                "security.audit.path must not be empty when audit is enabled"
            ));
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
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub backend: String,
    #[serde(alias = "path")]
    pub sqlite_path: String,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            backend: "sqlite".to_string(),
            sqlite_path: default_sqlite_path(),
        }
    }
}

fn default_sqlite_path() -> String {
    agentzero_common::paths::default_sqlite_path()
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
    pub allowed_servers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct PluginConfig {
    /// Enable the process-based plugin tool (legacy).
    pub enabled: bool,
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
            format: "openclaw".to_string(),
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
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 42617,
            require_pairing: true,
            allow_public_bind: false,
            node_control: NodeControlConfig::default(),
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
}

impl Default for OutboundLeakGuardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            action: "redact".to_string(),
            sensitivity: 0.7,
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
