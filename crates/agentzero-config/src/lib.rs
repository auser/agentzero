//! Configuration loading and policy mapping for AgentZero.
//!
//! Loads `agentzero.toml` with dotenv overlay, validates all fields, and
//! maps the config model into security policies (`ToolSecurityPolicy`,
//! `AuditPolicy`) consumed by the runtime.

mod loader;
mod model;
mod policy;
#[cfg(feature = "yaml-policy")]
pub mod security_policy;
mod templates;
pub mod watcher;
pub mod writer;

pub use loader::{env_or_secret, load, load_env_var, read_docker_secret, update_auto_approve};
pub use model::{
    A2aAgentConfig, A2aConfig, AgentSettings, AgentZeroConfig, AudioConfig, AuditConfig,
    AutonomyConfig, AutopilotTriggerAction, AutopilotTriggerCondition, AutopilotTriggerConfig,
    BrowserConfig, ChannelsGlobalConfig, ComputerUseConfig, CostConfig, CredentialProfile,
    DelegateAgentConfig, DepthRuleConfig, EmbeddingRoute, EstopConfig, FanOutStepConfig,
    GatewayConfig, GuardrailsConfig, HookSettings, HttpRequestConfig, IdentityConfig, LanesConfig,
    McpConfig, McpServerEntry, McpServersFile, MemoryConfig, ModelProviderProfile, ModelRoute,
    MultimodalConfig, NodeControlConfig, ObservabilityConfig, OtpConfig, OutboundLeakGuardConfig,
    PerplexityFilterConfig, PipelineConfig, PipelineTriggerConfig, PluginConfig, ProviderConfig,
    ProviderOptionsConfig, QueryClassificationConfig, QueryClassificationRule, ReadFileConfig,
    ResearchConfig, RuntimeConfig, SecurityConfig, ShellConfig, SkillsConfig, SwarmAgentConfig,
    SwarmConfig, SwarmRouterConfig, SyscallAnomalyConfig, TlsConfig, UrlAccessConfig,
    WasmRuntimeConfig, WasmSecurityConfig, WebFetchConfig, WebSearchConfig, WebSocketConfig,
    WriteFileConfig,
};
pub use policy::{
    build_agent_capability_set, load_audit_policy, load_tool_security_policy, AuditPolicy,
};
pub use templates::{
    discover_shared_templates, discover_templates, list_template_sources,
    template_paths_for_workspace, template_search_dirs, ResolvedTemplate, TemplateFile,
    TemplateSet, MAIN_SESSION_TEMPLATES, SHARED_SESSION_TEMPLATES, TEMPLATE_LOAD_ORDER,
};

#[cfg(test)]
mod tests;
