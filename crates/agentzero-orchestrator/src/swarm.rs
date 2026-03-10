//! Swarm builder: constructs a Coordinator from config.
//!
//! Reads `[swarm]` config, builds an InMemoryBus, creates agent workers
//! for each configured agent, and returns a ready-to-run Coordinator.

use crate::agent_router::{AgentDescriptor, AgentRouter};
use crate::coordinator::Coordinator;
use agentzero_channels::ChannelRegistry;
use agentzero_config::AgentZeroConfig;
use agentzero_core::event_bus::{FileBackedBus, InMemoryBus};
use agentzero_core::Agent;
use agentzero_infra::runtime::{build_runtime_execution, RunAgentRequest};
use std::path::Path;
use std::sync::Arc;

/// Build a swarm coordinator from config.
///
/// Returns `None` if swarm is not enabled. Returns `Some(coordinator, shutdown_tx)`
/// if swarm is enabled and at least one agent is configured.
pub async fn build_swarm(
    config: &AgentZeroConfig,
    channels: Arc<ChannelRegistry>,
    config_path: &Path,
    workspace_root: &Path,
) -> anyhow::Result<Option<(Coordinator, tokio::sync::watch::Sender<bool>)>> {
    let swarm_config = &config.swarm;

    if !swarm_config.enabled {
        return Ok(None);
    }

    if swarm_config.agents.is_empty() {
        tracing::warn!("swarm enabled but no agents configured");
        return Ok(None);
    }

    tracing::info!(
        agents = swarm_config.agents.len(),
        pipelines = swarm_config.pipelines.len(),
        bus_capacity = swarm_config.event_bus_capacity,
        "building swarm"
    );

    // 1. Create the event bus (file-backed if event_log_path is set)
    let bus: Arc<dyn agentzero_core::event_bus::EventBus> =
        if let Some(ref log_path) = swarm_config.event_log_path {
            let resolved = if Path::new(log_path).is_relative() {
                workspace_root.join(log_path)
            } else {
                log_path.into()
            };
            tracing::info!(path = %resolved.display(), "using file-backed event bus");
            Arc::new(
                FileBackedBus::open(&resolved, swarm_config.event_bus_capacity)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("failed to open event log at {}: {e}", resolved.display())
                    })?,
            )
        } else {
            Arc::new(InMemoryBus::new(swarm_config.event_bus_capacity))
        };

    // 2. Build the AI router
    let router = if !swarm_config.router.provider.is_empty() {
        // Build a provider for the router's classification model.
        let router_req = RunAgentRequest {
            workspace_root: workspace_root.to_path_buf(),
            config_path: config_path.to_path_buf(),
            message: String::new(),
            provider_override: Some(swarm_config.router.provider.clone()),
            model_override: if swarm_config.router.model.is_empty() {
                None
            } else {
                Some(swarm_config.router.model.clone())
            },
            profile_override: None,
            extra_tools: Vec::new(),
            conversation_id: None,
        };
        match build_runtime_execution(router_req).await {
            Ok(exec) => AgentRouter::new(
                Some(exec.provider),
                swarm_config.router.fallback_to_keywords,
            ),
            Err(e) => {
                tracing::warn!(error = %e, "failed to build router provider, using keyword-only");
                AgentRouter::keywords_only()
            }
        }
    } else {
        AgentRouter::keywords_only()
    };

    // 3. Create the coordinator
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);

    let mut coord = Coordinator::new(
        bus.clone(),
        channels,
        router,
        swarm_config.pipelines.clone(),
        swarm_config.shutdown_grace_ms,
    );

    // 4. Register each agent
    for (agent_id, agent_cfg) in &swarm_config.agents {
        tracing::info!(
            agent = %agent_id,
            name = %agent_cfg.name,
            subscribes_to = ?agent_cfg.subscribes_to,
            produces = ?agent_cfg.produces,
            "registering swarm agent"
        );

        let descriptor = AgentDescriptor {
            id: agent_id.clone(),
            name: agent_cfg.name.clone(),
            description: agent_cfg.description.clone(),
            keywords: agent_cfg.keywords.clone(),
            subscribes_to: agent_cfg.subscribes_to.clone(),
            produces: agent_cfg.produces.clone(),
            privacy_boundary: agent_cfg.privacy_boundary.clone(),
        };

        // Build a runtime execution for this agent.
        let req = RunAgentRequest {
            workspace_root: workspace_root.to_path_buf(),
            config_path: config_path.to_path_buf(),
            message: String::new(),
            provider_override: if agent_cfg.provider.is_empty() {
                None
            } else {
                Some(agent_cfg.provider.clone())
            },
            model_override: if agent_cfg.model.is_empty() {
                None
            } else {
                Some(agent_cfg.model.clone())
            },
            profile_override: None,
            extra_tools: Vec::new(),
            conversation_id: None,
        };

        match build_runtime_execution(req).await {
            Ok(exec) => {
                let mut agent_config = exec.config;
                if let Some(ref prompt) = agent_cfg.system_prompt {
                    agent_config.system_prompt = Some(prompt.clone());
                }
                agent_config.max_tool_iterations = agent_cfg.max_iterations;
                agent_config.privacy_boundary = agent_cfg.privacy_boundary.clone();

                let tools = if agent_cfg.allowed_tools.is_empty() {
                    exec.tools
                } else {
                    let allowed: std::collections::HashSet<&str> =
                        agent_cfg.allowed_tools.iter().map(|s| s.as_str()).collect();
                    exec.tools
                        .into_iter()
                        .filter(|t| allowed.contains(t.name()))
                        .collect()
                };

                let agent = Agent::new(agent_config, exec.provider, exec.memory, tools);

                coord.register_agent(
                    descriptor,
                    agent,
                    workspace_root.to_string_lossy().to_string(),
                );
            }
            Err(e) => {
                tracing::error!(
                    agent = %agent_id,
                    error = %e,
                    "failed to build agent, skipping"
                );
            }
        }
    }

    Ok(Some((coord, shutdown_tx)))
}
