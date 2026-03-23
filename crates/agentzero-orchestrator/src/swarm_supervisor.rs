//! Swarm supervisor — orchestrates goal-to-execution lifecycle.
//!
//! Takes a [`PlannedWorkflow`] from the goal planner, compiles it into an
//! [`ExecutionPlan`], and runs it using the parallel workflow executor with
//! [`SwarmContext`] for cross-agent awareness and [`RecoveryMonitor`] for
//! dead agent recovery.

use std::sync::Arc;

use agentzero_core::EventBus;
use serde::{Deserialize, Serialize};

use crate::goal_planner::PlannedWorkflow;
use crate::recovery::RecoveryConfig;
use crate::swarm_context::SwarmContext;
use crate::workflow_executor::{
    compile, execute_with_updates, NodeStatus, StatusUpdate, StepDispatcher,
};

// ── Types ────────────────────────────────────────────────────────────────────

/// Configuration for the swarm supervisor.
#[derive(Debug, Clone)]
pub struct SwarmConfig {
    /// Sandbox isolation level: "worktree", "container", "microvm".
    pub sandbox_level: String,
    /// Recovery configuration for dead agent handling.
    pub recovery: RecoveryConfig,
    /// Maximum total token budget (0 = unlimited).
    pub max_tokens: usize,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            sandbox_level: "worktree".to_string(),
            recovery: RecoveryConfig::default(),
            max_tokens: 0,
        }
    }
}

/// Result of a swarm execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmResult {
    /// The workflow run state.
    pub run_id: String,
    /// The workflow that was generated and executed.
    pub workflow_title: String,
    /// Number of agent nodes in the plan.
    pub node_count: usize,
    /// Final status of each node.
    pub node_statuses: std::collections::HashMap<String, NodeStatus>,
    /// Whether all nodes completed successfully.
    pub success: bool,
    /// Outputs from completed nodes.
    pub outputs: std::collections::HashMap<String, String>,
}

/// A swarm execution request.
#[derive(Debug, Clone)]
pub struct SwarmRequest {
    /// The natural language goal.
    pub goal: String,
    /// Pre-planned workflow (if goal was already decomposed).
    pub plan: Option<PlannedWorkflow>,
    /// Swarm configuration.
    pub config: SwarmConfig,
}

// ── SwarmSupervisor ──────────────────────────────────────────────────────────

/// Orchestrates goal decomposition and parallel agent execution.
///
/// The supervisor:
/// 1. Takes a goal or pre-planned workflow
/// 2. Compiles it into an execution plan
/// 3. Registers agents with SwarmContext for cross-agent awareness
/// 4. Executes the plan using the parallel JoinSet executor
/// 5. Collects results and reports status
pub struct SwarmSupervisor {
    event_bus: Option<Arc<dyn EventBus>>,
}

impl SwarmSupervisor {
    /// Create a new swarm supervisor.
    pub fn new() -> Self {
        Self { event_bus: None }
    }

