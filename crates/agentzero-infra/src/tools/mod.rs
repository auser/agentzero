mod agent_manage;
mod config_manage;
mod mcp;
mod plugin_scaffold;
mod skill_manage;
#[cfg(feature = "wasm-plugins")]
mod wasm_bridge;

use agentzero_core::agent_store::AgentStoreApi;
use agentzero_core::delegation::DelegateConfig;
use agentzero_core::routing::ModelRouter;
use agentzero_core::{DepthPolicy, Tool};
use agentzero_tools::ToolBuilder;
use std::collections::HashMap;
use std::sync::Arc;

pub use agent_manage::AgentManageTool;
pub use config_manage::ConfigManageTool;
pub use plugin_scaffold::PluginScaffoldTool;
pub use skill_manage::SkillManageTool;

pub use agentzero_tools::{
    AgentsIpcTool, ApplyPatchTool, BrowserOpenTool, BrowserTool, CliDiscoveryTool,
    CodeInterpreterTool, ComposioTool, ContentSearchTool, CronAddTool, CronListTool, CronPauseTool,
    CronRemoveTool, CronResumeTool, CronUpdateTool, DelegateCoordinationStatusTool, DelegateTool,
    DomainCreateTool, DomainInfoTool, DomainLearnTool, DomainLessonsTool, DomainListTool,
    DomainSearchTool, DomainUpdateTool, DomainVerifyTool, DomainWorkflowTool, FileEditTool,
    GitOperationsTool, GlobSearchTool, HardwareBoardInfoTool, HardwareMemoryMapTool,
    HardwareMemoryReadTool, HttpRequestTool, ImageGenTool, ImageInfoTool, MemoryForgetTool,
    MemoryRecallTool, MemoryStoreTool, ModelRoutingConfigTool, PdfReadTool, ProcessTool,
    ProxyConfigTool, PushoverTool, ReadFilePolicy, ReadFileTool, ScheduleTool, ScreenshotTool,
    ShellPolicy, ShellTool, SopAdvanceTool, SopApproveTool, SopExecuteTool, SopListTool,
    SopStatusTool, SubAgentListTool, SubAgentManageTool, SubAgentSpawnTool, TaskPlanTool,
    ToolSecurityPolicy, TtsTool, UrlValidationTool, VideoGenTool, WasmModuleTool, WasmToolExecTool,
    WebFetchTool, WebSearchTool, WriteFilePolicy, WriteFileTool,
};
#[cfg(feature = "document-tools")]
pub use agentzero_tools::{DocxReadTool, HtmlExtractTool};
pub use mcp::create_mcp_tools;
#[cfg(feature = "wasm-plugins")]
pub use wasm_bridge::WasmTool;

pub fn default_tools(
    policy: &ToolSecurityPolicy,
    router: Option<ModelRouter>,
    delegate_agents: Option<HashMap<String, DelegateConfig>>,
) -> anyhow::Result<Vec<Box<dyn Tool>>> {
    default_tools_inner(policy, router, delegate_agents, None)
}

/// Build the default tool set, optionally with an `AgentStore` for the
/// `agent_manage` tool.
pub fn default_tools_with_store(
    policy: &ToolSecurityPolicy,
    router: Option<ModelRouter>,
    delegate_agents: Option<HashMap<String, DelegateConfig>>,
    agent_store: Option<Arc<dyn AgentStoreApi>>,
) -> anyhow::Result<Vec<Box<dyn Tool>>> {
    default_tools_inner(policy, router, delegate_agents, agent_store)
}

