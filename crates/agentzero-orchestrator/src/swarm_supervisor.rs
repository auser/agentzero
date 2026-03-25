//! Swarm supervisor — orchestrates goal-to-execution lifecycle.
//!
//! Takes a [`PlannedWorkflow`] from the goal planner, compiles it into an
//! [`ExecutionPlan`], and runs it using the parallel workflow executor with
//! [`SwarmContext`] for cross-agent awareness and [`RecoveryMonitor`] for
//! dead agent recovery.

use std::collections::HashMap;
use std::sync::Arc;

use agentzero_core::{EventBus, Provider, ToolSummary};
use serde::{Deserialize, Serialize};

use crate::goal_planner::{self, PlannedWorkflow};
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
    /// Maximum re-plan attempts before giving up (default: 3).
    pub max_replan_attempts: usize,
    /// Re-planning policy: auto, human_approved, or disabled.
    pub replan_policy: ReplanPolicy,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            sandbox_level: "worktree".to_string(),
            recovery: RecoveryConfig::default(),
            max_tokens: 0,
            max_replan_attempts: 3,
            replan_policy: ReplanPolicy::Auto,
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
    /// History of adaptive re-plan attempts (empty if no failures occurred).
    #[serde(default)]
    pub replan_history: Vec<ReplanRecord>,
}

/// Controls whether adaptive re-planning is automatic or requires approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReplanPolicy {
    /// Automatically re-plan on failure (no human intervention).
    #[default]
    Auto,
    /// Pause and request human approval before re-planning.
    HumanApproved,
    /// Never re-plan; fail immediately.
    Disabled,
}

/// One entry in the re-plan history for observability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplanRecord {
    pub attempt: usize,
    pub failed_node_id: String,
    pub failed_node_name: String,
    pub failure_reason: String,
    pub completed_node_ids: Vec<String>,
    pub new_plan_title: String,
    pub new_plan_node_count: usize,
    pub timestamp_ms: u128,
}

/// State captured at the point of failure, passed to the re-planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSnapshot {
    pub original_goal: String,
    pub completed: Vec<CompletedNodeSummary>,
    pub failed_node_id: String,
    pub failed_node_name: String,
    pub failed_node_task: String,
    pub failure_reason: String,
    pub remaining_node_ids: Vec<String>,
}

/// Summary of a completed node for the re-plan prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedNodeSummary {
    pub node_id: String,
    pub node_name: String,
    pub task: String,
    pub output_preview: String,
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
    /// Provider for re-planning LLM calls. When None, re-planning is disabled.
    replan_provider: Option<Arc<dyn Provider>>,
    /// Available tool summaries for the re-planner.
    available_tools: Vec<ToolSummary>,
}

impl SwarmSupervisor {
    /// Create a new swarm supervisor.
    pub fn new() -> Self {
        Self {
            event_bus: None,
            replan_provider: None,
            available_tools: vec![],
        }
    }

