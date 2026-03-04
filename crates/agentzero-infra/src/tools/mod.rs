mod mcp;
mod plugin;
#[cfg(feature = "wasm-plugins")]
mod wasm_bridge;

use agentzero_core::Tool;
use agentzero_delegation::DelegateConfig;
use agentzero_routing::ModelRouter;
use agentzero_tools::ToolBuilder;
use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

pub use agentzero_tools::{
    ApplyPatchTool, BrowserOpenTool, BrowserTool, CliDiscoveryTool, ComposioTool,
    ContentSearchTool, CronAddTool, CronListTool, CronPauseTool, CronRemoveTool, CronResumeTool,
    CronUpdateTool, DelegateCoordinationStatusTool, DelegateTool, DocxReadTool, FileEditTool,
    GitOperationsTool, GlobSearchTool, HardwareBoardInfoTool, HardwareMemoryMapTool,
    HardwareMemoryReadTool, ImageInfoTool, MemoryForgetTool, MemoryRecallTool, MemoryStoreTool,
    ModelRoutingConfigTool, PdfReadTool, ProcessTool, ProxyConfigTool, PushoverTool,
    ReadFilePolicy, ReadFileTool, ScheduleTool, ScreenshotTool, ShellPolicy, ShellTool,
    SopAdvanceTool, SopApproveTool, SopExecuteTool, SopListTool, SopStatusTool, SubAgentListTool,
    SubAgentManageTool, SubAgentSpawnTool, TaskPlanTool, ToolSecurityPolicy, WasmModuleTool,
    WasmToolExecTool, WebSearchTool, WriteFilePolicy, WriteFileTool,
};
pub use mcp::McpTool;
pub use plugin::ProcessPluginTool;
#[cfg(feature = "wasm-plugins")]
pub use wasm_bridge::WasmTool;

pub fn default_tools(
    policy: &ToolSecurityPolicy,
    router: Option<ModelRouter>,
    delegate_agents: Option<HashMap<String, DelegateConfig>>,
) -> anyhow::Result<Vec<Box<dyn Tool>>> {
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(ReadFileTool::new(policy.read_file.clone())),
        Box::new(ShellTool::new(policy.shell.clone())),
        Box::new(GlobSearchTool),
        Box::new(ContentSearchTool),
        Box::new(MemoryStoreTool),
        Box::new(MemoryRecallTool),
        Box::new(MemoryForgetTool),
        Box::new(ImageInfoTool),
        Box::new(DocxReadTool),
        Box::new(PdfReadTool),
        Box::new(ScreenshotTool),
        Box::new(TaskPlanTool::default()),
        Box::new(ProcessTool::default()),
        Box::new(SubAgentSpawnTool::default()),
        Box::new(SubAgentListTool),
        Box::new(SubAgentManageTool),
        Box::new(CliDiscoveryTool),
        Box::new(ProxyConfigTool),
        Box::new(DelegateCoordinationStatusTool),
        Box::new(SopListTool),
        Box::new(SopStatusTool),
        Box::new(SopAdvanceTool),
        Box::new(SopApproveTool),
        Box::new(SopExecuteTool),
        Box::new(HardwareBoardInfoTool),
        Box::new(HardwareMemoryMapTool),
        Box::new(HardwareMemoryReadTool),
        Box::new(WasmModuleTool),
        Box::new(WasmToolExecTool),
    ];

    if policy.enable_write_file {
        tools.push(Box::new(WriteFileTool::new(policy.write_file.clone())));
        tools.push(Box::new(ApplyPatchTool));
        tools.push(Box::new(FileEditTool::new(
            policy.write_file.allowed_root.clone(),
            policy.write_file.max_write_bytes,
        )));
    }

    if policy.enable_git {
        tools.push(Box::new(GitOperationsTool::new()));
    }

    if policy.enable_cron {
        tools.push(Box::new(CronAddTool));
        tools.push(Box::new(CronListTool));
        tools.push(Box::new(CronRemoveTool));
        tools.push(Box::new(CronUpdateTool));
        tools.push(Box::new(CronPauseTool));
        tools.push(Box::new(CronResumeTool));
        tools.push(Box::new(ScheduleTool));
    }

    if policy.enable_web_search {
        tools.push(Box::new(WebSearchTool::default()));
    }

    if policy.enable_browser {
        tools.push(Box::new(BrowserTool::default()));
    }

    if policy.enable_browser_open {
        tools.push(Box::new(BrowserOpenTool::default()));
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

    if policy.enable_composio {
        tools.push(Box::new(ComposioTool));
    }

    if policy.enable_pushover {
        tools.push(Box::new(PushoverTool));
    }

    #[cfg(feature = "wasm-plugins")]
    if policy.enable_wasm_plugins {
        use agentzero_plugins::package::discover_plugins;
        use agentzero_plugins::wasm::WasmIsolationPolicy;

        let discovered = discover_plugins(
            policy.wasm_global_plugin_dir.as_deref(),
            policy.wasm_project_plugin_dir.as_deref(),
            policy.wasm_dev_plugin_dir.as_deref(),
        );
        let isolation = WasmIsolationPolicy::default();
        for plugin in discovered {
            match WasmTool::from_manifest(
                plugin.manifest.clone(),
                plugin.wasm_path.clone(),
                isolation.clone(),
            ) {
                Ok(tool) => tools.push(Box::new(tool)),
                Err(e) => {
                    tracing::warn!("skipping wasm plugin {}: {e}", plugin.manifest.id);
                }
            }
        }
    }

    if let Some(r) = router {
        tools.push(Box::new(ModelRoutingConfigTool::new(r)));
    }

    if let Some(agents) = delegate_agents {
        if !agents.is_empty() {
            let policy_for_builder = policy.clone();
            let builder: ToolBuilder =
                Arc::new(move || default_tools(&policy_for_builder, None, None));
            tools.push(Box::new(DelegateTool::new(agents, 0, builder)));
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
