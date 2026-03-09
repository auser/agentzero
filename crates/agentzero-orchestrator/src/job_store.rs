//! Async job tracking store for multi-agent orchestration.
//!
//! Each async job submission gets a [`RunId`] and is tracked here through its
//! lifecycle: `Pending → Running → Completed/Failed/Cancelled`.

use agentzero_core::{JobStatus, Lane, RunId};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Kind of event that occurred during a run's lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Created,
    Running,
    ToolCall { name: String },
    ToolResult { name: String },
    Completed { summary: String },
    Failed { error: String },
    Cancelled,
}

/// A single event in a run's persistent event log.
#[derive(Debug, Clone)]
pub struct RunEvent {
    pub timestamp: Instant,
    pub run_id: RunId,
    pub kind: EventKind,
}

/// Append-only event log for run lifecycle tracking.
#[derive(Debug, Clone, Default)]
pub struct EventLog {
    events: Arc<RwLock<HashMap<RunId, Vec<RunEvent>>>>,
}

impl EventLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an event to a run's log.
    pub async fn append(&self, run_id: &RunId, kind: EventKind) {
        let event = RunEvent {
            timestamp: Instant::now(),
            run_id: run_id.clone(),
            kind,
        };
        self.events
            .write()
            .await
            .entry(run_id.clone())
            .or_default()
            .push(event);
    }

    /// Get all events for a run.
    pub async fn get(&self, run_id: &RunId) -> Vec<RunEvent> {
        self.events
            .read()
            .await
            .get(run_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Remove events for a run (used during GC).
    pub async fn remove(&self, run_id: &RunId) {
        self.events.write().await.remove(run_id);
    }
}

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
    /// Persistent event log for run lifecycle tracking.
    event_log: EventLog,
}

