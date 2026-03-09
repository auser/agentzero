//! Lane-based work queuing for multi-agent orchestration.
//!
//! Lanes serialize or parallelize work items to prevent session collisions.
//! Each lane has its own bounded channel and configurable concurrency limit.
//!
//! - **Main**: Interactive user requests (serialized, one at a time).
//! - **Cron**: Scheduled jobs (parallel up to configured limit).
//! - **SubAgent**: Sub-agent work spawned by parent agents (parallel up to limit).

use agentzero_core::{Lane, QueueMode, RunId};
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};

/// Configuration for each lane's concurrency and queue capacity.
#[derive(Debug, Clone)]
pub struct LaneConfig {
    pub main_concurrency: usize,
    pub cron_concurrency: usize,
    pub subagent_concurrency: usize,
    pub queue_capacity: usize,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self {
            main_concurrency: 1,
            cron_concurrency: 3,
            subagent_concurrency: 5,
            queue_capacity: 64,
        }
    }
}

/// A work item submitted to a lane for processing.
#[derive(Debug)]
pub struct WorkItem {
    pub run_id: RunId,
    pub agent_id: String,
    pub message: String,
    pub lane: Lane,
    /// How this message should be routed within the lane.
    pub queue_mode: QueueMode,
    /// Channel to send the result back to the submitter.
    pub result_tx: Option<tokio::sync::oneshot::Sender<WorkResult>>,
}

/// Result of processing a work item.
#[derive(Debug)]
pub struct WorkResult {
    pub run_id: RunId,
    pub output: Result<String, String>,
}

/// Manages separate processing lanes with independent concurrency control.
#[derive(Clone)]
pub struct LaneManager {
    main_tx: mpsc::Sender<WorkItem>,
    cron_tx: mpsc::Sender<WorkItem>,
    subagent_tx: mpsc::Sender<WorkItem>,
    main_semaphore: Arc<Semaphore>,
    cron_semaphore: Arc<Semaphore>,
    subagent_semaphore: Arc<Semaphore>,
}

impl std::fmt::Debug for LaneManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaneManager")
            .field("main_permits", &self.main_semaphore.available_permits())
            .field("cron_permits", &self.cron_semaphore.available_permits())
            .field(
                "subagent_permits",
                &self.subagent_semaphore.available_permits(),
            )
            .finish()
    }
}

/// Receiver handles returned from [`LaneManager::new`].
/// The caller should spawn processing loops that drain these receivers.
pub struct LaneReceivers {
    pub main_rx: mpsc::Receiver<WorkItem>,
    pub cron_rx: mpsc::Receiver<WorkItem>,
    pub subagent_rx: mpsc::Receiver<WorkItem>,
}

impl LaneManager {
    /// Create a new lane manager with the given config.
    /// Returns the manager (for submitting work) and receivers (for processing work).
    pub fn new(config: &LaneConfig) -> (Self, LaneReceivers) {
        let (main_tx, main_rx) = mpsc::channel(config.queue_capacity);
        let (cron_tx, cron_rx) = mpsc::channel(config.queue_capacity);
        let (subagent_tx, subagent_rx) = mpsc::channel(config.queue_capacity);

        let manager = Self {
            main_tx,
            cron_tx,
            subagent_tx,
            main_semaphore: Arc::new(Semaphore::new(config.main_concurrency)),
            cron_semaphore: Arc::new(Semaphore::new(config.cron_concurrency)),
            subagent_semaphore: Arc::new(Semaphore::new(config.subagent_concurrency)),
        };

        let receivers = LaneReceivers {
            main_rx,
            cron_rx,
            subagent_rx,
        };

        (manager, receivers)
    }

    /// Submit a work item to the appropriate lane.
    /// Returns `Err` if the lane's queue is full.
    pub async fn submit(&self, item: WorkItem) -> Result<(), WorkItem> {
        let tx = match &item.lane {
            Lane::Main => &self.main_tx,
            Lane::Cron => &self.cron_tx,
            Lane::SubAgent { .. } => &self.subagent_tx,
        };
        tx.send(item).await.map_err(|e| e.0)
    }

