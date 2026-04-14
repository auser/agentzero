//! Tool implementations for AgentZero.
//!
//! Contains all built-in tools: file I/O, shell, git, browser, web fetch,
//! search, cron, MCP, Composio, Pushover, hardware, WASM module management,
//! and more. Each tool implements the `Tool` trait from `agentzero-core`.
//!
//! Tools are organized into three tiers for embedded binary size reduction:
//! - **Core**: Always compiled — essential file I/O, shell, search, memory, delegation.
//! - **Extended**: Common but optional — web, git, cron, approval, IPC.
//! - **Full**: Everything else — browser, hardware, domain, WASM, etc.

use serde::{Deserialize, Serialize};

/// Tool tier classification for binary size optimization.
///
/// Tools are split into three tiers so that embedded/minimal builds can
/// exclude higher-tier tools and their transitive dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ToolTier {
    /// ~10 essential tools: file I/O, shell, search, memory, delegation.
    /// Always compiled regardless of feature flags.
    Core = 0,
    /// ~20 additional tools: web search, HTTP, git, cron, approval, IPC.
    /// Compiled with `tools-extended` or `tools-full` features.
    Extended = 1,
    /// Everything else: browser, hardware, domain, WASM, etc.
    /// Compiled with `tools-full` feature (the default).
    Full = 2,
}

impl ToolTier {
    /// Returns `true` if `self` is at or below the given maximum tier.
    pub fn is_within(self, max_tier: ToolTier) -> bool {
        (self as u8) <= (max_tier as u8)
    }
}

impl std::fmt::Display for ToolTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolTier::Core => write!(f, "core"),
            ToolTier::Extended => write!(f, "extended"),
            ToolTier::Full => write!(f, "full"),
        }
    }
}

/// Returns the maximum tier enabled by the current feature flags.
pub fn max_compiled_tier() -> ToolTier {
    if cfg!(feature = "tools-full") {
        ToolTier::Full
    } else if cfg!(feature = "tools-extended") {
        ToolTier::Extended
    } else {
        ToolTier::Core
    }
}

/// Classify a tool by name into its tier.
///
/// Core tools are the minimum set for a useful agent. Extended tools add
/// networking and automation. Full includes everything else.
pub fn tool_tier(name: &str) -> ToolTier {
    match name {
        // --- Core tier: essential file I/O, search, memory, delegation ---
        "read_file"
        | "write_file"
        | "file_edit"
        | "apply_patch"
        | "glob_search"
        | "content_search"
        | "shell"
        | "memory_store"
        | "memory_recall"
        | "memory_forget"
        | "delegate"
        | "sub_agent_spawn"
        | "sub_agent_list"
        | "sub_agent_manage"
        | "delegate_coordination_status"
        | "task_plan"
        | "process"
        | "image_info"
        | "pdf_read"
        | "screenshot"
        | "conversation_timerange"
        | "semantic_recall" => ToolTier::Core,

        // --- Extended tier: networking, git, cron, approval, IPC ---
        "web_search"
        | "http_request"
        | "web_fetch"
        | "url_validation"
        | "git_operations"
        | "cron_add"
        | "cron_list"
        | "cron_remove"
        | "cron_update"
        | "cron_pause"
        | "cron_resume"
        | "schedule"
        | "agents_ipc"
        | "code_interpreter"
        | "sop_list"
        | "sop_status"
        | "sop_advance"
        | "sop_approve"
        | "sop_execute"
        | "cli_discovery"
        | "discord_search"
        | "proxy_config"
        | "model_routing_config"
        | "docx_read"
        | "html_extract"
        | "a2a"
        | "chunk_document" => ToolTier::Extended,

        // --- Full tier: everything else ---
        _ => ToolTier::Full,
    }
}

