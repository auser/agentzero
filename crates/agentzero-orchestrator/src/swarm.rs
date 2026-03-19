//! Swarm builder: constructs a Coordinator from config.
//!
//! Reads `[swarm]` config, builds an InMemoryBus, creates agent workers
//! for each configured agent, and returns a ready-to-run Coordinator.
//!
//! Uses a two-pass registration process so that the `ConverseTool` can be
//! injected with references to all agents' task channels before workers start.

use crate::a2a_client::A2aAgentEndpoint;
use crate::agent_router::{AgentDescriptor, AgentRouter};
use crate::coordinator::{Coordinator, TaskMessage};
use crate::presence::PresenceStore;
use agentzero_channels::ChannelRegistry;
use agentzero_config::AgentZeroConfig;
use agentzero_core::event_bus::{Event, EventBus, FileBackedBus, InMemoryBus};
use agentzero_core::{Agent, AgentEndpoint};
use agentzero_infra::runtime::{build_runtime_execution, RunAgentRequest};
use agentzero_storage::SqliteEventBus;
use agentzero_tools::ConverseTool;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

// ─── SwarmAgentEndpoint ─────────────────────────────────────────────────────

/// Wraps a coordinator agent worker's `task_tx` channel as an [`AgentEndpoint`].
///
/// When `send()` is called, it creates a `TaskMessage` with a oneshot result
/// channel, sends it to the agent worker, and waits for the response.
struct SwarmAgentEndpoint {
    id: String,
    task_tx: mpsc::Sender<TaskMessage>,
    timeout_secs: u64,
}

#[async_trait]
impl AgentEndpoint for SwarmAgentEndpoint {
    async fn send(&self, message: &str, conversation_id: &str) -> anyhow::Result<String> {
        let (result_tx, result_rx) = oneshot::channel();
        let event = Event::new(format!("converse.{}", self.id), "converse", message);
        self.task_tx
            .send(TaskMessage {
                event,
                correlation_id: conversation_id.to_string(),
                result_tx: Some(result_tx),
                cancelled: Arc::new(AtomicBool::new(false)),
            })
            .await
            .map_err(|_| anyhow::anyhow!("agent `{}` worker channel closed", self.id))?;

        let result = tokio::time::timeout(Duration::from_secs(self.timeout_secs), result_rx)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "agent `{}` did not respond within {}s",
                    self.id,
                    self.timeout_secs
                )
            })?
            .map_err(|_| {
                anyhow::anyhow!("agent `{}` worker dropped the result channel", self.id)
            })?;

        Ok(result.payload)
    }

    fn agent_id(&self) -> &str {
        &self.id
    }
}

// ─── Intermediate build state ───────────────────────────────────────────────

/// Holds a built agent and its pre-created task channel before registration.
struct BuiltAgent {
    descriptor: AgentDescriptor,
    agent: Agent,
    workspace_root: String,
    task_tx: mpsc::Sender<TaskMessage>,
    task_rx: mpsc::Receiver<TaskMessage>,
    /// Whether this agent has "converse" in its allowed_tools.
    wants_converse: bool,
    /// Conversation config from the agent's TOML.
    max_turns: usize,
    turn_timeout_secs: u64,
}

// ─── build_event_bus ────────────────────────────────────────────────────────

