//! Dead agent recovery for swarm execution.
//!
//! Monitors agent heartbeats via [`PresenceStore`] and automatically
//! recovers from agent failures by destroying sandboxes and marking
//! tasks for re-dispatch.

use std::sync::Arc;
use std::time::Duration;

use agentzero_core::event_bus::{Event, EventBus};
use serde::{Deserialize, Serialize};

use crate::presence::{PresenceStatus, PresenceStore};
use crate::sandbox::{AgentSandbox, SandboxHandle};

// ── Types ────────────────────────────────────────────────────────────────────

/// Configuration for dead agent recovery.
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    /// How often to check for dead agents.
    pub check_interval: Duration,
    /// Maximum number of recovery attempts per agent before giving up.
    pub max_retries: usize,
    /// Default heartbeat TTL for agents (used if not specified per-agent).
    pub default_ttl: Duration,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(10),
            max_retries: 3,
            default_ttl: Duration::from_secs(60),
        }
    }
}

/// An action the recovery system decided to take.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryAction {
    /// The node ID of the dead agent.
    pub node_id: String,
    /// Agent name.
    pub agent_name: String,
    /// What recovery action was taken.
    pub action: RecoveryActionType,
    /// Number of previous recovery attempts for this agent.
    pub attempt: usize,
}

/// Type of recovery action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryActionType {
    /// Sandbox destroyed, task reset to pending for re-dispatch.
    Reassigned,
    /// Max retries exceeded, agent permanently failed.
    Abandoned,
}

// ── RecoveryMonitor ──────────────────────────────────────────────────────────

/// Monitors agent presence and triggers recovery for dead agents.
///
/// Used by the swarm supervisor to detect and recover from agent failures.
/// Does not run its own loop — instead provides `check_and_recover()` which
/// should be called periodically by the supervisor.
pub struct RecoveryMonitor {
    presence: Arc<PresenceStore>,
    event_bus: Option<Arc<dyn EventBus>>,
    config: RecoveryConfig,
    /// Track retry counts per node_id.
    retry_counts: std::collections::HashMap<String, usize>,
}

impl RecoveryMonitor {
    /// Create a new recovery monitor.
    pub fn new(presence: Arc<PresenceStore>, config: RecoveryConfig) -> Self {
        Self {
            presence,
            event_bus: None,
            config,
            retry_counts: std::collections::HashMap::new(),
        }
    }

