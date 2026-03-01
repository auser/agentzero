pub mod agents_ipc;
pub mod apply_patch;
pub mod http_request;
pub mod read_file;
pub mod shell;
pub mod shell_parse;
pub mod url_validation;
pub mod web_fetch;
pub mod write_file;

use std::path::PathBuf;

pub use agents_ipc::AgentsIpcTool;
pub use agentzero_common::url_policy::UrlAccessPolicy;
pub use apply_patch::ApplyPatchTool;
pub use http_request::HttpRequestTool;
pub use read_file::{ReadFilePolicy, ReadFileTool};
pub use shell::{ShellPolicy, ShellTool};
pub use url_validation::UrlValidationTool;
pub use web_fetch::WebFetchTool;
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
            ]),
            url_access: UrlAccessPolicy::default(),
            enable_write_file: false,
            enable_mcp: false,
            allowed_mcp_servers: vec![],
            enable_process_plugin: false,
        }
    }
}