/// Build the event bus from config.
///
/// Used by the gateway and swarm builder to create a shared bus that is wired
/// into `JobStore`, `PresenceStore`, and the gateway SSE/WebSocket endpoints.
pub async fn build_event_bus(
    config: &AgentZeroConfig,
    workspace_root: &Path,
) -> anyhow::Result<Arc<dyn EventBus>> {
    let swarm_config = &config.swarm;
    let bus_kind = swarm_config.event_bus.as_deref().unwrap_or_else(|| {
        if swarm_config.event_log_path.is_some() {
            "file"
        } else {
            "memory"
        }
    });

    match bus_kind {
        "sqlite" => {
            let db_path = swarm_config
                .event_db_path
                .as_deref()
                .unwrap_or("data/events.db");
            let resolved = if Path::new(db_path).is_relative() {
                workspace_root.join(db_path)
            } else {
                db_path.into()
            };
            tracing::info!(
                path = %resolved.display(),
                retention_days = swarm_config.event_retention_days,
                "using sqlite event bus"
            );
            Ok(Arc::new(
                SqliteEventBus::open(&resolved, swarm_config.event_bus_capacity).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to open sqlite event bus at {}: {e}",
                        resolved.display()
                    )
                })?,
            ))
        }
        "file" => {
            let log_path = swarm_config
                .event_log_path
                .as_deref()
                .unwrap_or("data/events.jsonl");
            let resolved = if Path::new(log_path).is_relative() {
                workspace_root.join(log_path)
            } else {
                log_path.into()
            };
            tracing::info!(path = %resolved.display(), "using file-backed event bus");
            Ok(Arc::new(
                FileBackedBus::open(&resolved, swarm_config.event_bus_capacity)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("failed to open event log at {}: {e}", resolved.display())
                    })?,
            ))
        }
        "gossip" => {
            let db_path = swarm_config
                .event_db_path
                .as_deref()
                .unwrap_or("data/events.db");
            let resolved = if Path::new(db_path).is_relative() {
                workspace_root.join(db_path)
            } else {
                db_path.into()
            };
            let port = swarm_config.gossip_port.unwrap_or(9100);
            let listen_addr: std::net::SocketAddr = format!("0.0.0.0:{port}")
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid gossip listen address: {e}"))?;
            let peers: Vec<std::net::SocketAddr> = swarm_config
                .gossip_peers
                .iter()
                .filter_map(|p| p.parse().ok())
                .collect();
            tracing::info!(
                path = %resolved.display(),
                addr = %listen_addr,
                peers = peers.len(),
                "using gossip event bus"
            );
            let bus = crate::gossip::GossipEventBus::start(crate::gossip::GossipConfig {
                listen_addr,
                peers,
                db_path: resolved.to_string_lossy().to_string(),
                capacity: swarm_config.event_bus_capacity,
            })
            .await
            .map_err(|e| anyhow::anyhow!("failed to start gossip event bus: {e}"))?;
            Ok(bus as Arc<dyn EventBus>)
        }
        _ => {
            tracing::info!("using in-memory event bus");
            Ok(Arc::new(InMemoryBus::new(swarm_config.event_bus_capacity)))
        }
    }
}

// ─── build_swarm ────────────────────────────────────────────────────────────

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
    let bus = build_event_bus(config, workspace_root).await?;
    build_swarm_with_presence(config, channels, config_path, workspace_root, None, bus).await
}

/// Build a swarm coordinator from config, optionally wiring a presence store
/// so agents can register for the `/v1/agents` endpoint.
///
/// The caller must supply a pre-built `bus` so that the same bus instance can
/// be wired into `JobStore` and `PresenceStore` before the swarm starts.
pub async fn build_swarm_with_presence(
    config: &AgentZeroConfig,
    channels: Arc<ChannelRegistry>,
    config_path: &Path,
    workspace_root: &Path,
    presence: Option<Arc<PresenceStore>>,
    bus: Arc<dyn EventBus>,
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

    // 1. Build the AI router
    let router = if !swarm_config.router.provider.is_empty() {
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
            agent_store: None,
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

    // 2. Create the coordinator
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);

    let mut coord = Coordinator::new(
        bus.clone(),
        channels,
        router,
        swarm_config.pipelines.clone(),
        swarm_config.shutdown_grace_ms,
    );
    if let Some(ps) = presence {
        coord = coord.with_presence(ps);
    }

    // Wire agent store sync for hot-loading persistent agents.
    if let Ok(store) =
        crate::agent_store::AgentStore::persistent(config_path.parent().unwrap_or(workspace_root))
    {
        coord = coord.with_store_sync(crate::coordinator::StoreSyncConfig {
            store: Arc::new(store),
            config_path: config_path.to_path_buf(),
            workspace_root: workspace_root.to_path_buf(),
            interval_secs: 30,
        });
    }

    // ── Pass 1: Build all agents and create task channels ───────────────

    let mut built_agents: Vec<BuiltAgent> = Vec::new();
    let mut any_wants_converse = false;

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
            agent_store: None,
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

                let (task_tx, task_rx) = mpsc::channel::<TaskMessage>(32);
                let wants_converse = agent_cfg.allowed_tools.iter().any(|t| t == "converse");
                if wants_converse {
                    any_wants_converse = true;
                }

                built_agents.push(BuiltAgent {
                    descriptor,
                    agent,
                    workspace_root: workspace_root.to_string_lossy().to_string(),
                    task_tx,
                    task_rx,
                    wants_converse,
                    max_turns: agent_cfg.conversation.max_turns,
                    turn_timeout_secs: agent_cfg.conversation.turn_timeout_secs,
                });
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

    // ── Pass 2: Wire ConverseTool endpoints and register agents ─────────

    // Build endpoint map from all agents' task senders (shared across all
    // ConverseTool instances).
    let mut endpoints: HashMap<String, Arc<dyn AgentEndpoint>> = if any_wants_converse {
        built_agents
            .iter()
            .map(|ba| {
                let ep: Arc<dyn AgentEndpoint> = Arc::new(SwarmAgentEndpoint {
                    id: ba.descriptor.id.clone(),
                    task_tx: ba.task_tx.clone(),
                    timeout_secs: ba.turn_timeout_secs,
                });
                (ba.descriptor.id.clone(), ep)
            })
            .collect()
    } else {
        HashMap::new()
    };

    // ── Register external A2A agents as swarm endpoints ──────────────
    register_a2a_endpoints(config, &mut endpoints);

    for mut ba in built_agents {
        if ba.wants_converse {
            let converse = ConverseTool::new(endpoints.clone())
                .with_max_turns(ba.max_turns)
                .with_turn_timeout_secs(ba.turn_timeout_secs);
            ba.agent.add_tool(Box::new(converse));
            tracing::info!(
                agent = %ba.descriptor.id,
                max_turns = ba.max_turns,
                "injected ConverseTool"
            );
        }

        coord
            .register_agent_with_rx(
                ba.descriptor,
                ba.agent,
                ba.workspace_root,
                ba.task_tx,
                ba.task_rx,
            )
            .await;
    }

    Ok(Some((coord, shutdown_tx)))
}

