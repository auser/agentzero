//! Agent presence tracking with TTL-based liveness detection.
//!
//! Each agent worker heartbeats periodically. The presence store tracks the
//! last heartbeat time and reports agents as alive, stale, or dead based on
//! configurable TTL thresholds.

use agentzero_core::event_bus::Event;
use agentzero_core::EventBus;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Liveness status of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceStatus {
    /// Agent heartbeated within the TTL.
    Alive,
    /// Agent heartbeat is overdue but within 2x TTL (may be slow).
    Stale,
    /// Agent hasn't heartbeated in >2x TTL — presumed dead.
    Dead,
}

/// Per-agent presence record.
#[derive(Debug, Clone)]
pub struct PresenceRecord {
    pub agent_id: String,
    pub last_heartbeat: Instant,
    pub ttl: Duration,
    pub status: PresenceStatus,
}

/// Thread-safe store for tracking agent presence.
#[derive(Clone)]
pub struct PresenceStore {
    records: Arc<RwLock<HashMap<String, (Instant, Duration)>>>,
    /// Optional distributed event bus for publishing heartbeat events.
    event_bus: Option<Arc<dyn EventBus>>,
}

impl std::fmt::Debug for PresenceStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresenceStore")
            .field("event_bus", &self.event_bus.is_some())
            .finish()
    }
}

impl PresenceStore {
    pub fn new() -> Self {
        Self {
            records: Arc::new(RwLock::new(HashMap::new())),
            event_bus: None,
        }
    }

    /// Set the distributed event bus for publishing heartbeat events.
    pub fn with_event_bus(mut self, bus: Arc<dyn EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Register an agent with an initial heartbeat and TTL.
    pub async fn register(&self, agent_id: &str, ttl: Duration) {
        self.records
            .write()
            .await
            .insert(agent_id.to_string(), (Instant::now(), ttl));
    }

    /// Update an agent's heartbeat timestamp.
    pub async fn heartbeat(&self, agent_id: &str) {
        let mut records = self.records.write().await;
        if let Some(entry) = records.get_mut(agent_id) {
            entry.0 = Instant::now();
        }
        drop(records);

        // Publish heartbeat to distributed event bus if configured.
        if let Some(ref bus) = self.event_bus {
            let event = Event::new(
                "presence.heartbeat",
                agent_id,
                serde_json::json!({ "agent_id": agent_id }).to_string(),
            );
            let _ = bus.publish(event).await;
        }
    }

    /// Check if an agent is alive.
    pub async fn is_alive(&self, agent_id: &str) -> bool {
        self.status(agent_id).await == Some(PresenceStatus::Alive)
    }

    /// Get the presence status of an agent.
    pub async fn status(&self, agent_id: &str) -> Option<PresenceStatus> {
        let records = self.records.read().await;
        records.get(agent_id).map(|(last, ttl)| {
            let elapsed = last.elapsed();
            if elapsed <= *ttl {
                PresenceStatus::Alive
            } else if elapsed <= *ttl * 2 {
                PresenceStatus::Stale
            } else {
                PresenceStatus::Dead
            }
        })
    }

    /// List all agents with their presence status.
    pub async fn list_all(&self) -> Vec<PresenceRecord> {
        let records = self.records.read().await;
        records
            .iter()
            .map(|(id, (last, ttl))| {
                let elapsed = last.elapsed();
                let status = if elapsed <= *ttl {
                    PresenceStatus::Alive
                } else if elapsed <= *ttl * 2 {
                    PresenceStatus::Stale
                } else {
                    PresenceStatus::Dead
                };
                PresenceRecord {
                    agent_id: id.clone(),
                    last_heartbeat: *last,
                    ttl: *ttl,
                    status,
                }
            })
            .collect()
    }

    /// Remove agents that have been dead for longer than their TTL * 3.
    pub async fn gc_expired(&self) {
        let mut records = self.records.write().await;
        records.retain(|_, (last, ttl)| last.elapsed() <= *ttl * 3);
    }

    /// Remove an agent from the store (deregistration).
    pub async fn deregister(&self, agent_id: &str) {
        self.records.write().await.remove(agent_id);
    }
}

impl Default for PresenceStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_check_alive() {
        let store = PresenceStore::new();
        store.register("agent-1", Duration::from_secs(30)).await;

        assert!(store.is_alive("agent-1").await);
        assert_eq!(store.status("agent-1").await, Some(PresenceStatus::Alive));
    }

    #[tokio::test]
    async fn unknown_agent_returns_none() {
        let store = PresenceStore::new();
        assert_eq!(store.status("ghost").await, None);
        assert!(!store.is_alive("ghost").await);
    }

    #[tokio::test]
    async fn heartbeat_refreshes_timestamp() {
        let store = PresenceStore::new();
        store.register("agent-1", Duration::from_secs(30)).await;

        // Heartbeat should keep the agent alive.
        store.heartbeat("agent-1").await;
        assert!(store.is_alive("agent-1").await);
    }

    #[tokio::test]
    async fn stale_after_ttl() {
        let store = PresenceStore::new();
        // Use a very short TTL that will expire before we check.
        store.register("agent-1", Duration::from_millis(1)).await;

        // Wait for TTL to expire but stay within 2x.
        tokio::time::sleep(Duration::from_millis(2)).await;
        // Status should be Stale or Dead depending on timing.
        let status = store.status("agent-1").await.unwrap();
        assert!(
            status == PresenceStatus::Stale || status == PresenceStatus::Dead,
            "expected Stale or Dead, got {status:?}"
        );
    }

    #[tokio::test]
    async fn list_all_returns_all_agents() {
        let store = PresenceStore::new();
        store.register("a", Duration::from_secs(30)).await;
        store.register("b", Duration::from_secs(30)).await;

        let all = store.list_all().await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn deregister_removes_agent() {
        let store = PresenceStore::new();
        store.register("agent-1", Duration::from_secs(30)).await;
        store.deregister("agent-1").await;

        assert_eq!(store.status("agent-1").await, None);
    }

    #[tokio::test]
    async fn gc_expired_removes_dead_agents() {
        let store = PresenceStore::new();
        store.register("agent-1", Duration::from_millis(1)).await;

        // Wait for 3x TTL.
        tokio::time::sleep(Duration::from_millis(5)).await;
        store.gc_expired().await;

        assert_eq!(store.status("agent-1").await, None);
    }

    #[tokio::test]
    async fn event_bus_publishes_on_heartbeat() {
        let bus = Arc::new(agentzero_core::InMemoryBus::default_capacity());
        let store = PresenceStore::new().with_event_bus(bus.clone());
        let mut sub = bus.subscribe();

        store.register("agent-1", Duration::from_secs(30)).await;
        store.heartbeat("agent-1").await;

        let event = tokio::time::timeout(Duration::from_secs(1), sub.recv())
            .await
            .expect("timeout waiting for heartbeat event")
            .expect("recv");
        assert_eq!(event.topic, "presence.heartbeat");
        assert!(event.payload.contains("agent-1"));
    }

    #[tokio::test]
    async fn no_event_bus_heartbeat_still_works() {
        let store = PresenceStore::new();
        store.register("agent-1", Duration::from_secs(30)).await;
        store.heartbeat("agent-1").await;
        assert!(store.is_alive("agent-1").await);
    }
}