impl JobStore {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            notify: Arc::new(tx),
            event_log: EventLog::new(),
        }
    }

    /// Access the event log for recording tool calls and other events.
    pub fn event_log(&self) -> &EventLog {
        &self.event_log
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
        self.event_log.append(&run_id, EventKind::Created).await;
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
        let _ = self.notify.send((run_id.clone(), status.clone()));

        // Record in event log.
        let kind = match &status {
            JobStatus::Pending => EventKind::Created,
            JobStatus::Running => EventKind::Running,
            JobStatus::Completed { result } => EventKind::Completed {
                summary: result.clone(),
            },
            JobStatus::Failed { error } => EventKind::Failed {
                error: error.clone(),
            },
            JobStatus::Cancelled => EventKind::Cancelled,
        };
        self.event_log.append(run_id, kind).await;
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

    /// Cancel a job and all its descendants recursively (BFS).
    /// Returns the list of run IDs that were cancelled.
    pub async fn cascade_cancel(&self, run_id: &RunId) -> Vec<RunId> {
        let mut cancelled = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(run_id.clone());

        while let Some(current) = queue.pop_front() {
            // Cancel this job.
            let did_cancel = {
                let mut jobs = self.jobs.write().await;
                if let Some(record) = jobs.get_mut(&current) {
                    if !record.status.is_terminal() {
                        record.status = JobStatus::Cancelled;
                        record.updated_at = Instant::now();
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if did_cancel {
                let _ = self.notify.send((current.clone(), JobStatus::Cancelled));
                self.event_log.append(&current, EventKind::Cancelled).await;
                cancelled.push(current.clone());
            }

            // Find all children of this job.
            let children: Vec<RunId> = {
                let jobs = self.jobs.read().await;
                jobs.values()
                    .filter(|r| r.parent_run_id.as_ref() == Some(&current))
                    .filter(|r| !r.status.is_terminal())
                    .map(|r| r.run_id.clone())
                    .collect()
            };
            queue.extend(children);
        }

        cancelled
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

    /// Record a tool call event in the persistent event log.
    pub async fn record_tool_call(&self, run_id: &RunId, tool_name: &str) {
        self.event_log
            .append(
                run_id,
                EventKind::ToolCall {
                    name: tool_name.to_string(),
                },
            )
            .await;
    }

    /// Record a tool result event in the persistent event log.
    pub async fn record_tool_result(&self, run_id: &RunId, tool_name: &str) {
        self.event_log
            .append(
                run_id,
                EventKind::ToolResult {
                    name: tool_name.to_string(),
                },
            )
            .await;
    }

    /// Get the persistent event log for a run.
    pub async fn get_events(&self, run_id: &RunId) -> Vec<RunEvent> {
        self.event_log.get(run_id).await
    }

    /// Remove completed/failed/cancelled jobs older than `max_age`.
    pub async fn gc_expired(&self, max_age: std::time::Duration) {
        let cutoff = Instant::now() - max_age;
        let mut jobs = self.jobs.write().await;
        let expired: Vec<RunId> = jobs
            .iter()
            .filter(|(_, record)| record.status.is_terminal() && record.updated_at <= cutoff)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &expired {
            jobs.remove(id);
        }
        drop(jobs);
        // Clean up event logs for expired jobs.
        for id in &expired {
            self.event_log.remove(id).await;
        }
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

    #[tokio::test]
    async fn cascade_cancel_parent_and_children() {
        let store = JobStore::new();
        let parent = store.submit("parent".to_string(), Lane::Main, None).await;
        store.update_status(&parent, JobStatus::Running).await;

        let child1 = store
            .submit(
                "child1".to_string(),
                Lane::SubAgent {
                    parent_run_id: parent.clone(),
                    depth: 1,
                },
                Some(parent.clone()),
            )
            .await;
        store.update_status(&child1, JobStatus::Running).await;

        let child2 = store
            .submit(
                "child2".to_string(),
                Lane::SubAgent {
                    parent_run_id: parent.clone(),
                    depth: 1,
                },
                Some(parent.clone()),
            )
            .await;
        store.update_status(&child2, JobStatus::Running).await;

        let cancelled = store.cascade_cancel(&parent).await;
        assert_eq!(cancelled.len(), 3);

        assert_eq!(
            store.get(&parent).await.unwrap().status,
            JobStatus::Cancelled
        );
        assert_eq!(
            store.get(&child1).await.unwrap().status,
            JobStatus::Cancelled
        );
        assert_eq!(
            store.get(&child2).await.unwrap().status,
            JobStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn cascade_cancel_three_levels_deep() {
        let store = JobStore::new();
        let root = store.submit("root".to_string(), Lane::Main, None).await;
        store.update_status(&root, JobStatus::Running).await;

        let child = store
            .submit(
                "child".to_string(),
                Lane::SubAgent {
                    parent_run_id: root.clone(),
                    depth: 1,
                },
                Some(root.clone()),
            )
            .await;
        store.update_status(&child, JobStatus::Running).await;

        let grandchild = store
            .submit(
                "grandchild".to_string(),
                Lane::SubAgent {
                    parent_run_id: child.clone(),
                    depth: 2,
                },
                Some(child.clone()),
            )
            .await;
        store.update_status(&grandchild, JobStatus::Running).await;

        let cancelled = store.cascade_cancel(&root).await;
        assert_eq!(cancelled.len(), 3);
        assert_eq!(
            store.get(&grandchild).await.unwrap().status,
            JobStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn cascade_cancel_skips_already_terminal() {
        let store = JobStore::new();
        let parent = store.submit("parent".to_string(), Lane::Main, None).await;
        store.update_status(&parent, JobStatus::Running).await;

        let completed_child = store
            .submit(
                "done".to_string(),
                Lane::SubAgent {
                    parent_run_id: parent.clone(),
                    depth: 1,
                },
                Some(parent.clone()),
            )
            .await;
        store
            .update_status(
                &completed_child,
                JobStatus::Completed {
                    result: "ok".to_string(),
                },
            )
            .await;

        let running_child = store
            .submit(
                "running".to_string(),
                Lane::SubAgent {
                    parent_run_id: parent.clone(),
                    depth: 1,
                },
                Some(parent.clone()),
            )
            .await;
        store
            .update_status(&running_child, JobStatus::Running)
            .await;

        let cancelled = store.cascade_cancel(&parent).await;
        // Parent + running_child cancelled; completed_child skipped.
        assert_eq!(cancelled.len(), 2);
        assert!(cancelled.contains(&parent));
        assert!(cancelled.contains(&running_child));
    }

    #[tokio::test]
    async fn event_log_records_lifecycle() {
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;
        store.update_status(&run_id, JobStatus::Running).await;
        store.record_tool_call(&run_id, "read_file").await;
        store.record_tool_result(&run_id, "read_file").await;
        store
            .update_status(
                &run_id,
                JobStatus::Completed {
                    result: "done".to_string(),
                },
            )
            .await;

        let events = store.get_events(&run_id).await;
        assert_eq!(events.len(), 5); // Created, Running, ToolCall, ToolResult, Completed

        assert_eq!(events[0].kind, EventKind::Created);
        assert_eq!(events[1].kind, EventKind::Running);
        assert_eq!(
            events[2].kind,
            EventKind::ToolCall {
                name: "read_file".to_string()
            }
        );
        assert_eq!(
            events[3].kind,
            EventKind::ToolResult {
                name: "read_file".to_string()
            }
        );
        assert!(matches!(events[4].kind, EventKind::Completed { .. }));
    }

    #[tokio::test]
    async fn gc_also_removes_event_log() {
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;
        store.record_tool_call(&run_id, "test").await;
        store
            .update_status(
                &run_id,
                JobStatus::Completed {
                    result: "ok".to_string(),
                },
            )
            .await;

        store.gc_expired(std::time::Duration::ZERO).await;
        assert!(store.get(&run_id).await.is_none());
        assert!(store.get_events(&run_id).await.is_empty());
    }
}