// ── Core tier modules (always compiled) ──────────────────────────────
pub mod apply_patch;
pub mod autonomy;
pub mod checkpoint;
pub mod content_search;
pub mod conversation_timerange;
pub mod converse;
pub mod delegate;
pub mod delegate_coordination_status;
pub mod file_edit;
pub mod glob_search;
pub mod image_info;
pub mod memory_tools;
pub mod pdf_read;
pub mod process_tool;
pub mod read_file;
pub mod screenshot;
pub mod semantic_recall;
pub mod shell;
pub mod shell_parse;
pub mod subagent_tools;
pub mod task_manager;
pub mod task_plan;
pub mod write_file;

// ── Extended tier modules (tools-extended or tools-full) ─────────────
#[cfg(feature = "tools-extended")]
pub mod a2a;
#[cfg(feature = "tools-extended")]
pub mod agents_ipc;
#[cfg(feature = "tools-extended")]
pub mod cli_discovery;
#[cfg(feature = "tools-extended")]
pub mod code_interpreter;
#[cfg(feature = "tools-extended")]
pub mod cron_store;
#[cfg(feature = "tools-extended")]
pub mod cron_tools;
#[cfg(feature = "tools-extended")]
pub mod discord_search;
#[cfg(all(feature = "tools-extended", feature = "document-tools"))]
pub mod docx_read;
#[cfg(feature = "tools-extended")]
pub mod git_operations;
#[cfg(all(feature = "tools-extended", feature = "document-tools"))]
pub mod html_extract;
#[cfg(feature = "tools-extended")]
pub mod http_request;
#[cfg(feature = "tools-extended")]
pub mod model_routing_config;
#[cfg(feature = "tools-extended")]
pub mod proxy_config;
#[cfg(feature = "tools-extended")]
pub mod schedule;
#[cfg(feature = "tools-extended")]
pub mod sop;
#[cfg(feature = "tools-extended")]
pub mod sop_tools;
#[cfg(feature = "tools-extended")]
pub mod url_validation;
#[cfg(feature = "tools-extended")]
pub mod web_fetch;
#[cfg(feature = "tools-extended")]
pub mod web_search;

#[cfg(feature = "tools-extended")]
pub mod skills;

// ── Full tier modules (tools-full only) ──────────────────────────────
#[cfg(feature = "tools-full")]
pub mod browser;
#[cfg(feature = "tools-full")]
pub mod browser_open;
#[cfg(feature = "tools-full")]
pub mod domain;
#[cfg(feature = "tools-full")]
pub mod hardware;
#[cfg(feature = "tools-full")]
pub mod hardware_tools;
#[cfg(feature = "tools-full")]
pub mod pushover;
#[cfg(feature = "tools-full")]
pub mod wasm_tools;

// ── RAG modules (rag feature) ────────────────────────────────────────
#[cfg(feature = "rag")]
pub mod chunk_document;

use std::collections::HashMap;
use std::path::PathBuf;

// ── Core tier re-exports (always available) ──────────────────────────
pub use agentzero_core::common::url_policy::UrlAccessPolicy;
pub use apply_patch::ApplyPatchTool;
pub use content_search::ContentSearchTool;
pub use converse::ConverseTool;
pub use delegate::{DelegateTool, ToolBuilder};
pub use delegate_coordination_status::DelegateCoordinationStatusTool;
pub use file_edit::FileEditTool;
pub use glob_search::GlobSearchTool;
pub use image_info::ImageInfoTool;
pub use memory_tools::{MemoryForgetTool, MemoryRecallTool, MemoryStoreTool};
pub use pdf_read::PdfReadTool;
pub use process_tool::ProcessTool;
pub use read_file::{ReadFilePolicy, ReadFileTool};
pub use screenshot::ScreenshotTool;
pub use shell::{ShellPolicy, ShellTool};
pub use subagent_tools::{SubAgentListTool, SubAgentManageTool, SubAgentSpawnTool};
pub use task_manager::TaskManager;
pub use task_plan::TaskPlanTool;
pub use write_file::{WriteFilePolicy, WriteFileTool};

