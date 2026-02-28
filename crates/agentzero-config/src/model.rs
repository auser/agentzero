use anyhow::anyhow;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AgentZeroConfig {
    pub provider: ProviderConfig,
    pub memory: MemoryConfig,
    pub agent: AgentSettings,
    pub security: SecurityConfig,
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
        if self.provider.model.trim().is_empty() {
            return Err(anyhow!("provider.model must not be empty"));
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    #[serde(alias = "name")]
    pub kind: String,
    pub base_url: String,
    pub model: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: "openai".to_string(),
            base_url: "https://api.openai.com".to_string(),
            model: "gpt-4o-mini".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub max_tool_iterations: usize,
    pub request_timeout_ms: u64,
    pub memory_window_size: usize,
    pub max_prompt_chars: usize,
    pub mode: String,
    pub hooks: HookSettings,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_tool_iterations: 4,
            request_timeout_ms: 30_000,
            memory_window_size: 8,
            max_prompt_chars: 8_000,
            mode: "development".to_string(),
            hooks: HookSettings::default(),
        }
    }
}

impl AgentSettings {
    pub fn is_dev_mode(&self) -> bool {
        matches!(self.mode.trim(), "dev" | "development")
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HookSettings {
    pub enabled: bool,
    pub timeout_ms: u64,
    pub fail_closed: bool,
}

impl Default for HookSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_ms: 250,
            fail_closed: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
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
            ],
            read_file: ReadFileConfig::default(),
            write_file: WriteFileConfig::default(),
            shell: ShellConfig::default(),
            mcp: McpConfig::default(),
            plugin: PluginConfig::default(),
            audit: AuditConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ReadFileConfig {
    pub max_read_bytes: u64,
    pub allow_binary: bool,
}

impl Default for ReadFileConfig {
    fn default() -> Self {
        Self {
            max_read_bytes: 64 * 1024,
            allow_binary: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    pub max_args: usize,
    pub max_arg_length: usize,
    pub max_output_bytes: usize,
    pub forbidden_chars: String,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            max_args: 8,
            max_arg_length: 128,
            max_output_bytes: 8192,
            forbidden_chars: ";&|><$`\n\r".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct McpConfig {
    pub enabled: bool,
    pub allowed_servers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PluginConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
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
