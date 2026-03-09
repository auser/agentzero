//! Async job tracking store for multi-agent orchestration.
//!
//! Each async job submission gets a [`RunId`] and is tracked here through its
//! lifecycle: `Pending → Running → Completed/Failed/Cancelled`.

use agentzero_core::{JobStatus, Lane, RunId};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Record for a single async job.
#[derive(Debug, Clone)]
pub struct JobRecord {
    pub run_id: RunId,
    pub status: JobStatus,
    pub agent_id: String,
    pub lane: Lane,
    pub parent_run_id: Option<RunId>,
    pub created_at: Instant,
    pub updated_at: Instant,
    /// Estimated token usage for this run (input + output).
    pub tokens_used: u64,
    /// Estimated cost in USD (micro-dollars for precision).
    pub cost_microdollars: u64,
    /// User-supplied metadata tags for filtering/grouping.
    pub tags: HashMap<String, String>,
}

/// Thread-safe store for tracking async agent runs.
#[derive(Debug, Clone)]
pub struct JobStore {
    jobs: Arc<RwLock<HashMap<RunId, JobRecord>>>,
    /// Channel that fires whenever a job's status changes.
    notify: Arc<tokio::sync::broadcast::Sender<(RunId, JobStatus)>>,
}

impl JobStore {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            notify: Arc::new(tx),
        }
    }

    /// Submit a new job. Returns the assigned [`RunId`].
    pub async fn submit(
        &self,
        agent_id: String,
        lane: Lane,
        parent_run_id: Option<RunId>,
    ) -> RunId {
        let run_id = RunId::new();
        let now = Instant::now();
        let record = JobRecord {
            run_id: run_id.clone(),
            status: JobStatus::Pending,
            agent_id,
            lane,
            parent_run_id,
            created_at: now,
            updated_at: now,
            tokens_used: 0,
            cost_microdollars: 0,
            tags: HashMap::new(),
        };
        self.jobs.write().await.insert(run_id.clone(), record);
        let _ = self.notify.send((run_id.clone(), JobStatus::Pending));
        run_id
    }

    /// Update the status of an existing job.
    pub async fn update_status(&self, run_id: &RunId, status: JobStatus) {
        let mut jobs = self.jobs.write().await;
        if let Some(record) = jobs.get_mut(run_id) {
            record.status = status.clone();
            record.updated_at = Instant::now();
        }
        drop(jobs);
        let _ = self.notify.send((run_id.clone(), status));
    }

    /// Get a snapshot of a job record.
    pub async fn get(&self, run_id: &RunId) -> Option<JobRecord> {
        self.jobs.read().await.get(run_id).cloned()
    }

    /// List all jobs for a given parent run (sub-agent tracking).
    pub async fn list_by_parent(&self, parent_run_id: &RunId) -> Vec<JobRecord> {
        self.jobs
            .read()
            .await
            .values()
            .filter(|r| r.parent_run_id.as_ref() == Some(parent_run_id))
            .cloned()
            .collect()
    }

    /// List all jobs in a given lane.
    pub async fn list_by_lane(&self, lane: &Lane) -> Vec<JobRecord> {
        self.jobs
            .read()
            .await
            .values()
            .filter(|r| &r.lane == lane)
            .cloned()
            .collect()
    }

    /// Cancel a running or pending job. Returns true if the job existed and
    /// was not already in a terminal state.
    pub async fn cancel(&self, run_id: &RunId) -> bool {
        let mut jobs = self.jobs.write().await;
        if let Some(record) = jobs.get_mut(run_id) {
            if record.status.is_terminal() {
                return false;
            }
            record.status = JobStatus::Cancelled;
            record.updated_at = Instant::now();
            drop(jobs);
            let _ = self.notify.send((run_id.clone(), JobStatus::Cancelled));
            true
        } else {
            false
        }
    }

    /// List all jobs, optionally filtered by status string.
    pub async fn list_all(&self, status_filter: Option<&str>) -> Vec<JobRecord> {
        let jobs = self.jobs.read().await;
        jobs.values()
            .filter(|r| match status_filter {
                None => true,
                Some("pending") => matches!(r.status, JobStatus::Pending),
                Some("running") => matches!(r.status, JobStatus::Running),
                Some("completed") => matches!(r.status, JobStatus::Completed { .. }),
                Some("failed") => matches!(r.status, JobStatus::Failed { .. }),
                Some("cancelled") => matches!(r.status, JobStatus::Cancelled),
                Some(_) => true, // unknown filter = no filtering
            })
            .cloned()
            .collect()
    }

    /// Update token usage and cost for a job.
    pub async fn update_usage(&self, run_id: &RunId, tokens_used: u64, cost_microdollars: u64) {
        let mut jobs = self.jobs.write().await;
        if let Some(record) = jobs.get_mut(run_id) {
            record.tokens_used = tokens_used;
            record.cost_microdollars = cost_microdollars;
            record.updated_at = Instant::now();
        }
    }

    /// Subscribe to job status changes.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<(RunId, JobStatus)> {
        self.notify.subscribe()
    }

    /// Remove completed/failed/cancelled jobs older than `max_age`.
    pub async fn gc_expired(&self, max_age: std::time::Duration) {
        let cutoff = Instant::now() - max_age;
        let mut jobs = self.jobs.write().await;
        jobs.retain(|_, record| !record.status.is_terminal() || record.updated_at > cutoff);
    }

    /// Total number of tracked jobs.
    pub async fn len(&self) -> usize {
        self.jobs.read().await.len()
    }

    /// Whether the store is empty.
    pub async fn is_empty(&self) -> bool {
        self.jobs.read().await.is_empty()
    }
}

