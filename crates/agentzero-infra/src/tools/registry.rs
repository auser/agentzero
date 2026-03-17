//! Builder-pattern tool registry for declarative tool set construction.
//!
//! Replaces the manual if-chain in `default_tools_inner` with a composable
//! builder that groups tools by category.

use agentzero_core::agent_store::AgentStoreApi;
use agentzero_core::delegation::DelegateConfig;
use agentzero_core::routing::ModelRouter;
use agentzero_core::Tool;
use agentzero_tools::ToolBuilder;
use std::collections::HashMap;
use std::sync::Arc;

use super::{AgentManageTool, ConfigManageTool, PluginScaffoldTool, SkillManageTool};
use super::{
    AgentsIpcTool, ApplyPatchTool, BrowserOpenTool, BrowserTool, CliDiscoveryTool,
    CodeInterpreterTool, ComposioTool, ContentSearchTool, CronAddTool, CronListTool, CronPauseTool,
    CronRemoveTool, CronResumeTool, CronUpdateTool, DelegateCoordinationStatusTool, DelegateTool,
    DomainCreateTool, DomainInfoTool, DomainLearnTool, DomainLessonsTool, DomainListTool,
    DomainSearchTool, DomainUpdateTool, DomainVerifyTool, DomainWorkflowTool, FileEditTool,
    GitOperationsTool, GlobSearchTool, HardwareBoardInfoTool, HardwareMemoryMapTool,
    HardwareMemoryReadTool, HttpRequestTool, ImageGenTool, ImageInfoTool, MemoryForgetTool,
    MemoryRecallTool, MemoryStoreTool, ModelRoutingConfigTool, PdfReadTool, ProcessTool,
    ProxyConfigTool, PushoverTool, ReadFileTool, ScheduleTool, ScreenshotTool, ShellTool,
    SopAdvanceTool, SopApproveTool, SopExecuteTool, SopListTool, SopStatusTool, SubAgentListTool,
    SubAgentManageTool, SubAgentSpawnTool, TaskPlanTool, ToolSecurityPolicy, TtsTool,
    UrlValidationTool, VideoGenTool, WasmModuleTool, WasmToolExecTool, WebFetchTool, WebSearchTool,
    WriteFileTool,
};
#[cfg(feature = "document-tools")]
use super::{DocxReadTool, HtmlExtractTool};