// ── Extended tier re-exports ─────────────────────────────────────────
#[cfg(feature = "tools-extended")]
pub use a2a::A2aTool;
#[cfg(feature = "tools-extended")]
pub use agents_ipc::AgentsIpcTool;
#[cfg(feature = "tools-extended")]
pub use cli_discovery::CliDiscoveryTool;
#[cfg(feature = "tools-extended")]
pub use code_interpreter::{CodeInterpreterConfig, CodeInterpreterTool};
#[cfg(feature = "tools-extended")]
pub use cron_tools::{
    CronAddTool, CronListTool, CronPauseTool, CronRemoveTool, CronResumeTool, CronUpdateTool,
};
#[cfg(feature = "tools-extended")]
pub use discord_search::DiscordSearchTool;
#[cfg(all(feature = "tools-extended", feature = "document-tools"))]
pub use docx_read::DocxReadTool;
#[cfg(feature = "tools-extended")]
pub use git_operations::GitOperationsTool;
#[cfg(all(feature = "tools-extended", feature = "document-tools"))]
pub use html_extract::HtmlExtractTool;
#[cfg(feature = "tools-extended")]
pub use http_request::HttpRequestTool;
#[cfg(feature = "tools-extended")]
pub use model_routing_config::ModelRoutingConfigTool;
#[cfg(feature = "tools-extended")]
pub use proxy_config::{ProxyConfigTool, ProxySettings};
#[cfg(feature = "tools-extended")]
pub use schedule::ScheduleTool;
#[cfg(feature = "tools-extended")]
pub use sop_tools::{SopAdvanceTool, SopApproveTool, SopExecuteTool, SopListTool, SopStatusTool};
#[cfg(feature = "tools-extended")]
pub use url_validation::UrlValidationTool;
#[cfg(feature = "tools-extended")]
pub use web_fetch::WebFetchTool;
#[cfg(feature = "tools-extended")]
pub use web_search::{WebSearchConfig, WebSearchTool};

// Stub `WebSearchConfig` when extended tier is not compiled, so
// `ToolSecurityPolicy` always compiles without conditional fields.
#[cfg(not(feature = "tools-extended"))]
#[derive(Debug, Clone)]
pub struct WebSearchConfig {
    pub provider: String,
    pub brave_api_key: Option<String>,
    pub jina_api_key: Option<String>,
    pub timeout_secs: u64,
    pub user_agent: String,
}

#[cfg(not(feature = "tools-extended"))]
impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            provider: "duckduckgo".to_string(),
            brave_api_key: None,
            jina_api_key: None,
            timeout_secs: 15,
            user_agent: "AgentZero/1.0".to_string(),
        }
    }
}

// ── Full tier re-exports ─────────────────────────────────────────────
#[cfg(feature = "tools-full")]
pub use browser::BrowserTool;
#[cfg(feature = "tools-full")]
pub use browser_open::BrowserOpenTool;
#[cfg(feature = "tools-full")]
pub use domain::{
    DomainCreateTool, DomainInfoTool, DomainLearnTool, DomainLessonsTool, DomainListTool,
    DomainSearchTool, DomainUpdateTool, DomainVerifyTool, DomainWorkflowTool,
};
#[cfg(feature = "tools-full")]
pub use hardware_tools::{HardwareBoardInfoTool, HardwareMemoryMapTool, HardwareMemoryReadTool};
#[cfg(feature = "tools-full")]
pub use pushover::PushoverTool;
#[cfg(feature = "tools-full")]
pub use wasm_tools::{WasmModuleTool, WasmToolExecTool};