    /// Acquire a concurrency permit for the given lane.
    /// This should be called before processing a work item.
    pub async fn acquire_permit(&self, lane: &Lane) -> tokio::sync::OwnedSemaphorePermit {
        let semaphore = match lane {
            Lane::Main => &self.main_semaphore,
            Lane::Cron => &self.cron_semaphore,
            Lane::SubAgent { .. } => &self.subagent_semaphore,
        };
        semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore should not be closed")
    }

    /// Try to acquire a concurrency permit without blocking.
    pub fn try_acquire_permit(&self, lane: &Lane) -> Option<tokio::sync::OwnedSemaphorePermit> {
        let semaphore = match lane {
            Lane::Main => &self.main_semaphore,
            Lane::Cron => &self.cron_semaphore,
            Lane::SubAgent { .. } => &self.subagent_semaphore,
        };
        semaphore.clone().try_acquire_owned().ok()
    }

    /// Available permits for a lane (for metrics/diagnostics).
    pub fn available_permits(&self, lane: &Lane) -> usize {
        match lane {
            Lane::Main => self.main_semaphore.available_permits(),
            Lane::Cron => self.cron_semaphore.available_permits(),
            Lane::SubAgent { .. } => self.subagent_semaphore.available_permits(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn submit_to_main_lane() {
        let config = LaneConfig::default();
        let (manager, mut receivers) = LaneManager::new(&config);

        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        let item = WorkItem {
            run_id: RunId::new(),
            agent_id: "test-agent".to_string(),
            message: "hello".to_string(),
            lane: Lane::Main,
            queue_mode: QueueMode::default(),
            result_tx: Some(result_tx),
        };

        manager.submit(item).await.expect("should submit");

        // Receive from main lane.
        let received = receivers.main_rx.recv().await.expect("should receive");
        assert_eq!(received.agent_id, "test-agent");
        assert_eq!(received.message, "hello");

        // Send result back.
        let _ = received.result_tx.unwrap().send(WorkResult {
            run_id: received.run_id,
            output: Ok("world".to_string()),
        });

        let result = result_rx.await.expect("should get result");
        assert_eq!(result.output, Ok("world".to_string()));
    }

    #[tokio::test]
    async fn submit_to_subagent_lane() {
        let config = LaneConfig::default();
        let (manager, mut receivers) = LaneManager::new(&config);

        let parent = RunId::new();
        let item = WorkItem {
            run_id: RunId::new(),
            agent_id: "sub-agent".to_string(),
            message: "subtask".to_string(),
            lane: Lane::SubAgent {
                parent_run_id: parent.clone(),
                depth: 1,
            },
            queue_mode: QueueMode::default(),
            result_tx: None,
        };

        manager.submit(item).await.expect("should submit");

        let received = receivers.subagent_rx.recv().await.expect("should receive");
        assert_eq!(received.agent_id, "sub-agent");
    }

    #[tokio::test]
    async fn concurrency_limits_are_enforced() {
        let config = LaneConfig {
            main_concurrency: 1,
            ..Default::default()
        };
        let (manager, _receivers) = LaneManager::new(&config);

        // Acquire the single main permit.
        let permit = manager.try_acquire_permit(&Lane::Main);
        assert!(permit.is_some(), "first permit should succeed");

        // Second attempt should fail.
        let permit2 = manager.try_acquire_permit(&Lane::Main);
        assert!(permit2.is_none(), "second permit should fail (at capacity)");

        // Drop first permit — third attempt should succeed.
        drop(permit);
        let permit3 = manager.try_acquire_permit(&Lane::Main);
        assert!(
            permit3.is_some(),
            "third permit should succeed after release"
        );
    }

    #[tokio::test]
    async fn available_permits_reflects_state() {
        let config = LaneConfig {
            subagent_concurrency: 3,
            ..Default::default()
        };
        let (manager, _receivers) = LaneManager::new(&config);

        let lane = Lane::SubAgent {
            parent_run_id: RunId::new(),
            depth: 1,
        };

        assert_eq!(manager.available_permits(&lane), 3);

        let _p1 = manager.acquire_permit(&lane).await;
        assert_eq!(manager.available_permits(&lane), 2);

        let _p2 = manager.acquire_permit(&lane).await;
        assert_eq!(manager.available_permits(&lane), 1);
    }
}