/// Declarative tool registry builder.
///
/// Usage:
/// ```ignore
/// let tools = ToolRegistry::new()
///     .with_core(&policy)
///     .with_files(&policy)
///     .with_network(&policy)
///     .with_delegation(router, delegates, store)
///     .build()?;
/// ```
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Core tools: always available regardless of policy.
    /// Includes: read_file, shell, glob_search, content_search, memory,
    /// image_info, pdf_read, screenshot, task_plan, process, sub_agent,
    /// sop, hardware, wasm, cli_discovery, proxy_config, delegate_coordination.
    pub fn with_core(mut self, policy: &ToolSecurityPolicy) -> Self {
        self.tools.extend(vec![
            Box::new(ReadFileTool::new(policy.read_file.clone())) as Box<dyn Tool>,
            Box::new(ShellTool::new(policy.shell.clone())),
            Box::new(GlobSearchTool),
            Box::new(ContentSearchTool),
            Box::new(MemoryStoreTool),
            Box::new(MemoryRecallTool),
            Box::new(MemoryForgetTool),
            Box::new(ImageInfoTool),
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
        ]);

        #[cfg(feature = "document-tools")]
        self.tools.push(Box::new(DocxReadTool));

        self
    }

    /// File write tools: write_file, apply_patch, file_edit.
    pub fn with_files(mut self, policy: &ToolSecurityPolicy) -> Self {
        if policy.enable_write_file {
            self.tools
                .push(Box::new(WriteFileTool::new(policy.write_file.clone())));
            self.tools.push(Box::new(ApplyPatchTool));
            self.tools.push(Box::new(FileEditTool::new(
                policy.write_file.allowed_root.clone(),
                policy.write_file.max_write_bytes,
            )));
        }
        if policy.enable_git {
            self.tools.push(Box::new(GitOperationsTool::new()));
        }
        self
    }

    /// Cron/scheduling tools.
    pub fn with_cron(mut self, policy: &ToolSecurityPolicy) -> Self {
        if policy.enable_cron {
            self.tools.push(Box::new(CronAddTool));
            self.tools.push(Box::new(CronListTool));
            self.tools.push(Box::new(CronRemoveTool));
            self.tools.push(Box::new(CronUpdateTool));
            self.tools.push(Box::new(CronPauseTool));
            self.tools.push(Box::new(CronResumeTool));
            self.tools.push(Box::new(ScheduleTool));
        }
        self
    }

    /// Network tools: web_search, browser, http_request, web_fetch, url_validation.
    pub fn with_network(mut self, policy: &ToolSecurityPolicy) -> Self {
        if policy.enable_web_search {
            self.tools.push(Box::new(WebSearchTool::new(
                policy.web_search_config.clone(),
            )));
        }
        if policy.enable_browser {
            self.tools.push(Box::new(BrowserTool::default()));
        }
        if policy.enable_browser_open {
            self.tools.push(Box::new(BrowserOpenTool::default()));
        }
        if policy.enable_http_request {
            self.tools.push(Box::new(
                HttpRequestTool::default().with_url_policy(policy.url_access.clone()),
            ));
        }
        if policy.enable_web_fetch {
            self.tools.push(Box::new(
                WebFetchTool::default().with_url_policy(policy.url_access.clone()),
            ));
        }
        #[cfg(feature = "document-tools")]
        if policy.enable_html_extract {
            self.tools.push(Box::new(HtmlExtractTool));
        }
        if policy.enable_url_validation {
            self.tools.push(Box::new(
                UrlValidationTool::default().with_url_policy(policy.url_access.clone()),
            ));
        }
        self
    }

    /// IPC and communication tools.
    pub fn with_ipc(mut self, policy: &ToolSecurityPolicy) -> Self {
        if policy.enable_agents_ipc {
            self.tools.push(Box::new(AgentsIpcTool));
        }
        self
    }

    /// Code execution and media generation tools.
    pub fn with_media(mut self, policy: &ToolSecurityPolicy) -> Self {
        if policy.enable_code_interpreter {
            self.tools.push(Box::new(CodeInterpreterTool::default()));
        }
        if policy.enable_tts {
            self.tools.push(Box::new(TtsTool::default()));
        }
        if policy.enable_image_gen {
            self.tools.push(Box::new(ImageGenTool::default()));
        }
        if policy.enable_video_gen {
            self.tools.push(Box::new(VideoGenTool::default()));
        }
        self
    }

    /// Domain learning tools.
    pub fn with_domain(mut self, policy: &ToolSecurityPolicy) -> Self {
        if policy.enable_domain_tools {
            self.tools.push(Box::new(DomainCreateTool));
            self.tools.push(Box::new(DomainUpdateTool));
            self.tools.push(Box::new(DomainListTool));
            self.tools.push(Box::new(DomainInfoTool));
            self.tools.push(Box::new(DomainSearchTool::default()));
            self.tools.push(Box::new(DomainVerifyTool::default()));
            self.tools.push(Box::new(DomainWorkflowTool));
            self.tools.push(Box::new(DomainLearnTool));
            self.tools.push(Box::new(DomainLessonsTool));
        }
        self
    }

    /// Integration tools: composio, pushover.
    pub fn with_integrations(mut self, policy: &ToolSecurityPolicy) -> Self {
        if policy.enable_composio {
            self.tools.push(Box::new(ComposioTool));
        }
        if policy.enable_pushover {
            self.tools.push(Box::new(PushoverTool));
        }
        self
    }

    /// Agent management, config, skill, and plugin tools.
    pub fn with_self_config(
        mut self,
        policy: &ToolSecurityPolicy,
        agent_store: Option<&Arc<dyn AgentStoreApi>>,
    ) -> Self {
        if policy.enable_agent_manage {
            if let Some(store) = agent_store {
                self.tools
                    .push(Box::new(AgentManageTool::new(Arc::clone(store))));
            }
        }
        if policy.enable_self_config {
            self.tools.push(Box::new(ConfigManageTool));
            self.tools.push(Box::new(SkillManageTool));
            self.tools.push(Box::new(PluginScaffoldTool));
        }
        self
    }

    /// MCP server tools.
    pub fn with_mcp(mut self, policy: &ToolSecurityPolicy) -> anyhow::Result<Self> {
        if policy.enable_mcp && !policy.mcp_servers.is_empty() {
            let mcp_tools = super::create_mcp_tools(&policy.mcp_servers)?;
            self.tools.extend(mcp_tools);
        }
        Ok(self)
    }

    /// WASM plugin tools.
    #[cfg(feature = "wasm-plugins")]
    pub fn with_wasm_plugins(mut self, policy: &ToolSecurityPolicy) -> Self {
        if policy.enable_wasm_plugins {
            use agentzero_plugins::package::{discover_plugins, filter_by_state, PluginState};
            use agentzero_plugins::wasm::WasmIsolationPolicy;

            let discovered = discover_plugins(
                policy.wasm_global_plugin_dir.as_deref(),
                policy.wasm_project_plugin_dir.as_deref(),
                policy.wasm_dev_plugin_dir.as_deref(),
            );

            let discovered = if let Some(ref global_dir) = policy.wasm_global_plugin_dir {
                let state = PluginState::load(global_dir);
                filter_by_state(discovered, &state)
            } else {
                discovered
            };

            let mut isolation = WasmIsolationPolicy::default();
            if !policy.enable_http_request && !policy.enable_web_fetch {
                isolation.allow_network = false;
            }
            for plugin in discovered {
                match super::WasmTool::from_manifest(
                    plugin.manifest.clone(),
                    plugin.wasm_path.clone(),
                    isolation.clone(),
                ) {
                    Ok(tool) => self.tools.push(Box::new(tool)),
                    Err(e) => {
                        tracing::warn!("skipping wasm plugin {}: {e}", plugin.manifest.id);
                    }
                }
            }
        }
        self
    }

    /// Autopilot tools.
    #[cfg(feature = "autopilot")]
    pub fn with_autopilot(mut self, policy: &ToolSecurityPolicy) -> Self {
        if policy.enable_autopilot {
            self.tools
                .push(Box::new(agentzero_autopilot::tools::ProposalCreateTool));
            self.tools
                .push(Box::new(agentzero_autopilot::tools::ProposalVoteTool));
            self.tools
                .push(Box::new(agentzero_autopilot::tools::MissionStatusTool));
            self.tools
                .push(Box::new(agentzero_autopilot::tools::TriggerFireTool));
        }
        self
    }

    /// Model routing and delegation tools.
    pub fn with_delegation(
        mut self,
        policy: &ToolSecurityPolicy,
        router: Option<ModelRouter>,
        delegate_agents: Option<HashMap<String, DelegateConfig>>,
    ) -> Self {
        if let Some(r) = router {
            self.tools.push(Box::new(ModelRoutingConfigTool::new(r)));
        }
        if let Some(agents) = delegate_agents {
            if !agents.is_empty() {
                let policy_clone = policy.clone();
                let builder: ToolBuilder =
                    Arc::new(move || super::default_tools(&policy_clone, None, None));
                self.tools
                    .push(Box::new(DelegateTool::new(agents, 0, builder)));
            }
        }
        self
    }

    /// Apply a named preset: "sandbox", "dev", or "full".
    /// This is a convenience that calls the appropriate category methods.
    pub fn with_preset(
        self,
        policy: &ToolSecurityPolicy,
        router: Option<ModelRouter>,
        delegate_agents: Option<HashMap<String, DelegateConfig>>,
        agent_store: Option<&Arc<dyn AgentStoreApi>>,
    ) -> anyhow::Result<Self> {
        let registry = self
            .with_core(policy)
            .with_files(policy)
            .with_cron(policy)
            .with_network(policy)
            .with_ipc(policy)
            .with_media(policy)
            .with_domain(policy)
            .with_integrations(policy)
            .with_self_config(policy, agent_store)
            .with_mcp(policy)?;

        #[cfg(feature = "autopilot")]
        let registry = registry.with_autopilot(policy);

        #[cfg(feature = "wasm-plugins")]
        let registry = registry.with_wasm_plugins(policy);

        let registry = registry.with_delegation(policy, router, delegate_agents);

        Ok(registry)
    }

    /// Consume the builder and return the tool list.
    pub fn build(self) -> Vec<Box<dyn Tool>> {
        self.tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_core_includes_basic_tools() {
        let policy =
            ToolSecurityPolicy::default_for_workspace(std::env::current_dir().expect("cwd"));
        let tools = ToolRegistry::new().with_core(&policy).build();
        let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"glob_search"));
        assert!(names.contains(&"memory_store"));
    }

    #[test]
    fn registry_files_gated_by_policy() {
        let mut policy =
            ToolSecurityPolicy::default_for_workspace(std::env::current_dir().expect("cwd"));

        // Disabled
        let tools = ToolRegistry::new().with_files(&policy).build();
        let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"write_file"));
        assert!(!names.contains(&"git_operations"));

        // Enabled
        policy.enable_write_file = true;
        policy.enable_git = true;
        let tools = ToolRegistry::new().with_files(&policy).build();
        let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"git_operations"));
    }

    #[test]
    fn registry_preset_builds_all_categories() {
        let policy = ToolSecurityPolicy::preset_full(std::env::current_dir().expect("cwd"));
        let tools = ToolRegistry::new()
            .with_preset(&policy, None, None, None)
            .expect("preset should build")
            .build();
        let names: Vec<_> = tools.iter().map(|t| t.name()).collect();

        // Should have core + files + cron + network + media + domain tools
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"cron_add"));
        assert!(names.contains(&"web_search"));
        assert!(names.contains(&"http_request"));
        assert!(names.contains(&"code_interpreter"));
        assert!(names.contains(&"domain_create"));
    }

    #[test]
    fn registry_sandbox_excludes_write_tools() {
        let policy = ToolSecurityPolicy::preset_sandbox(std::env::current_dir().expect("cwd"));
        let tools = ToolRegistry::new()
            .with_preset(&policy, None, None, None)
            .expect("preset should build")
            .build();
        let names: Vec<_> = tools.iter().map(|t| t.name()).collect();

        assert!(names.contains(&"read_file"), "read should be available");
        assert!(
            !names.contains(&"write_file"),
            "write should NOT be available"
        );
        assert!(
            !names.contains(&"git_operations"),
            "git should NOT be available"
        );
        assert!(
            !names.contains(&"web_search"),
            "web_search should NOT be available"
        );
    }
}
