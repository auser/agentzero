//! Cross-agent context awareness for swarm execution.
//!
//! Tracks running agents' assignments so that parallel agents can be aware of
//! what their siblings are working on, preventing file conflicts and enabling
//! collaboration. Publishes completion events and detects file overlap.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use agentzero_core::event_bus::{Event, EventBus};

// ── Types ────────────────────────────────────────────────────────────────────

/// Status of an agent in the swarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAssignmentStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

/// An agent's assignment in the swarm — what it's working on and where.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAssignment {
    /// Node ID in the workflow graph.
    pub node_id: String,
    /// Human-readable agent name.
    pub name: String,
    /// Description of the task this agent is performing.
    pub task_description: String,
    /// Estimated file paths this agent might modify.
    pub estimated_file_scopes: Vec<String>,
    /// Current status.
    pub status: AgentAssignmentStatus,
    /// Files actually modified (populated after completion).
    pub files_modified: Vec<String>,
}

/// Context summary injected into an agent's prompt before execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiblingContext {
    /// Descriptions of what sibling agents are doing.
    pub siblings: Vec<SiblingInfo>,
    /// Files that might conflict with this agent's work.
    pub potential_conflicts: Vec<String>,
}

/// Summary of a sibling agent's assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiblingInfo {
    pub name: String,
    pub task: String,
    pub file_scopes: Vec<String>,
    pub status: AgentAssignmentStatus,
}

// ── SwarmContext ──────────────────────────────────────────────────────────────

/// Tracks all agent assignments in a swarm execution for cross-agent awareness.
#[derive(Clone)]
pub struct SwarmContext {
    assignments: Arc<Mutex<HashMap<String, AgentAssignment>>>,
    event_bus: Option<Arc<dyn EventBus>>,
    workflow_id: String,
}

impl SwarmContext {
    /// Create a new swarm context for the given workflow.
    pub fn new(workflow_id: impl Into<String>) -> Self {
        Self {
            assignments: Arc::new(Mutex::new(HashMap::new())),
            event_bus: None,
            workflow_id: workflow_id.into(),
        }
    }