impl Default for JobStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn submit_and_get_job() {
        let store = JobStore::new();
        let run_id = store
            .submit("researcher".to_string(), Lane::Main, None)
            .await;

        let record = store.get(&run_id).await.expect("should find job");
        assert_eq!(record.agent_id, "researcher");
        assert_eq!(record.status, JobStatus::Pending);
        assert!(record.parent_run_id.is_none());
    }

    #[tokio::test]
    async fn update_status_transitions() {
        let store = JobStore::new();
        let run_id = store.submit("writer".to_string(), Lane::Main, None).await;

        store.update_status(&run_id, JobStatus::Running).await;
        assert_eq!(store.get(&run_id).await.unwrap().status, JobStatus::Running);

        store
            .update_status(
                &run_id,
                JobStatus::Completed {
                    result: "done".to_string(),
                },
            )
            .await;
        let record = store.get(&run_id).await.unwrap();
        assert!(record.status.is_terminal());
    }

    #[tokio::test]
    async fn list_by_parent_filters_correctly() {
        let store = JobStore::new();
        let parent = RunId::new();
        let _child1 = store
            .submit(
                "scraper".to_string(),
                Lane::SubAgent {
                    parent_run_id: parent.clone(),
                    depth: 1,
                },
                Some(parent.clone()),
            )
            .await;
        let _child2 = store
            .submit(
                "analyzer".to_string(),
                Lane::SubAgent {
                    parent_run_id: parent.clone(),
                    depth: 1,
                },
                Some(parent.clone()),
            )
            .await;
        let _unrelated = store.submit("other".to_string(), Lane::Main, None).await;

        let children = store.list_by_parent(&parent).await;
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn gc_removes_old_terminal_jobs() {
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;
        store
            .update_status(
                &run_id,
                JobStatus::Completed {
                    result: "ok".to_string(),
                },
            )
            .await;

        // GC with zero max_age should remove it.
        store.gc_expired(std::time::Duration::ZERO).await;
        assert!(store.get(&run_id).await.is_none());
    }

    #[tokio::test]
    async fn subscribe_receives_status_updates() {
        let store = JobStore::new();
        let mut rx = store.subscribe();

        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;

        let (received_id, received_status) = rx.recv().await.unwrap();
        assert_eq!(received_id, run_id);
        assert_eq!(received_status, JobStatus::Pending);
    }

    #[tokio::test]
    async fn cancel_running_job() {
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;
        store.update_status(&run_id, JobStatus::Running).await;

        assert!(store.cancel(&run_id).await, "should cancel running job");
        let record = store.get(&run_id).await.unwrap();
        assert_eq!(record.status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn cancel_completed_job_returns_false() {
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;
        store
            .update_status(
                &run_id,
                JobStatus::Completed {
                    result: "done".to_string(),
                },
            )
            .await;

        assert!(
            !store.cancel(&run_id).await,
            "should not cancel completed job"
        );
    }

    #[tokio::test]
    async fn cancel_nonexistent_job_returns_false() {
        let store = JobStore::new();
        let fake = RunId("run-fake".to_string());
        assert!(!store.cancel(&fake).await);
    }

    #[tokio::test]
    async fn list_all_unfiltered() {
        let store = JobStore::new();
        store.submit("a".to_string(), Lane::Main, None).await;
        store.submit("b".to_string(), Lane::Main, None).await;
        let all = store.list_all(None).await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn list_all_filtered_by_status() {
        let store = JobStore::new();
        let r1 = store.submit("a".to_string(), Lane::Main, None).await;
        let _r2 = store.submit("b".to_string(), Lane::Main, None).await;
        store.update_status(&r1, JobStatus::Running).await;

        let running = store.list_all(Some("running")).await;
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].agent_id, "a");

        let pending = store.list_all(Some("pending")).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].agent_id, "b");
    }

    #[tokio::test]
    async fn update_usage_tracks_tokens_and_cost() {
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;
        store.update_usage(&run_id, 1500, 4200).await;

        let record = store.get(&run_id).await.unwrap();
        assert_eq!(record.tokens_used, 1500);
        assert_eq!(record.cost_microdollars, 4200);
    }
}
