//! Configuration loading and policy mapping for AgentZero.
//!
//! Loads `agentzero.toml` with dotenv overlay, validates all fields, and
//! maps the config model into security policies (`ToolSecurityPolicy`,
//! `AuditPolicy`) consumed by the runtime.

mod loader;
mod model;
mod policy;
mod templates;
pub mod watcher;

pub use loader::{load, load_env_var, update_auto_approve};
pub use model::{
    AgentSettings, AgentZeroConfig, AuditConfig, AutonomyConfig, BrowserConfig,
    ChannelsGlobalConfig, ComposioConfig, ComputerUseConfig, CostConfig, CredentialProfile,
    DelegateAgentConfig, EmbeddingRoute, EstopConfig, GatewayConfig, HookSettings,
    HttpRequestConfig, IdentityConfig, McpConfig, MemoryConfig, ModelProviderProfile, ModelRoute,
    MultimodalConfig, NodeControlConfig, ObservabilityConfig, OtpConfig, OutboundLeakGuardConfig,
    PerplexityFilterConfig, PluginConfig, ProviderConfig, ProviderOptionsConfig,
    QueryClassificationConfig, QueryClassificationRule, ReadFileConfig, ResearchConfig,
    RuntimeConfig, SecurityConfig, ShellConfig, SkillsConfig, SyscallAnomalyConfig,
    UrlAccessConfig, WasmRuntimeConfig, WasmSecurityConfig, WebFetchConfig, WebSearchConfig,
    WriteFileConfig,
};
pub use policy::{load_audit_policy, load_tool_security_policy, AuditPolicy};
pub use templates::{
    discover_shared_templates, discover_templates, list_template_sources,
    template_paths_for_workspace, template_search_dirs, ResolvedTemplate, TemplateFile,
    TemplateSet, MAIN_SESSION_TEMPLATES, SHARED_SESSION_TEMPLATES, TEMPLATE_LOAD_ORDER,
};

#[cfg(test)]
mod tests;