    /// Attach an event bus for publishing completion events.
    pub fn with_event_bus(mut self, bus: Arc<dyn EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Register an agent's assignment.
    pub async fn register(
        &self,
        node_id: impl Into<String>,
        name: impl Into<String>,
        task_description: impl Into<String>,
        estimated_file_scopes: Vec<String>,
    ) {
        let node_id = node_id.into();
        let assignment = AgentAssignment {
            node_id: node_id.clone(),
            name: name.into(),
            task_description: task_description.into(),
            estimated_file_scopes,
            status: AgentAssignmentStatus::Pending,
            files_modified: Vec::new(),
        };
        let mut assignments = self.assignments.lock().await;
        assignments.insert(node_id, assignment);
    }

    /// Mark an agent as running.
    pub async fn mark_running(&self, node_id: &str) {
        let mut assignments = self.assignments.lock().await;
        if let Some(a) = assignments.get_mut(node_id) {
            a.status = AgentAssignmentStatus::Running;
        }
    }

    /// Mark an agent as completed and record its modified files.
    ///
    /// Publishes a `swarm.agent.completed` event via the event bus.
    pub async fn mark_completed(&self, node_id: &str, files_modified: Vec<String>) {
        let agent_name;
        {
            let mut assignments = self.assignments.lock().await;
            if let Some(a) = assignments.get_mut(node_id) {
                a.status = AgentAssignmentStatus::Completed;
                a.files_modified = files_modified.clone();
                agent_name = a.name.clone();
            } else {
                return;
            }
        }

        // Publish completion event.
        if let Some(ref bus) = self.event_bus {
            let payload = serde_json::json!({
                "workflow_id": self.workflow_id,
                "node_id": node_id,
                "agent_name": agent_name,
                "files_modified": files_modified,
            });
            let event = Event::new(
                "swarm.agent.completed",
                format!("swarm/{}", self.workflow_id),
                payload.to_string(),
            );
            if let Err(e) = bus.publish(event).await {
                tracing::warn!(
                    node_id = %node_id,
                    error = %e,
                    "failed to publish agent completion event"
                );
            }
        }
    }

    /// Mark an agent as failed.
    ///
    /// Publishes a `swarm.agent.failed` event via the event bus.
    pub async fn mark_failed(&self, node_id: &str, error: &str) {
        let agent_name;
        {
            let mut assignments = self.assignments.lock().await;
            if let Some(a) = assignments.get_mut(node_id) {
                a.status = AgentAssignmentStatus::Failed;
                agent_name = a.name.clone();
            } else {
                return;
            }
        }

        if let Some(ref bus) = self.event_bus {
            let payload = serde_json::json!({
                "workflow_id": self.workflow_id,
                "node_id": node_id,
                "agent_name": agent_name,
                "error": error,
            });
            let event = Event::new(
                "swarm.agent.failed",
                format!("swarm/{}", self.workflow_id),
                payload.to_string(),
            );
            let _ = bus.publish(event).await;
        }
    }

    /// Build a sibling context for the given agent.
    ///
    /// Returns information about other running/pending agents and any
    /// potential file conflicts based on estimated file scopes.
    pub async fn sibling_context(&self, node_id: &str) -> SiblingContext {
        let assignments = self.assignments.lock().await;

        let this_scopes: HashSet<&str> = assignments
            .get(node_id)
            .map(|a| a.estimated_file_scopes.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        let mut siblings = Vec::new();
        let mut potential_conflicts = Vec::new();

        for (id, assignment) in assignments.iter() {
            if id == node_id {
                continue;
            }

            // Only include running or pending siblings.
            if !matches!(
                assignment.status,
                AgentAssignmentStatus::Running | AgentAssignmentStatus::Pending
            ) {
                continue;
            }

            siblings.push(SiblingInfo {
                name: assignment.name.clone(),
                task: assignment.task_description.clone(),
                file_scopes: assignment.estimated_file_scopes.clone(),
                status: assignment.status,
            });

            // Check for file scope overlap.
            for scope in &assignment.estimated_file_scopes {
                if this_scopes.contains(scope.as_str()) {
                    potential_conflicts.push(scope.clone());
                }
            }
        }

        SiblingContext {
            siblings,
            potential_conflicts,
        }
    }

    /// Format sibling context as a text block for injection into an agent's prompt.
    pub async fn format_context_for_prompt(&self, node_id: &str) -> Option<String> {
        let ctx = self.sibling_context(node_id).await;

        if ctx.siblings.is_empty() {
            return None;
        }

        let mut lines = vec![
            "## Swarm Awareness — Parallel Agents".to_string(),
            String::new(),
            "The following agents are running in parallel with you:".to_string(),
        ];

        for sib in &ctx.siblings {
            let scopes = if sib.file_scopes.is_empty() {
                "unknown".to_string()
            } else {
                sib.file_scopes.join(", ")
            };
            lines.push(format!(
                "- **{}** ({:?}): {} (files: {})",
                sib.name, sib.status, sib.task, scopes
            ));
        }

        if !ctx.potential_conflicts.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "**Warning**: Potential file conflicts with your scope: {}",
                ctx.potential_conflicts.join(", ")
            ));
            lines.push(
                "Coordinate carefully to avoid editing the same lines in these files.".to_string(),
            );
        }