    /// Attach an event bus for publishing recovery events.
    pub fn with_event_bus(mut self, bus: Arc<dyn EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Register an agent with the presence store using the configured TTL.
    pub async fn register_agent(&self, agent_id: &str, ttl: Option<Duration>) {
        let ttl = ttl.unwrap_or(self.config.default_ttl);
        self.presence.register(agent_id, ttl).await;
    }

    /// Send a heartbeat for an agent.
    pub async fn heartbeat(&self, agent_id: &str) {
        self.presence.heartbeat(agent_id).await;
    }

    /// Deregister an agent (completed normally).
    pub async fn deregister(&mut self, agent_id: &str) {
        self.presence.deregister(agent_id).await;
        self.retry_counts.remove(agent_id);
    }

    /// Check all registered agents for dead status and take recovery actions.
    ///
    /// For each dead agent:
    /// - If retries remain: destroy sandbox, emit `swarm.agent.reassigned`, return `Reassigned`
    /// - If max retries exceeded: emit `swarm.agent.abandoned`, return `Abandoned`
    ///
    /// The caller (supervisor) is responsible for actually re-dispatching reassigned tasks.
    pub async fn check_and_recover(
        &mut self,
        sandboxes: &std::collections::HashMap<String, SandboxHandle>,
        sandbox_backend: &dyn AgentSandbox,
        agent_names: &std::collections::HashMap<String, String>,
    ) -> Vec<RecoveryAction> {
        let all = self.presence.list_all().await;
        let mut actions = Vec::new();
        // Collect events to publish after the mutable borrow on retry_counts.
        let mut events: Vec<(String, String, String, usize)> = Vec::new();
        let mut to_deregister = Vec::new();

        for record in all {
            if record.status != PresenceStatus::Dead {
                continue;
            }

            let node_id = record.agent_id.clone();
            let agent_name = agent_names
                .get(&node_id)
                .cloned()
                .unwrap_or_else(|| node_id.clone());

            let attempt = self.retry_counts.entry(node_id.clone()).or_insert(0);
            *attempt += 1;
            let attempt_val = *attempt;

            if attempt_val > self.config.max_retries {
                actions.push(RecoveryAction {
                    node_id: node_id.clone(),
                    agent_name: agent_name.clone(),
                    action: RecoveryActionType::Abandoned,
                    attempt: attempt_val,
                });
                events.push((
                    "swarm.agent.abandoned".into(),
                    node_id.clone(),
                    agent_name,
                    attempt_val,
                ));
                to_deregister.push(node_id);
                continue;
            }

            // Destroy the sandbox if it exists.
            if let Some(handle) = sandboxes.get(&node_id) {
                if let Err(e) = sandbox_backend.destroy(handle).await {
                    tracing::warn!(
                        node_id = %node_id,
                        error = %e,
                        "failed to destroy sandbox during recovery"
                    );
                }
            }

            actions.push(RecoveryAction {
                node_id: node_id.clone(),
                agent_name: agent_name.clone(),
                action: RecoveryActionType::Reassigned,
                attempt: attempt_val,
            });
            events.push((
                "swarm.agent.reassigned".into(),
                node_id.clone(),
                agent_name,
                attempt_val,
            ));
            to_deregister.push(node_id);
        }

        // Publish events and deregister outside the retry_counts borrow.
        for (topic, node_id, agent_name, attempt) in events {
            self.publish_event(&topic, &node_id, &agent_name, attempt)
                .await;
        }
        for node_id in to_deregister {
            self.presence.deregister(&node_id).await;
        }

        actions
    }

    async fn publish_event(&self, topic: &str, node_id: &str, agent_name: &str, attempt: usize) {
        if let Some(ref bus) = self.event_bus {
            let payload = serde_json::json!({
                "node_id": node_id,
                "agent_name": agent_name,
                "attempt": attempt,
                "max_retries": self.config.max_retries,
            });
            let event = Event::new(topic, "swarm.recovery", payload.to_string());
            let _ = bus.publish(event).await;
        }
    }

    /// Get the current retry count for an agent.
    pub fn retry_count(&self, node_id: &str) -> usize {
        self.retry_counts.get(node_id).copied().unwrap_or(0)
    }

    /// Get the recovery config.
    pub fn config(&self) -> &RecoveryConfig {
        &self.config
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::WorktreeSandbox;
    use std::collections::HashMap;

    fn sandbox_backend() -> WorktreeSandbox {
        WorktreeSandbox::new(std::path::PathBuf::from("/tmp/unused"))
    }

    #[tokio::test]
    async fn alive_agent_not_recovered() {
        let presence = Arc::new(PresenceStore::new());
        let config = RecoveryConfig {
            max_retries: 3,
            default_ttl: Duration::from_secs(60),
            ..Default::default()
        };
        let mut monitor = RecoveryMonitor::new(Arc::clone(&presence), config);

        monitor.register_agent("n1", None).await;
        monitor.heartbeat("n1").await;

        let actions = monitor
            .check_and_recover(&HashMap::new(), &sandbox_backend(), &HashMap::new())
            .await;
        assert!(actions.is_empty(), "alive agent should not be recovered");
    }

    #[tokio::test]
    async fn dead_agent_gets_reassigned() {
        let presence = Arc::new(PresenceStore::new());
        let config = RecoveryConfig {
            max_retries: 3,
            default_ttl: Duration::from_millis(1),
            ..Default::default()
        };
        let mut monitor = RecoveryMonitor::new(Arc::clone(&presence), config);

        monitor
            .register_agent("n1", Some(Duration::from_millis(1)))
            .await;

        // Wait for agent to die (>2x TTL).
        tokio::time::sleep(Duration::from_millis(5)).await;

        let mut names = HashMap::new();
        names.insert("n1".to_string(), "test-agent".to_string());

        let actions = monitor
            .check_and_recover(&HashMap::new(), &sandbox_backend(), &names)
            .await;

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].node_id, "n1");
        assert_eq!(actions[0].agent_name, "test-agent");
        assert_eq!(actions[0].action, RecoveryActionType::Reassigned);
        assert_eq!(actions[0].attempt, 1);
    }

