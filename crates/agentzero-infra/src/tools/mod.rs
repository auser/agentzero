mod mcp;
mod plugin;

use agentzero_core::Tool;
use agentzero_delegation::DelegateConfig;
use agentzero_routing::ModelRouter;
use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;

pub use agentzero_tools::{
    DelegateTool, ModelRoutingConfigTool, ReadFilePolicy, ReadFileTool, ShellPolicy, ShellTool,
    ToolSecurityPolicy, WriteFilePolicy, WriteFileTool,
};
pub use mcp::McpTool;
pub use plugin::ProcessPluginTool;

pub fn default_tools(
    policy: &ToolSecurityPolicy,
    router: Option<ModelRouter>,
    delegate_agents: Option<HashMap<String, DelegateConfig>>,
) -> anyhow::Result<Vec<Box<dyn Tool>>> {
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(ReadFileTool::new(policy.read_file.clone())),
        Box::new(ShellTool::new(policy.shell.clone())),
    ];

    if policy.enable_write_file {
        tools.push(Box::new(WriteFileTool::new(policy.write_file.clone())));
    }

    if policy.enable_mcp {
        let mcp_tool = McpTool::from_env(&policy.allowed_mcp_servers)?;
        tools.push(Box::new(mcp_tool));
    }

    if policy.enable_process_plugin {
        let plugin_tool = optional_process_plugin_tool_from_env()?.ok_or_else(|| {
            anyhow::anyhow!("plugin tool enabled but AGENTZERO_PLUGIN_TOOL is missing")
        })?;
        tools.push(Box::new(plugin_tool));
    }

    if let Some(r) = router {
        tools.push(Box::new(ModelRoutingConfigTool::new(r)));
    }

    if let Some(agents) = delegate_agents {
        if !agents.is_empty() {
            tools.push(Box::new(DelegateTool::new(agents, 0)));
        }
    }

    Ok(tools)
}

#[derive(Debug, Deserialize)]
struct PluginToolEnvConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

fn optional_process_plugin_tool_from_env() -> anyhow::Result<Option<ProcessPluginTool>> {
    let raw = match std::env::var("AGENTZERO_PLUGIN_TOOL") {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };

    let parsed: PluginToolEnvConfig =
        serde_json::from_str(&raw).context("AGENTZERO_PLUGIN_TOOL must be valid JSON")?;
    let tool = ProcessPluginTool::new("plugin_exec", parsed.command, parsed.args)?;
    Ok(Some(tool))
}

#[cfg(test)]
mod tests {
    use super::{default_tools, optional_process_plugin_tool_from_env, ToolSecurityPolicy};
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn optional_plugin_tool_parses_valid_env() {
        let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
        std::env::set_var("AGENTZERO_PLUGIN_TOOL", r#"{"command":"cat","args":[]}"#);
        let result = optional_process_plugin_tool_from_env().expect("valid plugin env should load");
        assert!(result.is_some());
        std::env::remove_var("AGENTZERO_PLUGIN_TOOL");
    }

    #[test]
    fn optional_plugin_tool_rejects_invalid_json() {
        let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
        std::env::set_var("AGENTZERO_PLUGIN_TOOL", r#"{"command":}"#);
        let result = optional_process_plugin_tool_from_env();
        assert!(result.is_err());
        std::env::remove_var("AGENTZERO_PLUGIN_TOOL");
    }

    #[test]
    fn default_tools_fail_when_plugin_is_enabled_without_env() {
        let _guard = ENV_LOCK.lock().expect("env lock should be acquirable");
        std::env::remove_var("AGENTZERO_PLUGIN_TOOL");
        let mut policy = ToolSecurityPolicy::default_for_workspace(
            std::env::current_dir().expect("cwd should be readable"),
        );
        policy.enable_process_plugin = true;

        let result = default_tools(&policy, None, None);
        assert!(result.is_err());
        let err = result.err().expect("missing plugin env should fail closed");
        assert!(err.to_string().contains("AGENTZERO_PLUGIN_TOOL"));
    }

    #[test]
    fn default_tools_do_not_include_write_file_when_disabled() {
        let policy = ToolSecurityPolicy::default_for_workspace(
            std::env::current_dir().expect("cwd should be readable"),
        );
        let tools = default_tools(&policy, None, None).expect("default tools should build");
        let names = tools
            .into_iter()
            .map(|tool| tool.name())
            .collect::<Vec<_>>();
        assert!(!names.contains(&"write_file"));
    }
}