/// MCP server definition for the tool security policy.
///
/// Kept in `agentzero-tools` to avoid a circular dependency with `agentzero-config`.
/// The config layer converts its own `McpServerEntry` into this type.
#[derive(Debug, Clone, Default)]
pub struct McpServerDef {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    /// Optional SHA-256 hash of the server binary for attestation.
    pub sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolSecurityPolicy {
    pub read_file: ReadFilePolicy,
    pub write_file: WriteFilePolicy,
    pub shell: ShellPolicy,
    pub url_access: UrlAccessPolicy,
    pub enable_write_file: bool,
    pub enable_mcp: bool,
    pub allowed_mcp_servers: Vec<String>,
    pub mcp_servers: HashMap<String, McpServerDef>,
    pub enable_git: bool,
    pub enable_cron: bool,
    pub enable_web_search: bool,
    pub web_search_config: WebSearchConfig,
    pub enable_browser: bool,
    pub enable_browser_open: bool,
    pub enable_http_request: bool,
    pub enable_web_fetch: bool,
    pub enable_url_validation: bool,
    pub enable_agents_ipc: bool,
    pub enable_html_extract: bool,
    pub enable_pushover: bool,
    pub enable_code_interpreter: bool,
    pub enable_autopilot: bool,
    pub enable_agent_manage: bool,
    pub enable_domain_tools: bool,
    pub enable_self_config: bool,
    pub enable_wasm_plugins: bool,
    pub wasm_global_plugin_dir: Option<PathBuf>,
    pub wasm_project_plugin_dir: Option<PathBuf>,
    pub wasm_dev_plugin_dir: Option<PathBuf>,
    /// Enable A2A (Agent-to-Agent) protocol tool for dynamic agent discovery and messaging.
    pub enable_a2a_tool: bool,
    /// Enable dynamic tool creation at runtime (tool_create tool).
    pub enable_dynamic_tools: bool,
}

impl ToolSecurityPolicy {
    pub fn default_for_workspace(workspace_root: PathBuf) -> Self {
        Self {
            read_file: ReadFilePolicy::default_for_root(workspace_root.clone()),
            write_file: WriteFilePolicy::default_for_root(workspace_root),
            shell: ShellPolicy::default_with_commands(vec![
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
            ]),
            url_access: UrlAccessPolicy::default(),
            enable_write_file: false,
            enable_mcp: false,
            allowed_mcp_servers: vec![],
            mcp_servers: HashMap::new(),
            enable_git: false,
            enable_cron: false,
            enable_web_search: false,
            web_search_config: WebSearchConfig::default(),
            enable_browser: false,
            enable_browser_open: false,
            enable_http_request: false,
            enable_web_fetch: false,
            enable_url_validation: false,
            enable_agents_ipc: true,
            enable_html_extract: false,
            enable_pushover: false,
            enable_code_interpreter: false,
            enable_autopilot: false,
            enable_agent_manage: false,
            enable_domain_tools: false,
            enable_self_config: false,
            enable_wasm_plugins: false,
            wasm_global_plugin_dir: None,
            wasm_project_plugin_dir: None,
            wasm_dev_plugin_dir: None,
            enable_a2a_tool: false,
            enable_dynamic_tools: false,
        }
    }
}

/// All known core-tier tool names.
pub const CORE_TOOL_NAMES: &[&str] = &[
    "read_file",
    "write_file",
    "file_edit",
    "apply_patch",
    "glob_search",
    "content_search",
    "shell",
    "memory_store",
    "memory_recall",
    "memory_forget",
    "delegate",
    "sub_agent_spawn",
    "sub_agent_list",
    "sub_agent_manage",
    "delegate_coordination_status",
    "task_plan",
    "process",
    "image_info",
    "pdf_read",
    "screenshot",
    "conversation_timerange",
    "semantic_recall",
];

/// All known extended-tier tool names (not including core).
pub const EXTENDED_TOOL_NAMES: &[&str] = &[
    "web_search",
    "http_request",
    "web_fetch",
    "url_validation",
    "git_operations",
    "cron_add",
    "cron_list",
    "cron_remove",
    "cron_update",
    "cron_pause",
    "cron_resume",
    "schedule",
    "agents_ipc",
    "code_interpreter",
    "sop_list",
    "sop_status",
    "sop_advance",
    "sop_approve",
    "sop_execute",
    "cli_discovery",
    "discord_search",
    "proxy_config",
    "model_routing_config",
    "docx_read",
    "html_extract",
    "a2a",
];

/// All known full-tier tool names (not including core or extended).
pub const FULL_TOOL_NAMES: &[&str] = &[
    "browser",
    "browser_open",
    "pushover",
    "hardware_board_info",
    "hardware_memory_map",
    "hardware_memory_read",
    "domain_create",
    "domain_info",
    "domain_learn",
    "domain_lessons",
    "domain_list",
    "domain_search",
    "domain_update",
    "domain_verify",
    "domain_workflow",
    "wasm_module",
    "wasm_tool_exec",
    "agent_manage",
    "config_manage",
    "skill_manage",
    "plugin_scaffold",
];

/// Returns the list of tool names for a specific tier (not cumulative).
pub fn tools_in_tier(tier: ToolTier) -> &'static [&'static str] {
    match tier {
        ToolTier::Core => CORE_TOOL_NAMES,
        ToolTier::Extended => EXTENDED_TOOL_NAMES,
        ToolTier::Full => FULL_TOOL_NAMES,
    }
}