// ─── A2A endpoint registration ──────────────────────────────────────────────

/// Register external A2A agents from config as swarm endpoints.
///
/// Each entry in `config.a2a.agents` becomes an `A2aAgentEndpoint` that can be
/// reached through the `ConverseTool`, allowing local agents to delegate tasks
/// to remote A2A-compatible agents.
fn register_a2a_endpoints(
    config: &AgentZeroConfig,
    endpoints: &mut HashMap<String, Arc<dyn AgentEndpoint>>,
) {
    if !config.a2a.enabled {
        return;
    }

    for (agent_id, agent_cfg) in &config.a2a.agents {
        if agent_cfg.url.is_empty() {
            tracing::warn!(
                agent = %agent_id,
                "skipping A2A agent with empty URL"
            );
            continue;
        }

        let endpoint = A2aAgentEndpoint::new(
            agent_id.clone(),
            agent_cfg.url.clone(),
            agent_cfg.auth_token.clone(),
            agent_cfg.timeout_secs,
        );

        tracing::info!(
            agent = %agent_id,
            url = %agent_cfg.url,
            timeout_secs = agent_cfg.timeout_secs,
            "registered external A2A agent endpoint"
        );

        endpoints.insert(agent_id.clone(), Arc::new(endpoint));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_config::{A2aAgentConfig, A2aConfig};

    fn make_config_with_a2a(
        enabled: bool,
        agents: HashMap<String, A2aAgentConfig>,
    ) -> AgentZeroConfig {
        AgentZeroConfig {
            a2a: A2aConfig { enabled, agents },
            ..Default::default()
        }
    }

    #[test]
    fn register_a2a_endpoints_adds_configured_agents() {
        let mut agents = HashMap::new();
        agents.insert(
            "remote-coder".to_string(),
            A2aAgentConfig {
                url: "https://coder.example.com".to_string(),
                auth_token: Some("tok-123".to_string()),
                timeout_secs: 60,
            },
        );
        agents.insert(
            "remote-writer".to_string(),
            A2aAgentConfig {
                url: "https://writer.example.com".to_string(),
                auth_token: None,
                timeout_secs: 120,
            },
        );
        let config = make_config_with_a2a(true, agents);

        let mut endpoints: HashMap<String, Arc<dyn AgentEndpoint>> = HashMap::new();
        register_a2a_endpoints(&config, &mut endpoints);

        assert_eq!(endpoints.len(), 2);
        assert!(endpoints.contains_key("remote-coder"));
        assert!(endpoints.contains_key("remote-writer"));
        assert_eq!(endpoints["remote-coder"].agent_id(), "remote-coder");
        assert_eq!(endpoints["remote-writer"].agent_id(), "remote-writer");
    }

    #[test]
    fn register_a2a_endpoints_skips_when_disabled() {
        let mut agents = HashMap::new();
        agents.insert(
            "remote-agent".to_string(),
            A2aAgentConfig {
                url: "https://agent.example.com".to_string(),
                auth_token: None,
                timeout_secs: 30,
            },
        );
        let config = make_config_with_a2a(false, agents);

        let mut endpoints: HashMap<String, Arc<dyn AgentEndpoint>> = HashMap::new();
        register_a2a_endpoints(&config, &mut endpoints);

        assert!(
            endpoints.is_empty(),
            "should not register when a2a is disabled"
        );
    }

    #[test]
    fn register_a2a_endpoints_skips_empty_url() {
        let mut agents = HashMap::new();
        agents.insert(
            "broken".to_string(),
            A2aAgentConfig {
                url: String::new(),
                auth_token: None,
                timeout_secs: 30,
            },
        );
        let config = make_config_with_a2a(true, agents);

        let mut endpoints: HashMap<String, Arc<dyn AgentEndpoint>> = HashMap::new();
        register_a2a_endpoints(&config, &mut endpoints);

        assert!(endpoints.is_empty(), "should skip agents with empty URL");
    }
}
