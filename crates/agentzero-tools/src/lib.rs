//! Tool implementations for AgentZero.
//!
//! Contains all built-in tools: file I/O, shell, git, browser, web fetch,
//! search, cron, MCP, Composio, Pushover, hardware, WASM module management,
//! and more. Each tool implements the `Tool` trait from `agentzero-core`.

pub mod agents_ipc;
pub mod apply_patch;
pub mod autonomy;
pub mod browser;
pub mod browser_open;
pub mod cli_discovery;
pub mod composio;
pub mod content_search;
pub mod cron_store;
pub mod cron_tools;
pub mod delegate;
pub mod delegate_coordination_status;
pub mod docx_read;
pub mod file_edit;
pub mod git_operations;
pub mod glob_search;
pub mod hardware;
pub mod hardware_tools;
pub mod html_extract;
pub mod http_request;
pub mod image_info;
pub mod memory_tools;
pub mod model_routing_config;
pub mod pdf_read;
pub mod process_tool;
pub mod proxy_config;
pub mod pushover;
pub mod read_file;
pub mod schedule;
pub mod screenshot;
pub mod shell;
pub mod shell_parse;
pub mod skills;
pub mod sop_tools;
pub mod subagent_tools;
pub mod task_plan;
pub mod url_validation;
pub mod wasm_tools;
pub mod web_fetch;
pub mod web_search;
pub mod write_file;

use std::path::PathBuf;

pub use agents_ipc::AgentsIpcTool;
pub use agentzero_core::common::url_policy::UrlAccessPolicy;
pub use apply_patch::ApplyPatchTool;
pub use browser::BrowserTool;
pub use browser_open::BrowserOpenTool;
pub use cli_discovery::CliDiscoveryTool;
pub use composio::ComposioTool;
pub use content_search::ContentSearchTool;
pub use cron_tools::{
    CronAddTool, CronListTool, CronPauseTool, CronRemoveTool, CronResumeTool, CronUpdateTool,
};
pub use delegate::{DelegateTool, ToolBuilder};
pub use delegate_coordination_status::DelegateCoordinationStatusTool;
pub use docx_read::DocxReadTool;
pub use file_edit::FileEditTool;
pub use git_operations::GitOperationsTool;
pub use glob_search::GlobSearchTool;
pub use hardware_tools::{HardwareBoardInfoTool, HardwareMemoryMapTool, HardwareMemoryReadTool};
pub use html_extract::HtmlExtractTool;
pub use http_request::HttpRequestTool;
pub use image_info::ImageInfoTool;
pub use memory_tools::{MemoryForgetTool, MemoryRecallTool, MemoryStoreTool};
pub use model_routing_config::ModelRoutingConfigTool;
pub use pdf_read::PdfReadTool;
pub use process_tool::ProcessTool;
pub use proxy_config::ProxyConfigTool;
pub use pushover::PushoverTool;
pub use read_file::{ReadFilePolicy, ReadFileTool};
pub use schedule::ScheduleTool;
pub use screenshot::ScreenshotTool;
pub use shell::{ShellPolicy, ShellTool};
pub use sop_tools::{SopAdvanceTool, SopApproveTool, SopExecuteTool, SopListTool, SopStatusTool};
pub use subagent_tools::{SubAgentListTool, SubAgentManageTool, SubAgentSpawnTool};
pub use task_plan::TaskPlanTool;
pub use url_validation::UrlValidationTool;
pub use wasm_tools::{WasmModuleTool, WasmToolExecTool};
pub use web_fetch::WebFetchTool;
pub use web_search::WebSearchTool;
pub use write_file::{WriteFilePolicy, WriteFileTool};

#[derive(Debug, Clone)]
pub struct ToolSecurityPolicy {
    pub read_file: ReadFilePolicy,
    pub write_file: WriteFilePolicy,
    pub shell: ShellPolicy,
    pub url_access: UrlAccessPolicy,
    pub enable_write_file: bool,
    pub enable_mcp: bool,
    pub allowed_mcp_servers: Vec<String>,
    pub enable_process_plugin: bool,
    pub enable_git: bool,
    pub enable_cron: bool,
    pub enable_web_search: bool,
    pub enable_browser: bool,
    pub enable_browser_open: bool,
    pub enable_http_request: bool,
    pub enable_web_fetch: bool,
    pub enable_url_validation: bool,
    pub enable_agents_ipc: bool,
    pub enable_html_extract: bool,
    pub enable_composio: bool,
    pub enable_pushover: bool,
    pub enable_wasm_plugins: bool,
    pub wasm_global_plugin_dir: Option<PathBuf>,
    pub wasm_project_plugin_dir: Option<PathBuf>,
    pub wasm_dev_plugin_dir: Option<PathBuf>,
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
            enable_process_plugin: false,
            enable_git: false,
            enable_cron: false,
            enable_web_search: false,
            enable_browser: false,
            enable_browser_open: false,
            enable_http_request: false,
            enable_web_fetch: false,
            enable_url_validation: false,
            enable_agents_ipc: true,
            enable_html_extract: false,
            enable_composio: false,
            enable_pushover: false,
            enable_wasm_plugins: false,
            wasm_global_plugin_dir: None,
            wasm_project_plugin_dir: None,
            wasm_dev_plugin_dir: None,
        }
    }
}