/// Returns the total count of tools available at the given tier (cumulative).
pub fn tool_count_at_tier(tier: ToolTier) -> usize {
    let mut count = CORE_TOOL_NAMES.len();
    if tier >= ToolTier::Extended {
        count += EXTENDED_TOOL_NAMES.len();
    }
    if tier >= ToolTier::Full {
        count += FULL_TOOL_NAMES.len();
    }
    count
}

/// Filter a list of tool names by tier, keeping only those at or below `max_tier`.
pub fn filter_tools_by_tier(tool_names: &[&str], max_tier: ToolTier) -> Vec<String> {
    tool_names
        .iter()
        .filter(|name| tool_tier(name).is_within(max_tier))
        .map(|name| (*name).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_tier_classifies_core_tools() {
        let core_tools = [
            "read_file",
            "write_file",
            "file_edit",
            "apply_patch",
            "glob_search",
            "content_search",
            "shell",
            "memory_store",
            "memory_recall",
            "memory_forget",
            "delegate",
            "sub_agent_spawn",
            "sub_agent_list",
            "sub_agent_manage",
            "task_plan",
            "process",
            "image_info",
            "pdf_read",
            "screenshot",
            "conversation_timerange",
            "semantic_recall",
        ];
        for name in &core_tools {
            assert_eq!(
                tool_tier(name),
                ToolTier::Core,
                "{name} should be Core tier"
            );
        }
    }

    #[test]
    fn tool_tier_classifies_extended_tools() {
        let extended_tools = [
            "web_search",
            "http_request",
            "web_fetch",
            "url_validation",
            "git_operations",
            "cron_add",
            "cron_list",
            "cron_remove",
            "cron_update",
            "cron_pause",
            "cron_resume",
            "schedule",
            "agents_ipc",
            "code_interpreter",
            "sop_list",
            "sop_status",
            "sop_advance",
            "sop_approve",
            "sop_execute",
            "cli_discovery",
            "discord_search",
            "proxy_config",
            "model_routing_config",
            "a2a",
        ];
        for name in &extended_tools {
            assert_eq!(
                tool_tier(name),
                ToolTier::Extended,
                "{name} should be Extended tier"
            );
        }
    }

    #[test]
    fn tool_tier_classifies_full_tools() {
        let full_tools = [
            "browser",
            "browser_open",
            "composio",
            "pushover",
            "domain_create",
            "hardware_board_info",
            "image_gen",
            "tts",
            "video_gen",
            "wasm_module",
            "wasm_tool_exec",
            "agent_manage",
        ];
        for name in &full_tools {
            assert_eq!(
                tool_tier(name),
                ToolTier::Full,
                "{name} should be Full tier"
            );
        }
    }

    #[test]
    fn core_tier_includes_file_ops_and_shell() {
        let names = ["read_file", "write_file", "shell", "web_search", "browser"];
        let filtered = filter_tools_by_tier(&names, ToolTier::Core);
        assert!(filtered.contains(&"read_file".to_string()));
        assert!(filtered.contains(&"write_file".to_string()));
        assert!(filtered.contains(&"shell".to_string()));
        assert!(!filtered.contains(&"web_search".to_string()));
        assert!(!filtered.contains(&"browser".to_string()));
    }

    #[test]
    fn extended_tier_includes_web_tools() {
        let names = [
            "read_file",
            "shell",
            "web_search",
            "http_request",
            "browser",
            "composio",
        ];
        let filtered = filter_tools_by_tier(&names, ToolTier::Extended);
        assert!(filtered.contains(&"read_file".to_string()));
        assert!(filtered.contains(&"shell".to_string()));
        assert!(filtered.contains(&"web_search".to_string()));
        assert!(filtered.contains(&"http_request".to_string()));
        assert!(!filtered.contains(&"browser".to_string()));
        assert!(!filtered.contains(&"composio".to_string()));
    }

    #[test]
    fn full_tier_includes_everything() {
        let names = [
            "read_file",
            "shell",
            "web_search",
            "http_request",
            "browser",
            "composio",
            "hardware_board_info",
        ];
        let filtered = filter_tools_by_tier(&names, ToolTier::Full);
        assert_eq!(
            filtered.len(),
            names.len(),
            "Full tier should include all tools"
        );
    }

    #[test]
    fn tier_filtering_removes_non_matching_tools() {
        let names = [
            "read_file",
            "glob_search",
            "shell",
            "web_search",
            "git_operations",
            "browser",
            "composio",
            "pushover",
        ];
        let core_only = filter_tools_by_tier(&names, ToolTier::Core);
        assert_eq!(core_only.len(), 3, "Only 3 core tools should pass");

        let extended = filter_tools_by_tier(&names, ToolTier::Extended);
        assert_eq!(extended.len(), 5, "5 core+extended tools should pass");

        let full = filter_tools_by_tier(&names, ToolTier::Full);
        assert_eq!(full.len(), 8, "All 8 tools should pass for full tier");
    }

    #[test]
    fn tier_ordering_is_correct() {
        assert!(ToolTier::Core < ToolTier::Extended);
        assert!(ToolTier::Extended < ToolTier::Full);
        assert!(ToolTier::Core.is_within(ToolTier::Full));
        assert!(ToolTier::Extended.is_within(ToolTier::Full));
        assert!(!ToolTier::Full.is_within(ToolTier::Core));
        assert!(!ToolTier::Extended.is_within(ToolTier::Core));
    }

    #[test]
    fn max_compiled_tier_reflects_features() {
        let tier = max_compiled_tier();
        assert!(tier.is_within(ToolTier::Full));
    }

    #[test]
    fn tool_count_increases_with_tier() {
        let core = tool_count_at_tier(ToolTier::Core);
        let extended = tool_count_at_tier(ToolTier::Extended);
        let full = tool_count_at_tier(ToolTier::Full);
        assert!(core < extended);
        assert!(extended < full);
    }

    #[test]
    fn tools_in_tier_returns_correct_lists() {
        assert_eq!(tools_in_tier(ToolTier::Core), CORE_TOOL_NAMES);
        assert_eq!(tools_in_tier(ToolTier::Extended), EXTENDED_TOOL_NAMES);
        assert_eq!(tools_in_tier(ToolTier::Full), FULL_TOOL_NAMES);
    }

    #[test]
    fn tier_name_constants_are_consistent_with_tool_tier() {
        for name in CORE_TOOL_NAMES {
            assert_eq!(tool_tier(name), ToolTier::Core, "{name} should be Core");
        }
        for name in EXTENDED_TOOL_NAMES {
            assert_eq!(
                tool_tier(name),
                ToolTier::Extended,
                "{name} should be Extended"
            );
        }
        for name in FULL_TOOL_NAMES {
            assert_eq!(tool_tier(name), ToolTier::Full, "{name} should be Full");
        }
    }
}
