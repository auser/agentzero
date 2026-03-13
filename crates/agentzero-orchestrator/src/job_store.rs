//! Async job tracking store for multi-agent orchestration.
//!
//! Each async job submission gets a [`RunId`] and is tracked here through its
//! lifecycle: `Pending → Running → Completed/Failed/Cancelled`.

use agentzero_core::event_bus::Event;
use agentzero_core::{EventBus, JobStatus, Lane, RunId};
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
    /// Agent that successfully claimed this job via [`JobStore::try_claim`].
    /// `None` until a claim succeeds.
    pub claimed_by: Option<String>,
    /// Organization that owns this job (multi-tenancy isolation).
    /// `None` for backward compatibility with single-tenant deployments.
    pub org_id: Option<String>,
}

/// Thread-safe store for tracking async agent runs.
#[derive(Clone)]
pub struct JobStore {
    jobs: Arc<RwLock<HashMap<RunId, JobRecord>>>,
    /// Channel that fires whenever a job's status changes.
    notify: Arc<tokio::sync::broadcast::Sender<(RunId, JobStatus)>>,
    /// Persistent event log for run lifecycle tracking.
    event_log: EventLog,
    /// Optional distributed event bus for cross-instance awareness.
    event_bus: Option<Arc<dyn EventBus>>,
}

impl std::fmt::Debug for JobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobStore")
            .field("event_bus", &self.event_bus.is_some())
            .finish()
    }
}