    /// Attach an event bus for swarm-level events.
    pub fn with_event_bus(mut self, bus: Arc<dyn EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Execute a pre-planned workflow.
    ///
    /// Compiles the plan, registers agents with SwarmContext, and runs
    /// the workflow using the parallel executor.
    pub async fn execute(
        &self,
        plan: &PlannedWorkflow,
        initial_input: &str,
        dispatcher: Arc<dyn StepDispatcher>,
        status_tx: Option<tokio::sync::mpsc::Sender<StatusUpdate>>,
    ) -> anyhow::Result<SwarmResult> {
        // Convert planned workflow to ReactFlow JSON.
        let (nodes, edges) = plan.to_workflow_json();

        // Compile into execution plan.
        let workflow_id = format!(
            "swarm-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let exec_plan = compile(&workflow_id, &nodes, &edges)
            .map_err(|e| anyhow::anyhow!("workflow compilation failed: {e}"))?;

        // Set up SwarmContext for cross-agent awareness.
        let swarm_ctx = if let Some(ref bus) = self.event_bus {
            SwarmContext::new(&workflow_id).with_event_bus(Arc::clone(bus))
        } else {
            SwarmContext::new(&workflow_id)
        };

        // Register all agent nodes.
        for node in &plan.nodes {
            swarm_ctx
                .register(&node.id, &node.name, &node.task, node.file_scopes.clone())
                .await;
        }

        tracing::info!(
            workflow_id = %workflow_id,
            title = %plan.title,
            node_count = plan.nodes.len(),
            "starting swarm execution"
        );

        // Execute with the parallel JoinSet executor.
        let run = execute_with_updates(&exec_plan, initial_input, dispatcher, status_tx).await?;

        // Update SwarmContext with completion status.
        for (node_id, status) in &run.node_statuses {
            match status {
                NodeStatus::Completed => {
                    swarm_ctx.mark_completed(node_id, vec![]).await;
                }
                NodeStatus::Failed => {
                    swarm_ctx.mark_failed(node_id, "execution failed").await;
                }
                _ => {}
            }
        }

        // Collect text outputs.
        let mut outputs = std::collections::HashMap::new();
        for ((node_id, port), value) in &run.outputs {
            if port == "response" {
                if let Some(text) = value.as_str() {
                    outputs.insert(node_id.clone(), text.to_string());
                }
            }
        }

        let success = run
            .node_statuses
            .values()
            .all(|s| matches!(s, NodeStatus::Completed | NodeStatus::Skipped));

        tracing::info!(
            workflow_id = %workflow_id,
            success = success,
            completed = run.node_statuses.values().filter(|s| **s == NodeStatus::Completed).count(),
            failed = run.node_statuses.values().filter(|s| **s == NodeStatus::Failed).count(),
            "swarm execution complete"
        );

        Ok(SwarmResult {
            run_id: run.run_id,
            workflow_title: plan.title.clone(),
            node_count: plan.nodes.len(),
            node_statuses: run.node_statuses,
            success,
            outputs,
        })
    }
}

impl Default for SwarmSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal_planner::{PlannedNode, PlannedWorkflow};
    use crate::workflow_executor::{ExecutionStep, StepDispatcher};
    use std::sync::Arc;

    struct MockDispatcher;

    #[async_trait::async_trait]
    impl StepDispatcher for MockDispatcher {
        async fn run_agent(
            &self,
            step: &ExecutionStep,
            input: &str,
            _context: Option<&serde_json::Value>,
        ) -> anyhow::Result<String> {
            Ok(format!("[{}] processed: {}", step.name, input))
        }

        async fn run_tool(
            &self,
            tool_name: &str,
            input: &serde_json::Value,
        ) -> anyhow::Result<String> {
            Ok(format!("tool:{tool_name} result for {input}"))
        }

        async fn send_channel(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn simple_plan() -> PlannedWorkflow {
        PlannedWorkflow {
            title: "Test Swarm".to_string(),
            nodes: vec![
                PlannedNode {
                    id: "n1".to_string(),
                    name: "researcher".to_string(),
                    task: "Research the topic".to_string(),
                    depends_on: vec![],
                    file_scopes: vec!["src/research.rs".to_string()],
                    sandbox_level: "worktree".to_string(),
                    tool_hints: vec![],
                },
                PlannedNode {
                    id: "n2".to_string(),
                    name: "writer".to_string(),
                    task: "Write the code".to_string(),
                    depends_on: vec!["n1".to_string()],
                    file_scopes: vec!["src/main.rs".to_string()],
                    sandbox_level: "worktree".to_string(),
                    tool_hints: vec![],
                },
            ],
        }
    }

    #[tokio::test]
    async fn execute_simple_swarm() {
        let supervisor = SwarmSupervisor::new();
        let plan = simple_plan();
        let dispatcher: Arc<dyn StepDispatcher> = Arc::new(MockDispatcher);

        let result = supervisor
            .execute(&plan, "Build something", dispatcher, None)
            .await
            .expect("execute");

        assert!(result.success);
        assert_eq!(result.node_count, 2);
        assert_eq!(result.workflow_title, "Test Swarm");
        assert_eq!(result.node_statuses.get("n1"), Some(&NodeStatus::Completed));
        assert_eq!(result.node_statuses.get("n2"), Some(&NodeStatus::Completed));
    }

    #[tokio::test]
    async fn execute_parallel_swarm() {
        let plan = PlannedWorkflow {
            title: "Parallel Tasks".to_string(),
            nodes: vec![
                PlannedNode {
                    id: "a".to_string(),
                    name: "alpha".to_string(),
                    task: "Task A".to_string(),
                    depends_on: vec![],
                    file_scopes: vec![],
                    sandbox_level: "worktree".to_string(),
                    tool_hints: vec![],
                },
                PlannedNode {
                    id: "b".to_string(),
                    name: "beta".to_string(),
                    task: "Task B".to_string(),
                    depends_on: vec![],
                    file_scopes: vec![],
                    sandbox_level: "worktree".to_string(),
                    tool_hints: vec![],
                },
                PlannedNode {
                    id: "c".to_string(),
                    name: "merger".to_string(),
                    task: "Merge results".to_string(),
                    depends_on: vec!["a".to_string(), "b".to_string()],
                    file_scopes: vec![],
                    sandbox_level: "worktree".to_string(),
                    tool_hints: vec![],
                },
            ],
        };

        let supervisor = SwarmSupervisor::new();
        let dispatcher: Arc<dyn StepDispatcher> = Arc::new(MockDispatcher);

        let result = supervisor
            .execute(&plan, "go", dispatcher, None)
            .await
            .expect("execute");

        assert!(result.success);
        assert_eq!(result.node_count, 3);
        // All nodes should complete.
        for id in &["a", "b", "c"] {
            assert_eq!(
                result.node_statuses.get(*id),
                Some(&NodeStatus::Completed),
                "node {id} should complete"
            );
        }
    }

    #[tokio::test]
    async fn execute_with_status_updates() {
        let supervisor = SwarmSupervisor::new();
        let plan = simple_plan();
        let dispatcher: Arc<dyn StepDispatcher> = Arc::new(MockDispatcher);

        let (tx, mut rx) = tokio::sync::mpsc::channel(64);

        let result = supervisor
            .execute(&plan, "start", dispatcher, Some(tx))
            .await
            .expect("execute");

        assert!(result.success);

        // Collect all status updates.
        let mut updates = Vec::new();
        while let Ok(update) = rx.try_recv() {
            updates.push(update);
        }

        // Should have at least Running + Completed for each node.
        assert!(
            updates.len() >= 4,
            "expected at least 4 status updates, got {}",
            updates.len()
        );
    }

    #[tokio::test]
    async fn execute_captures_outputs() {
        let supervisor = SwarmSupervisor::new();
        let plan = PlannedWorkflow {
            title: "Output Test".to_string(),
            nodes: vec![PlannedNode {
                id: "n1".to_string(),
                name: "worker".to_string(),
                task: "Do work".to_string(),
                depends_on: vec![],
                file_scopes: vec![],
                sandbox_level: "worktree".to_string(),
                tool_hints: vec![],
            }],
        };
        let dispatcher: Arc<dyn StepDispatcher> = Arc::new(MockDispatcher);

        let result = supervisor
            .execute(&plan, "input text", dispatcher, None)
            .await
            .expect("execute");

        let output = result.outputs.get("n1").expect("should have n1 output");
        assert!(output.contains("worker"));
        assert!(output.contains("input text"));
    }

    #[tokio::test]
    async fn execute_with_event_bus() {
        let bus = Arc::new(agentzero_core::InMemoryBus::default_capacity());
        let supervisor = SwarmSupervisor::new().with_event_bus(bus.clone());
        let plan = simple_plan();
        let dispatcher: Arc<dyn StepDispatcher> = Arc::new(MockDispatcher);

        let mut sub = bus.subscribe();

        let result = supervisor
            .execute(&plan, "go", dispatcher, None)
            .await
            .expect("execute");

        assert!(result.success);

        // Should have published completion events via SwarmContext.
        let mut topics = Vec::new();
        while let Ok(event) =
            tokio::time::timeout(std::time::Duration::from_millis(50), sub.recv()).await
        {
            if let Ok(e) = event {
                topics.push(e.topic);
            }
        }

        assert!(
            topics.iter().any(|t| t == "swarm.agent.completed"),
            "should publish completion events, got: {topics:?}"
        );
    }
}