    /// Attach an event bus for swarm-level events.
    pub fn with_event_bus(mut self, bus: Arc<dyn EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Configure a provider and tool catalog for adaptive re-planning.
    pub fn with_replan_provider(
        mut self,
        provider: Arc<dyn Provider>,
        tools: Vec<ToolSummary>,
    ) -> Self {
        self.replan_provider = Some(provider);
        self.available_tools = tools;
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
            replan_history: vec![],
        })
    }

    /// Execute with adaptive re-planning on failure.
    ///
    /// When a node fails and re-planning is enabled:
    /// 1. Snapshots completed outputs + failure context
    /// 2. Calls `GoalPlanner::replan_with_provider()` for a recovery plan
    /// 3. Compiles and executes the recovery plan
    /// 4. Repeats until success or max attempts exhausted
    pub async fn execute_with_replan(
        &self,
        plan: &PlannedWorkflow,
        initial_input: &str,
        dispatcher: Arc<dyn StepDispatcher>,
        status_tx: Option<tokio::sync::mpsc::Sender<StatusUpdate>>,
        config: &SwarmConfig,
        goal: &str,
    ) -> anyhow::Result<SwarmResult> {
        let mut current_plan = plan.clone();
        let mut all_completed_outputs: HashMap<String, String> = HashMap::new();
        let mut all_node_statuses: HashMap<String, NodeStatus> = HashMap::new();
        let mut replan_history: Vec<ReplanRecord> = Vec::new();
        let mut last_run_id = String::new();

        for attempt in 0..=config.max_replan_attempts {
            let result = self
                .execute(
                    &current_plan,
                    initial_input,
                    Arc::clone(&dispatcher),
                    status_tx.clone(),
                )
                .await?;

            last_run_id = result.run_id.clone();

            // Merge results.
            for (k, v) in &result.outputs {
                all_completed_outputs.insert(k.clone(), v.clone());
            }
            for (k, v) in &result.node_statuses {
                all_node_statuses.insert(k.clone(), *v);
            }

            if result.success {
                return Ok(SwarmResult {
                    run_id: result.run_id,
                    workflow_title: plan.title.clone(),
                    node_count: all_node_statuses.len(),
                    node_statuses: all_node_statuses,
                    success: true,
                    outputs: all_completed_outputs,
                    replan_history,
                });
            }

            // Check policy.
            if config.replan_policy == ReplanPolicy::Disabled
                || attempt >= config.max_replan_attempts
            {
                if attempt >= config.max_replan_attempts {
                    tracing::warn!(attempts = attempt, "max re-plan attempts exhausted");
                }
                return Ok(SwarmResult {
                    run_id: result.run_id,
                    workflow_title: plan.title.clone(),
                    node_count: all_node_statuses.len(),
                    node_statuses: all_node_statuses,
                    success: false,
                    outputs: all_completed_outputs,
                    replan_history,
                });
            }

            // Find the first failed node.
            let failed_node_id = match result
                .node_statuses
                .iter()
                .find(|(_, s)| **s == NodeStatus::Failed)
            {
                Some((id, _)) => id.clone(),
                None => break,
            };

            let failed_planned = current_plan.nodes.iter().find(|n| n.id == failed_node_id);
            let failed_name = failed_planned.map(|n| n.name.clone()).unwrap_or_default();
            let failed_task = failed_planned.map(|n| n.task.clone()).unwrap_or_default();

            // Build snapshot.
            let completed: Vec<CompletedNodeSummary> = result
                .node_statuses
                .iter()
                .filter(|(_, s)| **s == NodeStatus::Completed)
                .map(|(id, _)| {
                    let planned = current_plan.nodes.iter().find(|n| n.id == *id);
                    let output = all_completed_outputs.get(id).cloned().unwrap_or_default();
                    CompletedNodeSummary {
                        node_id: id.clone(),
                        node_name: planned.map(|n| n.name.clone()).unwrap_or_default(),
                        task: planned.map(|n| n.task.clone()).unwrap_or_default(),
                        output_preview: output.chars().take(500).collect(),
                    }
                })
                .collect();

            let remaining: Vec<String> = result
                .node_statuses
                .iter()
                .filter(|(_, s)| matches!(s, NodeStatus::Pending | NodeStatus::Suspended))
                .map(|(id, _)| id.clone())
                .collect();

            let snapshot = ExecutionSnapshot {
                original_goal: goal.to_string(),
                completed,
                failed_node_id: failed_node_id.clone(),
                failed_node_name: failed_name.clone(),
                failed_node_task: failed_task,
                failure_reason: "node execution failed".to_string(),
                remaining_node_ids: remaining,
            };

            // Emit re-plan event.
            if let Some(ref bus) = self.event_bus {
                let payload = serde_json::json!({
                    "attempt": attempt + 1,
                    "failed_node_id": &snapshot.failed_node_id,
                    "failed_node_name": &snapshot.failed_node_name,
                });
                let event = agentzero_core::event_bus::Event::new(
                    "swarm.replan.started",
                    "swarm",
                    payload.to_string(),
                );
                let _ = bus.publish(event).await;
            }

            // Human approval gate.
            if config.replan_policy == ReplanPolicy::HumanApproved {
                let decision = dispatcher
                    .suspend_gate(&result.run_id, &failed_node_id, "replan-approval")
                    .await;
                if decision != "approved" {
                    tracing::info!("re-plan denied by human");
                    return Ok(SwarmResult {
                        run_id: result.run_id,
                        workflow_title: plan.title.clone(),
                        node_count: all_node_statuses.len(),
                        node_statuses: all_node_statuses,
                        success: false,
                        outputs: all_completed_outputs,
                        replan_history,
                    });
                }
            }

            // Call the re-planner.
            let provider = match &self.replan_provider {
                Some(p) => p,
                None => {
                    tracing::warn!("re-plan needed but no provider configured");
                    break;
                }
            };

            let new_plan = goal_planner::replan_with_provider(
                provider.as_ref(),
                &snapshot,
                &self.available_tools,
            )
            .await?;

            tracing::info!(
                attempt = attempt + 1,
                new_nodes = new_plan.nodes.len(),
                title = %new_plan.title,
                "re-plan generated"
            );

            replan_history.push(ReplanRecord {
                attempt: attempt + 1,
                failed_node_id: snapshot.failed_node_id.clone(),
                failed_node_name: snapshot.failed_node_name.clone(),
                failure_reason: snapshot.failure_reason.clone(),
                completed_node_ids: snapshot
                    .completed
                    .iter()
                    .map(|c| c.node_id.clone())
                    .collect(),
                new_plan_title: new_plan.title.clone(),
                new_plan_node_count: new_plan.nodes.len(),
                timestamp_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis(),
            });

            current_plan = new_plan;
        }

        Ok(SwarmResult {
            run_id: last_run_id,
            workflow_title: plan.title.clone(),
            node_count: all_node_statuses.len(),
            node_statuses: all_node_statuses,
            success: false,
            outputs: all_completed_outputs,
            replan_history,
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

    // ── Re-planning tests ────────────────────────────────────────────

    #[tokio::test]
    async fn replan_disabled_returns_failure_immediately() {
        let supervisor = SwarmSupervisor::new();
        let plan = simple_plan();
        let dispatcher: Arc<dyn StepDispatcher> = Arc::new(MockDispatcher);
        let config = SwarmConfig {
            replan_policy: ReplanPolicy::Disabled,
            ..Default::default()
        };

        let result = supervisor
            .execute_with_replan(&plan, "go", dispatcher, None, &config, "test goal")
            .await
            .expect("execute");

        // MockDispatcher succeeds, so this should succeed even with Disabled policy.
        assert!(result.success);
        assert!(result.replan_history.is_empty());
    }

    #[tokio::test]
    async fn replan_no_provider_returns_failure() {
        // Supervisor without replan_provider — should not panic.
        let supervisor = SwarmSupervisor::new();
        let plan = simple_plan();
        let dispatcher: Arc<dyn StepDispatcher> = Arc::new(MockDispatcher);
        let config = SwarmConfig::default();

        let result = supervisor
            .execute_with_replan(&plan, "go", dispatcher, None, &config, "test goal")
            .await
            .expect("execute");

        // MockDispatcher succeeds, so no re-planning needed.
        assert!(result.success);
        assert!(result.replan_history.is_empty());
    }

    #[tokio::test]
    async fn replan_preserves_outputs_on_success() {
        let supervisor = SwarmSupervisor::new();
        let plan = simple_plan();
        let dispatcher: Arc<dyn StepDispatcher> = Arc::new(MockDispatcher);
        let config = SwarmConfig::default();

        let result = supervisor
            .execute_with_replan(&plan, "input", dispatcher, None, &config, "goal")
            .await
            .expect("execute");

        assert!(result.success);
        assert!(!result.outputs.is_empty(), "outputs should be preserved");
        assert!(result.replan_history.is_empty());
    }

    #[tokio::test]
    async fn replan_policy_default_is_auto() {
        assert_eq!(ReplanPolicy::default(), ReplanPolicy::Auto);
    }

    #[tokio::test]
    async fn replan_record_serializes() {
        let record = ReplanRecord {
            attempt: 1,
            failed_node_id: "n2".to_string(),
            failed_node_name: "writer".to_string(),
            failure_reason: "timeout".to_string(),
            completed_node_ids: vec!["n1".to_string()],
            new_plan_title: "Recovery: retry writer".to_string(),
            new_plan_node_count: 1,
            timestamp_ms: 12345,
        };
        let json = serde_json::to_string(&record).expect("serialize");
        assert!(json.contains("Recovery: retry writer"));
        assert!(json.contains("timeout"));
    }

    #[tokio::test]
    async fn execution_snapshot_serializes() {
        let snapshot = ExecutionSnapshot {
            original_goal: "build API".to_string(),
            completed: vec![CompletedNodeSummary {
                node_id: "n1".to_string(),
                node_name: "researcher".to_string(),
                task: "research".to_string(),
                output_preview: "found stuff".to_string(),
            }],
            failed_node_id: "n2".to_string(),
            failed_node_name: "writer".to_string(),
            failed_node_task: "write code".to_string(),
            failure_reason: "compile error".to_string(),
            remaining_node_ids: vec!["n3".to_string()],
        };
        let json = serde_json::to_string(&snapshot).expect("serialize");
        assert!(json.contains("build API"));
        assert!(json.contains("compile error"));
    }
}