        Some(lines.join("\n"))
    }

    /// Detect file overlaps between a completed agent and currently running agents.
    ///
    /// Returns a list of `(running_node_id, overlapping_files)` pairs.
    pub async fn detect_overlaps(&self, completed_node_id: &str) -> Vec<(String, Vec<String>)> {
        let assignments = self.assignments.lock().await;

        let completed_files: HashSet<&str> = assignments
            .get(completed_node_id)
            .map(|a| a.files_modified.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        if completed_files.is_empty() {
            return Vec::new();
        }

        let mut overlaps = Vec::new();

        for (id, assignment) in assignments.iter() {
            if id == completed_node_id {
                continue;
            }
            if assignment.status != AgentAssignmentStatus::Running {
                continue;
            }

            let overlap: Vec<String> = assignment
                .estimated_file_scopes
                .iter()
                .filter(|f| completed_files.contains(f.as_str()))
                .cloned()
                .collect();

            if !overlap.is_empty() {
                overlaps.push((id.clone(), overlap));
            }
        }

        overlaps
    }

    /// Get all current assignments (snapshot).
    pub async fn assignments(&self) -> HashMap<String, AgentAssignment> {
        self.assignments.lock().await.clone()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::event_bus::InMemoryBus;

    #[tokio::test]
    async fn register_and_track_assignments() {
        let ctx = SwarmContext::new("wf-1");

        ctx.register(
            "n1",
            "researcher",
            "Research APIs",
            vec!["src/api.rs".into()],
        )
        .await;
        ctx.register("n2", "writer", "Write docs", vec!["docs/api.md".into()])
            .await;

        let assignments = ctx.assignments().await;
        assert_eq!(assignments.len(), 2);
        assert_eq!(assignments["n1"].name, "researcher");
        assert_eq!(assignments["n1"].status, AgentAssignmentStatus::Pending);
    }

    #[tokio::test]
    async fn sibling_context_excludes_self() {
        let ctx = SwarmContext::new("wf-2");

        ctx.register("n1", "alpha", "Task A", vec!["src/a.rs".into()])
            .await;
        ctx.register("n2", "beta", "Task B", vec!["src/b.rs".into()])
            .await;
        ctx.mark_running("n1").await;
        ctx.mark_running("n2").await;

        let sib = ctx.sibling_context("n1").await;
        assert_eq!(sib.siblings.len(), 1);
        assert_eq!(sib.siblings[0].name, "beta");
        assert!(sib.potential_conflicts.is_empty());
    }

    #[tokio::test]
    async fn sibling_context_detects_file_scope_overlap() {
        let ctx = SwarmContext::new("wf-3");

        ctx.register(
            "n1",
            "alpha",
            "Edit shared",
            vec!["src/shared.rs".into(), "src/a.rs".into()],
        )
        .await;
        ctx.register(
            "n2",
            "beta",
            "Also edit shared",
            vec!["src/shared.rs".into(), "src/b.rs".into()],
        )
        .await;
        ctx.mark_running("n1").await;
        ctx.mark_running("n2").await;

        let sib = ctx.sibling_context("n1").await;
        assert_eq!(sib.potential_conflicts, vec!["src/shared.rs"]);
    }

    #[tokio::test]
    async fn completed_agent_excluded_from_siblings() {
        let ctx = SwarmContext::new("wf-4");

        ctx.register("n1", "done", "Finished task", vec![]).await;
        ctx.register("n2", "running", "Active task", vec![]).await;
        ctx.mark_completed("n1", vec![]).await;
        ctx.mark_running("n2").await;

        let sib = ctx.sibling_context("n2").await;
        assert!(
            sib.siblings.is_empty(),
            "completed agent should not appear as sibling"
        );
    }

    #[tokio::test]
    async fn format_context_returns_none_when_no_siblings() {
        let ctx = SwarmContext::new("wf-5");
        ctx.register("n1", "solo", "Work alone", vec![]).await;

        let text = ctx.format_context_for_prompt("n1").await;
        assert!(text.is_none());
    }

    #[tokio::test]
    async fn format_context_includes_conflict_warning() {
        let ctx = SwarmContext::new("wf-6");

        ctx.register("n1", "alpha", "Edit main", vec!["src/main.rs".into()])
            .await;
        ctx.register("n2", "beta", "Also main", vec!["src/main.rs".into()])
            .await;
        ctx.mark_running("n1").await;
        ctx.mark_running("n2").await;

        let text = ctx
            .format_context_for_prompt("n1")
            .await
            .expect("should have context");
        assert!(text.contains("Parallel Agents"));
        assert!(text.contains("beta"));
        assert!(text.contains("Potential file conflicts"));
        assert!(text.contains("src/main.rs"));
    }

    #[tokio::test]
    async fn detect_overlaps_after_completion() {
        let ctx = SwarmContext::new("wf-7");

        ctx.register("n1", "writer", "Write code", vec!["src/lib.rs".into()])
            .await;
        ctx.register("n2", "reviewer", "Review code", vec!["src/lib.rs".into()])
            .await;
        ctx.mark_running("n1").await;
        ctx.mark_running("n2").await;

        // n1 completes and modified src/lib.rs.
        ctx.mark_completed("n1", vec!["src/lib.rs".into()]).await;

        let overlaps = ctx.detect_overlaps("n1").await;
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].0, "n2");
        assert_eq!(overlaps[0].1, vec!["src/lib.rs"]);
    }

    #[tokio::test]
    async fn event_bus_publishes_completion() {
        let bus = Arc::new(InMemoryBus::new(64));
        let ctx = SwarmContext::new("wf-8").with_event_bus(bus.clone());

        let mut subscriber = bus.subscribe();

        ctx.register("n1", "agent", "Do work", vec![]).await;
        ctx.mark_completed("n1", vec!["output.txt".into()]).await;

        let event = subscriber.recv().await.expect("should receive event");
        assert_eq!(event.topic, "swarm.agent.completed");
        assert!(event.payload.contains("output.txt"));
    }

    #[tokio::test]
    async fn event_bus_publishes_failure() {
        let bus = Arc::new(InMemoryBus::new(64));
        let ctx = SwarmContext::new("wf-9").with_event_bus(bus.clone());

        let mut subscriber = bus.subscribe();

        ctx.register("n1", "agent", "Do work", vec![]).await;
        ctx.mark_failed("n1", "something broke").await;

        let event = subscriber.recv().await.expect("should receive event");
        assert_eq!(event.topic, "swarm.agent.failed");
        assert!(event.payload.contains("something broke"));
    }
}