impl JobStore {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            notify: Arc::new(tx),
            event_log: EventLog::new(),
            event_bus: None,
        }
    }

    /// Set the distributed event bus for publishing job state transitions.
    pub fn with_event_bus(mut self, bus: Arc<dyn EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
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
        self.submit_for_org(agent_id, lane, parent_run_id, None)
            .await
    }

    /// Submit a new job scoped to an organization. Returns the assigned [`RunId`].
    pub async fn submit_for_org(
        &self,
        agent_id: String,
        lane: Lane,
        parent_run_id: Option<RunId>,
        org_id: Option<String>,
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
            claimed_by: None,
            org_id,
        };
        self.jobs.write().await.insert(run_id.clone(), record);
        let _ = self.notify.send((run_id.clone(), JobStatus::Pending));
        self.event_log.append(&run_id, EventKind::Created).await;

        // Publish to distributed event bus if configured.
        if let Some(ref bus) = self.event_bus {
            let payload = serde_json::json!({
                "run_id": run_id.as_str(),
                "status": "pending",
            })
            .to_string();
            let event = Event::new("job.pending", "job_store", payload);
            if let Err(e) = bus.publish(event).await {
                tracing::warn!(error = %e, "failed to publish job submit event");
            }
        }

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

        // Publish to distributed event bus if configured.
        if let Some(ref bus) = self.event_bus {
            let topic = format!("job.{}", status_to_topic(&status));
            let payload = serde_json::json!({
                "run_id": run_id.as_str(),
                "status": status_to_topic(&status),
            })
            .to_string();
            let event = Event::new(topic, "job_store", payload);
            if let Err(e) = bus.publish(event).await {
                tracing::warn!(error = %e, "failed to publish job status event");
            }
        }
    }

    /// Atomically transition a job from `Pending` to `Running`, recording the
    /// claiming agent. Returns `true` if this caller won the claim, `false` if
    /// the job was already claimed, running, or in a terminal state.
    ///
    /// Use this in Steer mode to prevent multiple agents from picking up the
    /// same work item.
    pub async fn try_claim(&self, run_id: &RunId, agent_id: &str) -> bool {
        let mut jobs = self.jobs.write().await;
        if let Some(record) = jobs.get_mut(run_id) {
            if record.status == JobStatus::Pending {
                record.status = JobStatus::Running;
                record.agent_id = agent_id.to_string();
                record.claimed_by = Some(agent_id.to_string());
                record.updated_at = Instant::now();
                drop(jobs);
                self.event_log.append(run_id, EventKind::Running).await;
                let _ = self.notify.send((run_id.clone(), JobStatus::Running));
                return true;
            }
        }
        false
    }

    /// Get a snapshot of a job record.
    pub async fn get(&self, run_id: &RunId) -> Option<JobRecord> {
        self.jobs.read().await.get(run_id).cloned()
    }

    /// Get a job record, but only if it belongs to the specified org.
    /// Returns `None` if the job doesn't exist or belongs to a different org.
    pub async fn get_for_org(&self, run_id: &RunId, org_id: &str) -> Option<JobRecord> {
        self.jobs
            .read()
            .await
            .get(run_id)
            .filter(|r| r.org_id.as_deref() == Some(org_id))
            .cloned()
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

    /// Emergency stop: cascade-cancel all active root-level runs.
    /// Returns the total list of run IDs that were cancelled (roots + descendants).
    pub async fn emergency_stop_all(&self) -> Vec<RunId> {
        self.emergency_stop_for_org(None).await
    }

    /// Emergency stop scoped to an organization. When `org_id` is `None`,
    /// cancels all active roots (backward-compatible).
    pub async fn emergency_stop_for_org(&self, org_id: Option<&str>) -> Vec<RunId> {
        let roots: Vec<RunId> = {
            let jobs = self.jobs.read().await;
            jobs.values()
                .filter(|r| r.parent_run_id.is_none() && !r.status.is_terminal())
                .filter(|r| match org_id {
                    Some(oid) => r.org_id.as_deref() == Some(oid),
                    None => true,
                })
                .map(|r| r.run_id.clone())
                .collect()
        };

        let mut all_cancelled = Vec::new();
        for root in roots {
            let cancelled = self.cascade_cancel(&root).await;
            all_cancelled.extend(cancelled);
        }
        all_cancelled
    }

    /// List all jobs, optionally filtered by status string.
    pub async fn list_all(&self, status_filter: Option<&str>) -> Vec<JobRecord> {
        self.list_all_for_org(status_filter, None).await
    }

    /// List jobs scoped to an organization, optionally filtered by status.
    /// When `org_id` is `None`, returns all jobs (backward-compatible).
    pub async fn list_all_for_org(
        &self,
        status_filter: Option<&str>,
        org_id: Option<&str>,
    ) -> Vec<JobRecord> {
        let jobs = self.jobs.read().await;
        jobs.values()
            .filter(|r| match org_id {
                Some(oid) => r.org_id.as_deref() == Some(oid),
                None => true,
            })
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

fn status_to_topic(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Pending => "pending",
        JobStatus::Running => "running",
        JobStatus::Completed { .. } => "completed",
        JobStatus::Failed { .. } => "failed",
        JobStatus::Cancelled => "cancelled",
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
    async fn emergency_stop_all_cancels_roots_and_children() {
        let store = JobStore::new();

        // Root 1 with a child.
        let r1 = store.submit("a".to_string(), Lane::Main, None).await;
        store.update_status(&r1, JobStatus::Running).await;
        let child = store
            .submit("a-child".to_string(), Lane::Main, Some(r1.clone()))
            .await;
        store.update_status(&child, JobStatus::Running).await;

        // Root 2 (running).
        let r2 = store.submit("b".to_string(), Lane::Main, None).await;
        store.update_status(&r2, JobStatus::Running).await;

        // Root 3 (already completed — should be skipped).
        let r3 = store.submit("c".to_string(), Lane::Main, None).await;
        store
            .update_status(
                &r3,
                JobStatus::Completed {
                    result: "done".to_string(),
                },
            )
            .await;

        let cancelled = store.emergency_stop_all().await;
        // r1 + child + r2 cancelled; r3 skipped.
        assert_eq!(cancelled.len(), 3);
        assert!(cancelled.contains(&r1));
        assert!(cancelled.contains(&child));
        assert!(cancelled.contains(&r2));
        assert!(!cancelled.contains(&r3));
    }

    #[tokio::test]
    async fn emergency_stop_all_empty_store() {
        let store = JobStore::new();
        let cancelled = store.emergency_stop_all().await;
        assert!(cancelled.is_empty());
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

    #[tokio::test]
    async fn try_claim_pending_job_succeeds() {
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;

        assert!(store.try_claim(&run_id, "claimer-1").await);
        let record = store.get(&run_id).await.unwrap();
        assert_eq!(record.status, JobStatus::Running);
        assert_eq!(record.agent_id, "claimer-1");
        assert_eq!(record.claimed_by.as_deref(), Some("claimer-1"));
    }

    #[tokio::test]
    async fn try_claim_already_running_fails() {
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;
        store.update_status(&run_id, JobStatus::Running).await;

        assert!(!store.try_claim(&run_id, "late-claimer").await);
    }

    #[tokio::test]
    async fn try_claim_terminal_job_fails() {
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

        assert!(!store.try_claim(&run_id, "claimer").await);
    }

    #[tokio::test]
    async fn try_claim_double_claim_second_fails() {
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;

        assert!(store.try_claim(&run_id, "first").await);
        assert!(!store.try_claim(&run_id, "second").await);

        let record = store.get(&run_id).await.unwrap();
        assert_eq!(record.claimed_by.as_deref(), Some("first"));
    }

    #[tokio::test]
    async fn try_claim_nonexistent_job_fails() {
        let store = JobStore::new();
        let fake = RunId("run-nope".to_string());
        assert!(!store.try_claim(&fake, "claimer").await);
    }

    // --- Org isolation tests (Phase F) ---

    #[tokio::test]
    async fn org_isolation_job_invisible_to_other_org() {
        let store = JobStore::new();
        let run_id = store
            .submit_for_org(
                "agent".to_string(),
                Lane::Main,
                None,
                Some("org-a".to_string()),
            )
            .await;

        // org-a can see it
        assert!(store.get_for_org(&run_id, "org-a").await.is_some());
        // org-b cannot
        assert!(store.get_for_org(&run_id, "org-b").await.is_none());
    }

    #[tokio::test]
    async fn list_all_for_org_filters_correctly() {
        let store = JobStore::new();
        store
            .submit_for_org("a".to_string(), Lane::Main, None, Some("org-a".to_string()))
            .await;
        store
            .submit_for_org("b".to_string(), Lane::Main, None, Some("org-b".to_string()))
            .await;
        store
            .submit_for_org("c".to_string(), Lane::Main, None, Some("org-a".to_string()))
            .await;

        let org_a_jobs = store.list_all_for_org(None, Some("org-a")).await;
        assert_eq!(org_a_jobs.len(), 2);
        assert!(org_a_jobs
            .iter()
            .all(|j| j.org_id.as_deref() == Some("org-a")));

        let org_b_jobs = store.list_all_for_org(None, Some("org-b")).await;
        assert_eq!(org_b_jobs.len(), 1);

        // None org_id returns all
        let all_jobs = store.list_all_for_org(None, None).await;
        assert_eq!(all_jobs.len(), 3);
    }

    #[tokio::test]
    async fn list_all_for_org_with_status_filter() {
        let store = JobStore::new();
        let r1 = store
            .submit_for_org("a".to_string(), Lane::Main, None, Some("org-a".to_string()))
            .await;
        store
            .submit_for_org("b".to_string(), Lane::Main, None, Some("org-a".to_string()))
            .await;
        store.update_status(&r1, JobStatus::Running).await;

        let running = store.list_all_for_org(Some("running"), Some("org-a")).await;
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].agent_id, "a");

        let pending = store.list_all_for_org(Some("pending"), Some("org-a")).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].agent_id, "b");
    }

    #[tokio::test]
    async fn emergency_stop_for_org_only_cancels_own_jobs() {
        let store = JobStore::new();
        let r_a = store
            .submit_for_org("a".to_string(), Lane::Main, None, Some("org-a".to_string()))
            .await;
        store.update_status(&r_a, JobStatus::Running).await;

        let r_b = store
            .submit_for_org("b".to_string(), Lane::Main, None, Some("org-b".to_string()))
            .await;
        store.update_status(&r_b, JobStatus::Running).await;

        let cancelled = store.emergency_stop_for_org(Some("org-a")).await;
        assert_eq!(cancelled.len(), 1);
        assert!(cancelled.contains(&r_a));

        // org-b's job should still be running
        assert_eq!(
            store
                .get(&r_b)
                .await
                .expect("org-b job should exist")
                .status,
            JobStatus::Running
        );
    }

    #[tokio::test]
    async fn submit_for_org_inherits_org_id() {
        let store = JobStore::new();
        let run_id = store
            .submit_for_org(
                "agent".to_string(),
                Lane::Main,
                None,
                Some("acme-corp".to_string()),
            )
            .await;
        let record = store.get(&run_id).await.expect("job should exist");
        assert_eq!(record.org_id.as_deref(), Some("acme-corp"));
    }

    #[tokio::test]
    async fn backward_compat_submit_has_no_org_id() {
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;
        let record = store.get(&run_id).await.expect("job should exist");
        assert!(record.org_id.is_none());
    }

    #[tokio::test]
    async fn event_bus_publishes_on_submit() {
        let bus = Arc::new(agentzero_core::InMemoryBus::default_capacity());
        let store = JobStore::new().with_event_bus(bus.clone());
        let mut sub = bus.subscribe();

        let _run_id = store.submit("agent".to_string(), Lane::Main, None).await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), sub.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(event.topic, "job.pending");
        assert!(event.payload.contains("pending"));
    }

    #[tokio::test]
    async fn event_bus_publishes_on_status_change() {
        let bus = Arc::new(agentzero_core::InMemoryBus::default_capacity());
        let store = JobStore::new().with_event_bus(bus.clone());
        let mut sub = bus.subscribe();

        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;

        // Consume the submit event.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), sub.recv())
            .await
            .expect("timeout")
            .expect("recv");

        // Transition to Running.
        store.update_status(&run_id, JobStatus::Running).await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), sub.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(event.topic, "job.running");

        // Transition to Completed.
        store
            .update_status(
                &run_id,
                JobStatus::Completed {
                    result: "done".to_string(),
                },
            )
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), sub.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(event.topic, "job.completed");
    }

    #[tokio::test]
    async fn no_event_bus_still_works() {
        // Verify backward compat: no event bus configured should not panic.
        let store = JobStore::new();
        let run_id = store.submit("agent".to_string(), Lane::Main, None).await;
        store.update_status(&run_id, JobStatus::Running).await;
        let record = store.get(&run_id).await.expect("job");
        assert_eq!(record.status, JobStatus::Running);
    }
}