    #[tokio::test]
    async fn max_retries_triggers_abandon() {
        let presence = Arc::new(PresenceStore::new());
        let config = RecoveryConfig {
            max_retries: 2,
            default_ttl: Duration::from_millis(1),
            ..Default::default()
        };
        let mut monitor = RecoveryMonitor::new(Arc::clone(&presence), config);

        let names = HashMap::new();
        let sandboxes = HashMap::new();
        let backend = sandbox_backend();

        // Simulate 3 deaths (exceeds max_retries of 2).
        for i in 1..=3 {
            monitor
                .register_agent("n1", Some(Duration::from_millis(1)))
                .await;
            tokio::time::sleep(Duration::from_millis(5)).await;

            let actions = monitor
                .check_and_recover(&sandboxes, &backend, &names)
                .await;

            assert_eq!(actions.len(), 1, "iteration {i}");
            if i <= 2 {
                assert_eq!(actions[0].action, RecoveryActionType::Reassigned);
            } else {
                assert_eq!(actions[0].action, RecoveryActionType::Abandoned);
            }
        }
    }

    #[tokio::test]
    async fn deregister_clears_retry_count() {
        let presence = Arc::new(PresenceStore::new());
        let config = RecoveryConfig::default();
        let mut monitor = RecoveryMonitor::new(Arc::clone(&presence), config);

        monitor.register_agent("n1", None).await;
        // Simulate a recovery that incremented retry count.
        monitor.retry_counts.insert("n1".to_string(), 2);

        monitor.deregister("n1").await;
        assert_eq!(monitor.retry_count("n1"), 0);
    }

    #[tokio::test]
    async fn event_bus_publishes_reassigned() {
        let bus = Arc::new(agentzero_core::InMemoryBus::default_capacity());
        let presence = Arc::new(PresenceStore::new());
        let config = RecoveryConfig {
            max_retries: 3,
            default_ttl: Duration::from_millis(1),
            ..Default::default()
        };
        let mut monitor =
            RecoveryMonitor::new(Arc::clone(&presence), config).with_event_bus(bus.clone());

        let mut sub = bus.subscribe();

        monitor
            .register_agent("n1", Some(Duration::from_millis(1)))
            .await;
        tokio::time::sleep(Duration::from_millis(5)).await;

        monitor
            .check_and_recover(&HashMap::new(), &sandbox_backend(), &HashMap::new())
            .await;

        let event = tokio::time::timeout(Duration::from_secs(1), sub.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(event.topic, "swarm.agent.reassigned");
        assert!(event.payload.contains("\"attempt\":1"));
    }

    #[tokio::test]
    async fn heartbeat_keeps_agent_alive() {
        let presence = Arc::new(PresenceStore::new());
        let config = RecoveryConfig {
            max_retries: 3,
            default_ttl: Duration::from_millis(50),
            ..Default::default()
        };
        let mut monitor = RecoveryMonitor::new(Arc::clone(&presence), config);

        monitor
            .register_agent("n1", Some(Duration::from_millis(50)))
            .await;

        // Heartbeat before TTL expires.
        tokio::time::sleep(Duration::from_millis(20)).await;
        monitor.heartbeat("n1").await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        monitor.heartbeat("n1").await;

        let actions = monitor
            .check_and_recover(&HashMap::new(), &sandbox_backend(), &HashMap::new())
            .await;
        assert!(actions.is_empty(), "heartbeating agent should stay alive");
    }
}