fn default_tools_inner(
    policy: &ToolSecurityPolicy,
    router: Option<ModelRouter>,
    delegate_agents: Option<HashMap<String, DelegateConfig>>,
    agent_store: Option<Arc<dyn AgentStoreApi>>,
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
        #[cfg(feature = "document-tools")]
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
        tools.push(Box::new(WebSearchTool::new(
            policy.web_search_config.clone(),
        )));
    }

    if policy.enable_browser {
        tools.push(Box::new(BrowserTool::default()));
    }

    if policy.enable_browser_open {
        tools.push(Box::new(BrowserOpenTool::default()));
    }

    if policy.enable_http_request {
        tools.push(Box::new(
            HttpRequestTool::default().with_url_policy(policy.url_access.clone()),
        ));
    }

    if policy.enable_web_fetch {
        tools.push(Box::new(
            WebFetchTool::default().with_url_policy(policy.url_access.clone()),
        ));
    }

    #[cfg(feature = "document-tools")]
    if policy.enable_html_extract {
        tools.push(Box::new(HtmlExtractTool));
    }

    if policy.enable_url_validation {
        tools.push(Box::new(
            UrlValidationTool::default().with_url_policy(policy.url_access.clone()),
        ));
    }

    if policy.enable_agents_ipc {
        tools.push(Box::new(AgentsIpcTool));
    }

    if policy.enable_mcp && !policy.mcp_servers.is_empty() {
        let mcp_tools = create_mcp_tools(&policy.mcp_servers)?;
        tools.extend(mcp_tools);
    }

    if policy.enable_code_interpreter {
        tools.push(Box::new(CodeInterpreterTool::default()));
    }

    if policy.enable_tts {
        tools.push(Box::new(TtsTool::default()));
    }

    if policy.enable_image_gen {
        tools.push(Box::new(ImageGenTool::default()));
    }

    if policy.enable_video_gen {
        tools.push(Box::new(VideoGenTool::default()));
    }

    #[cfg(feature = "autopilot")]
    if policy.enable_autopilot {
        tools.push(Box::new(agentzero_autopilot::tools::ProposalCreateTool));
        tools.push(Box::new(agentzero_autopilot::tools::ProposalVoteTool));
        tools.push(Box::new(agentzero_autopilot::tools::MissionStatusTool));
        tools.push(Box::new(agentzero_autopilot::tools::TriggerFireTool));
    }

    if policy.enable_domain_tools {
        tools.push(Box::new(DomainCreateTool));
        tools.push(Box::new(DomainUpdateTool));
        tools.push(Box::new(DomainListTool));
        tools.push(Box::new(DomainInfoTool));
        tools.push(Box::new(DomainSearchTool::default()));
        tools.push(Box::new(DomainVerifyTool::default()));
        tools.push(Box::new(DomainWorkflowTool));
        tools.push(Box::new(DomainLearnTool));
        tools.push(Box::new(DomainLessonsTool));
    }

    if policy.enable_composio {
        tools.push(Box::new(ComposioTool));
    }

    if policy.enable_pushover {
        tools.push(Box::new(PushoverTool));
    }

    if policy.enable_agent_manage {
        if let Some(ref store) = agent_store {
            tools.push(Box::new(AgentManageTool::new(Arc::clone(store))));
        }
    }

    if policy.enable_self_config {
        tools.push(Box::new(ConfigManageTool));
        tools.push(Box::new(SkillManageTool));
        tools.push(Box::new(PluginScaffoldTool));
    }

    #[cfg(feature = "wasm-plugins")]
    if policy.enable_wasm_plugins {
        use agentzero_plugins::package::{discover_plugins, filter_by_state, PluginState};
        use agentzero_plugins::wasm::WasmIsolationPolicy;

        let discovered = discover_plugins(
            policy.wasm_global_plugin_dir.as_deref(),
            policy.wasm_project_plugin_dir.as_deref(),
            policy.wasm_dev_plugin_dir.as_deref(),
        );

        // Filter out disabled plugins via state.json
        let discovered = if let Some(ref global_dir) = policy.wasm_global_plugin_dir {
            let state = PluginState::load(global_dir);
            filter_by_state(discovered, &state)
        } else {
            discovered
        };

        let mut isolation = WasmIsolationPolicy::default();
        // Privacy enforcement: disable network for plugins when network tools
        // are disabled (e.g., local_only mode).
        if !policy.enable_http_request && !policy.enable_web_fetch {
            isolation.allow_network = false;
        }
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

/// Build the default tool set, then filter by depth policy.
/// If no policy is provided or no rule matches the depth, all tools pass through.
pub fn default_tools_with_depth(
    policy: &ToolSecurityPolicy,
    router: Option<ModelRouter>,
    delegate_agents: Option<HashMap<String, DelegateConfig>>,
    depth: u8,
    depth_policy: &DepthPolicy,
) -> anyhow::Result<Vec<Box<dyn Tool>>> {
    let all_tools = default_tools_inner(policy, router, delegate_agents, None)?;

    if depth_policy.rules.is_empty() {
        return Ok(all_tools);
    }

    let tool_names: Vec<&str> = all_tools.iter().map(|t| t.name()).collect();
    let allowed = depth_policy.filter_tools(depth, &tool_names);

    let filtered = all_tools
        .into_iter()
        .filter(|t| allowed.iter().any(|name| name == t.name()))
        .collect();

    Ok(filtered)
}

#[cfg(test)]
mod tests {
    use super::{default_tools, default_tools_with_depth, ToolSecurityPolicy};
    use agentzero_core::{DepthPolicy, DepthRule};
    use std::collections::HashSet;

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

    #[test]
    fn default_tools_include_agents_ipc_when_enabled() {
        let policy = ToolSecurityPolicy::default_for_workspace(
            std::env::current_dir().expect("cwd should be readable"),
        );
        // agents_ipc defaults to true
        assert!(policy.enable_agents_ipc);
        let tools = default_tools(&policy, None, None).expect("default tools should build");
        let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"agents_ipc"),
            "agents_ipc should be registered"
        );
    }

    #[test]
    fn default_tools_include_network_tools_when_enabled() {
        let mut policy = ToolSecurityPolicy::default_for_workspace(
            std::env::current_dir().expect("cwd should be readable"),
        );
        policy.enable_http_request = true;
        policy.enable_web_fetch = true;
        policy.enable_url_validation = true;

        let tools = default_tools(&policy, None, None).expect("default tools should build");
        let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"http_request"),
            "http_request should be registered"
        );
        assert!(
            names.contains(&"web_fetch"),
            "web_fetch should be registered"
        );
        assert!(
            names.contains(&"url_validation"),
            "url_validation should be registered"
        );
    }

    #[test]
    fn default_tools_exclude_network_tools_when_disabled() {
        let mut policy = ToolSecurityPolicy::default_for_workspace(
            std::env::current_dir().expect("cwd should be readable"),
        );
        policy.enable_http_request = false;
        policy.enable_web_fetch = false;
        policy.enable_url_validation = false;
        policy.enable_agents_ipc = false;

        let tools = default_tools(&policy, None, None).expect("default tools should build");
        let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"http_request"));
        assert!(!names.contains(&"web_fetch"));
        assert!(!names.contains(&"url_validation"));
        assert!(!names.contains(&"agents_ipc"));
    }

    #[test]
    fn depth_tools_empty_policy_returns_all_tools() {
        let policy = ToolSecurityPolicy::default_for_workspace(
            std::env::current_dir().expect("cwd should be readable"),
        );
        let depth_policy = DepthPolicy::default(); // empty rules
        let all_tools = default_tools(&policy, None, None).expect("default tools should build");
        let all_count = all_tools.len();

        let depth_tools = default_tools_with_depth(&policy, None, None, 0, &depth_policy)
            .expect("depth tools should build");
        assert_eq!(
            depth_tools.len(),
            all_count,
            "empty policy should return all tools"
        );

        // Also at deeper depth
        let depth_tools_deep = default_tools_with_depth(&policy, None, None, 5, &depth_policy)
            .expect("depth tools should build");
        assert_eq!(depth_tools_deep.len(), all_count);
    }

    #[test]
    fn depth_tools_denylist_removes_tools_at_depth() {
        let policy = ToolSecurityPolicy::default_for_workspace(
            std::env::current_dir().expect("cwd should be readable"),
        );
        let depth_policy = DepthPolicy {
            rules: vec![DepthRule {
                max_depth: 5,
                allowed_tools: HashSet::new(), // no allowlist = start with all
                denied_tools: HashSet::from(["shell".to_string(), "read_file".to_string()]),
            }],
        };

        let tools = default_tools_with_depth(&policy, None, None, 1, &depth_policy)
            .expect("depth tools should build");
        let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"shell"), "shell should be denied");
        assert!(!names.contains(&"read_file"), "read_file should be denied");
        // Other tools should still be present
        assert!(names.contains(&"glob_search"), "glob_search should remain");
    }

    #[test]
    fn depth_tools_allowlist_restricts_tools_at_depth() {
        let policy = ToolSecurityPolicy::default_for_workspace(
            std::env::current_dir().expect("cwd should be readable"),
        );
        let depth_policy = DepthPolicy {
            rules: vec![DepthRule {
                max_depth: 3,
                allowed_tools: HashSet::from([
                    "read_file".to_string(),
                    "glob_search".to_string(),
                    "content_search".to_string(),
                ]),
                denied_tools: HashSet::new(),
            }],
        };

        let tools = default_tools_with_depth(&policy, None, None, 1, &depth_policy)
            .expect("depth tools should build");
        let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
        assert_eq!(names.len(), 3, "only 3 allowed tools should remain");
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"glob_search"));
        assert!(names.contains(&"content_search"));
    }

    #[test]
    fn depth_tools_no_matching_rule_returns_all() {
        let policy = ToolSecurityPolicy::default_for_workspace(
            std::env::current_dir().expect("cwd should be readable"),
        );
        // Rule only applies to depth <= 2
        let depth_policy = DepthPolicy {
            rules: vec![DepthRule {
                max_depth: 2,
                allowed_tools: HashSet::from(["read_file".to_string()]),
                denied_tools: HashSet::new(),
            }],
        };

        let all_tools = default_tools(&policy, None, None).expect("default tools should build");
        let all_count = all_tools.len();

        // Depth 1 should match the rule → restricted
        let tools_d1 = default_tools_with_depth(&policy, None, None, 1, &depth_policy)
            .expect("depth tools should build");
        assert_eq!(tools_d1.len(), 1, "depth 1 should match rule");

        // Depth 5 exceeds max_depth 2 → no rule matches → all tools
        let tools_d5 = default_tools_with_depth(&policy, None, None, 5, &depth_policy)
            .expect("depth tools should build");
        assert_eq!(tools_d5.len(), all_count, "depth 5 should return all tools");
    }
}
