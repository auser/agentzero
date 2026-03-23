//! Workflow execution engine — compiles visual workflow graphs into executable
//! plans and runs them step-by-step with topological ordering.
//!
//! The compiler resolves config nodes (Provider, Role) at build time, produces
//! parallelizable execution levels, and the executor dispatches each node type
//! to the appropriate runtime (agent loop, tool execute, channel send).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::task::JoinSet;

// ── Types ────────────────────────────────────────────────────────────────────

/// Classification of a workflow node for execution purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Agent,
    Tool,
    Channel,
    Schedule,
    Gate,
    SubAgent,
    Provider,
    Role,
    /// Human-in-the-loop input node — behaves like a Channel trigger at
    /// execution time (passes through input as `response` output).
    HumanInput,
    /// File I/O nodes — dispatched as built-in tools at execution time.
    SaveFile,
    ReadFile,
    /// HTTP request node — dispatched as a tool at execution time.
    HttpRequest,
}

impl NodeType {
    /// Parse from the node_type string used in the UI.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "agent" => Some(Self::Agent),
            "tool" => Some(Self::Tool),
            "channel" => Some(Self::Channel),
            "human_input" => Some(Self::HumanInput),
            "save_file" => Some(Self::SaveFile),
            "read_file" => Some(Self::ReadFile),
            "http_request" => Some(Self::HttpRequest),
            "schedule" => Some(Self::Schedule),
            "gate" => Some(Self::Gate),
            "subagent" => Some(Self::SubAgent),
            "provider" => Some(Self::Provider),
            "role" => Some(Self::Role),
            _ => None,
        }
    }

    /// Config nodes don't execute — their values are folded into connected nodes.
    pub fn is_config(&self) -> bool {
        matches!(self, Self::Provider | Self::Role)
    }

    /// Trigger nodes have no inputs — they initiate execution.
    pub fn is_trigger(&self) -> bool {
        matches!(self, Self::Schedule | Self::Channel | Self::HumanInput)
    }
}

/// A single node in the workflow graph, parsed from ReactFlow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    pub node_type: NodeType,
    pub name: String,
    pub metadata: serde_json::Value,
}

/// A directed edge between two ports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub id: String,
    pub source: String,
    pub source_port: String,
    pub target: String,
    pub target_port: String,
    /// Optional condition expression for conditional routing.
    #[serde(default)]
    pub condition: Option<String>,
}

/// Provider/role configuration resolved from config nodes and folded into
/// the agent nodes they connect to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResolvedNodeConfig {
    /// Provider kind (e.g. "anthropic", "openai").
    #[serde(default)]
    pub provider: Option<String>,
    /// Model name override.
    #[serde(default)]
    pub model: Option<String>,
    /// Role name.
    #[serde(default)]
    pub role_name: Option<String>,
    /// Role description/instructions.
    #[serde(default)]
    pub role_description: Option<String>,
}

/// One step in the execution plan — a node that actually runs.
#[derive(Debug, Clone)]
pub struct ExecutionStep {
    pub node_id: String,
    pub node_type: NodeType,
    pub name: String,
    pub metadata: serde_json::Value,
    /// Resolved config from connected Provider/Role nodes.
    pub config: ResolvedNodeConfig,
}

/// Compiled execution plan — topologically sorted into parallelizable levels.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub workflow_id: String,
    /// Each level contains steps that can run in parallel.
    pub levels: Vec<Vec<ExecutionStep>>,
    /// Edge map: (source_node, source_port) → Vec<(target_node, target_port)>
    pub edges: HashMap<(String, String), Vec<(String, String)>>,
    /// Reverse edge map for collecting inputs: (target_node, target_port) → Vec<(source_node, source_port)>
    pub reverse_edges: HashMap<(String, String), Vec<(String, String)>>,
    /// All execution steps indexed by node ID (for ready-queue executor).
    pub steps: HashMap<String, ExecutionStep>,
    /// Dependency graph: node_id → set of node IDs it depends on (executable edges only).
    pub deps: HashMap<String, HashSet<String>>,
    /// Reverse dependency graph: node_id → set of node IDs that depend on it.
    pub dependents: HashMap<String, HashSet<String>>,
}

/// Status of a single node during workflow execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
    Suspended,
}

/// Runtime state of a workflow execution.
#[derive(Debug, Clone)]
pub struct WorkflowRun {
    pub run_id: String,
    pub workflow_id: String,
    /// Output values from completed steps: (node_id, port_id) → Value
    pub outputs: HashMap<(String, String), serde_json::Value>,
    /// Current status of each node.
    pub node_statuses: HashMap<String, NodeStatus>,
}

// ── Compiler ─────────────────────────────────────────────────────────────────

/// Errors during workflow compilation.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("workflow graph contains a cycle involving node '{0}'")]
    CycleDetected(String),
    #[error("unknown node type '{0}' on node '{1}'")]
    UnknownNodeType(String, String),
    #[error("workflow has no executable nodes")]
    EmptyGraph,
}

/// Compile a workflow definition (ReactFlow nodes + edges) into an ExecutionPlan.
pub fn compile(
    workflow_id: &str,
    nodes: &[serde_json::Value],
    edges: &[serde_json::Value],
) -> Result<ExecutionPlan, CompileError> {
    // 1. Parse nodes
    let mut parsed_nodes: HashMap<String, WorkflowNode> = HashMap::new();
    for node_val in nodes {
        let id = node_val["id"].as_str().unwrap_or_default().to_string();
        let data = &node_val["data"];
        let node_type_str = data["nodeType"].as_str().unwrap_or_default();
        let node_type = NodeType::parse(node_type_str)
            .ok_or_else(|| CompileError::UnknownNodeType(node_type_str.to_string(), id.clone()))?;

        parsed_nodes.insert(
            id.clone(),
            WorkflowNode {
                id,
                node_type,
                name: data["name"].as_str().unwrap_or_default().to_string(),
                metadata: data["metadata"].clone(),
            },
        );
    }

    // 2. Parse edges
    let mut parsed_edges: Vec<WorkflowEdge> = Vec::new();
    let mut edge_map: HashMap<(String, String), Vec<(String, String)>> = HashMap::new();
    let mut reverse_edge_map: HashMap<(String, String), Vec<(String, String)>> = HashMap::new();

    for edge_val in edges {
        let source = edge_val["source"].as_str().unwrap_or_default().to_string();
        let target = edge_val["target"].as_str().unwrap_or_default().to_string();
        let source_port = edge_val["sourceHandle"]
            .as_str()
            .unwrap_or("output")
            .to_string();
        let target_port = edge_val["targetHandle"]
            .as_str()
            .unwrap_or("input")
            .to_string();
        let condition = edge_val["data"]["condition"].as_str().map(String::from);

        edge_map
            .entry((source.clone(), source_port.clone()))
            .or_default()
            .push((target.clone(), target_port.clone()));

        reverse_edge_map
            .entry((target.clone(), target_port.clone()))
            .or_default()
            .push((source.clone(), source_port.clone()));

        parsed_edges.push(WorkflowEdge {
            id: edge_val["id"].as_str().unwrap_or_default().to_string(),
            source,
            source_port,
            target,
            target_port,
            condition,
        });
    }

    // 3. Resolve config nodes (Provider, Role) into connected agent configs
    let mut node_configs: HashMap<String, ResolvedNodeConfig> = HashMap::new();
    for node in parsed_nodes.values() {
        if !node.node_type.is_config() {
            continue;
        }
        // Find all edges FROM this config node
        for edge in &parsed_edges {
            if edge.source != node.id {
                continue;
            }
            let config = node_configs.entry(edge.target.clone()).or_default();
            match node.node_type {
                NodeType::Provider => {
                    config.provider = node.metadata["provider_name"].as_str().map(String::from);
                    config.model = node.metadata["model_name"].as_str().map(String::from);
                }
                NodeType::Role => {
                    config.role_name = node.metadata["role_name"].as_str().map(String::from);
                    config.role_description =
                        node.metadata["role_description"].as_str().map(String::from);
                }
                _ => {}
            }
        }
    }

    // 4. Filter to executable nodes only (exclude config nodes)
    let executable: Vec<&WorkflowNode> = parsed_nodes
        .values()
        .filter(|n| !n.node_type.is_config())
        .collect();

    if executable.is_empty() {
        return Err(CompileError::EmptyGraph);
    }

    // 5. Topological sort (Kahn's algorithm) into parallelizable levels
    let exec_ids: HashSet<&str> = executable.iter().map(|n| n.id.as_str()).collect();

    // Build adjacency for executable nodes only
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for id in &exec_ids {
        in_degree.insert(id, 0);
        adj.insert(id, Vec::new());
    }

    for edge in &parsed_edges {
        let src = edge.source.as_str();
        let tgt = edge.target.as_str();
        // Only count edges between executable nodes
        if exec_ids.contains(src) && exec_ids.contains(tgt) {
            *in_degree.entry(tgt).or_default() += 1;
            adj.entry(src).or_default().push(tgt);
        }
    }

    let mut levels: Vec<Vec<ExecutionStep>> = Vec::new();
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut visited = 0usize;

    while !queue.is_empty() {
        let mut level = Vec::new();
        let mut next_queue = VecDeque::new();

        for node_id in queue.drain(..) {
            visited += 1;
            let node = &parsed_nodes[node_id];
            level.push(ExecutionStep {
                node_id: node.id.clone(),
                node_type: node.node_type,
                name: node.name.clone(),
                metadata: node.metadata.clone(),
                config: node_configs.remove(&node.id).unwrap_or_default(),
            });

            for &neighbor in adj.get(node_id).unwrap_or(&Vec::new()) {
                let deg = in_degree.get_mut(neighbor).expect("node in exec set");
                *deg -= 1;
                if *deg == 0 {
                    next_queue.push_back(neighbor);
                }
            }
        }

        if !level.is_empty() {
            levels.push(level);
        }
        queue = next_queue;
    }

    if visited != exec_ids.len() {
        // Find a node involved in the cycle for the error message
        let cycle_node = in_degree
            .iter()
            .find(|(_, &deg)| deg > 0)
            .map(|(&id, _)| id.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        return Err(CompileError::CycleDetected(cycle_node));
    }

    // Build step index, deps, and dependents maps for the ready-queue executor
    let mut steps_map: HashMap<String, ExecutionStep> = HashMap::new();
    let mut deps_map: HashMap<String, HashSet<String>> = HashMap::new();
    let mut dependents_map: HashMap<String, HashSet<String>> = HashMap::new();

    for level in &levels {
        for step in level {
            steps_map.insert(step.node_id.clone(), step.clone());
            deps_map.entry(step.node_id.clone()).or_default();
            dependents_map.entry(step.node_id.clone()).or_default();
        }
    }

    for edge in &parsed_edges {
        let src = edge.source.as_str();
        let tgt = edge.target.as_str();
        if exec_ids.contains(src) && exec_ids.contains(tgt) {
            deps_map
                .entry(tgt.to_string())
                .or_default()
                .insert(src.to_string());
            dependents_map
                .entry(src.to_string())
                .or_default()
                .insert(tgt.to_string());
        }
    }

    Ok(ExecutionPlan {
        workflow_id: workflow_id.to_string(),
        levels,
        edges: edge_map,
        reverse_edges: reverse_edge_map,
        steps: steps_map,
        deps: deps_map,
        dependents: dependents_map,
    })
}

// ── Executor ─────────────────────────────────────────────────────────────────

/// Trait for dispatching individual workflow steps.
/// Implemented by the gateway/infra layer to wire real agent execution.
#[async_trait::async_trait]
pub trait StepDispatcher: Send + Sync {
    /// Execute an agent node. Returns the response text.
    async fn run_agent(
        &self,
        step: &ExecutionStep,
        input: &str,
        context: Option<&serde_json::Value>,
    ) -> anyhow::Result<String>;

    /// Execute a tool node directly. Returns the tool output.
    async fn run_tool(&self, tool_name: &str, input: &serde_json::Value) -> anyhow::Result<String>;

    /// Send a message to a channel.
    async fn send_channel(&self, channel_type: &str, message: &str) -> anyhow::Result<()>;

    /// Suspend execution at a gate node and wait for a human decision.
    ///
    /// Returns the decision string ("approved" or "denied").
    /// Default implementation auto-approves (for non-interactive contexts).
    async fn suspend_gate(&self, _run_id: &str, _node_id: &str, _node_name: &str) -> String {
        "approved".to_string()
    }
}

/// Real-time status update emitted during workflow execution.
#[derive(Debug, Clone, Serialize)]
pub struct StatusUpdate {
    pub node_id: String,
    pub node_name: String,
    pub status: NodeStatus,
    /// Output text from the node (only set on completion).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// Execute a compiled workflow plan.
///
/// Dispatches nodes concurrently via `tokio::JoinSet` — ready nodes are
/// spawned immediately and dependents are unblocked as each completes.
pub async fn execute(
    plan: &ExecutionPlan,
    initial_input: &str,
    dispatcher: Arc<dyn StepDispatcher>,
) -> anyhow::Result<WorkflowRun> {
    execute_with_updates(plan, initial_input, dispatcher, None).await
}

/// Execute a workflow with optional real-time status updates via a channel.
///
/// Uses `tokio::JoinSet` for true parallel execution: ready nodes are spawned
/// as concurrent tasks immediately, and as each completes its dependents are
/// unblocked and spawned without waiting for sibling nodes.
pub async fn execute_with_updates(
    plan: &ExecutionPlan,
    initial_input: &str,
    dispatcher: Arc<dyn StepDispatcher>,
    status_tx: Option<tokio::sync::mpsc::Sender<StatusUpdate>>,
) -> anyhow::Result<WorkflowRun> {
    let run_id = format!(
        "wfrun-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );

    // Shared mutable state accessed by spawned tasks and the main loop.
    let outputs: Arc<Mutex<HashMap<(String, String), serde_json::Value>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let node_statuses: Arc<Mutex<HashMap<String, NodeStatus>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Initialize all nodes as Pending.
    {
        let mut statuses = node_statuses.lock().await;
        for step in plan.steps.values() {
            statuses.insert(step.node_id.clone(), NodeStatus::Pending);
        }
    }

    // Track remaining dependencies per node (mutable, owned by main loop).
    let mut pending_deps: HashMap<String, HashSet<String>> = plan.deps.clone();

    // Seed root nodes with initial input.
    let mut initial_ready: Vec<String> = Vec::new();
    {
        let mut out = outputs.lock().await;
        for (node_id, deps) in &pending_deps {
            if deps.is_empty() {
                initial_ready.push(node_id.clone());
                out.insert(
                    (node_id.clone(), "input".to_string()),
                    serde_json::Value::String(initial_input.to_string()),
                );
            }
        }
    }

    // JoinSet for concurrent node execution.
    // Each task returns (node_id, node_name, node_type, result).
    type TaskResult = (String, String, NodeType, Result<StepOutput, String>);
    let mut join_set: JoinSet<TaskResult> = JoinSet::new();

    // Spawn a node into the JoinSet.
    let spawn_node =
        |join_set: &mut JoinSet<TaskResult>,
         node_id: String,
         step: ExecutionStep,
         dispatcher: Arc<dyn StepDispatcher>,
         outputs: Arc<Mutex<HashMap<(String, String), serde_json::Value>>>,
         node_statuses: Arc<Mutex<HashMap<String, NodeStatus>>>,
         reverse_edges: HashMap<(String, String), Vec<(String, String)>>,
         status_tx: Option<tokio::sync::mpsc::Sender<StatusUpdate>>| {
            join_set.spawn(async move {
                let node_name = step.name.clone();
                let node_type = step.node_type;

                // Check if this node should be skipped (gate routing).
                {
                    let statuses = node_statuses.lock().await;
                    if statuses.get(&node_id) == Some(&NodeStatus::Skipped) {
                        return (
                            node_id,
                            node_name,
                            node_type,
                            Ok(StepOutput {
                                port_values: HashMap::new(),
                            }),
                        );
                    }
                }

                // Collect inputs from upstream (needs lock on outputs).
                let (input_text, context) = {
                    let out = outputs.lock().await;
                    let run_view = OutputView(&out);
                    let input = collect_input_text_from(&run_view, &reverse_edges, &node_id);
                    let ctx = collect_context_from(&run_view, &reverse_edges, &node_id);
                    (input, ctx)
                };

                // Mark as Running.
                {
                    let mut statuses = node_statuses.lock().await;
                    statuses.insert(node_id.clone(), NodeStatus::Running);
                }
                if let Some(ref tx) = status_tx {
                    let _ = tx
                        .send(StatusUpdate {
                            node_id: node_id.clone(),
                            node_name: node_name.clone(),
                            status: NodeStatus::Running,
                            output: None,
                        })
                        .await;
                }

                // Dispatch the step.
                let result =
                    dispatch_step(&*dispatcher, &step, &input_text, context.as_ref()).await;

                let step_result = match result {
                    Ok(output) => Ok(output),
                    Err(e) => {
                        tracing::error!(
                            node_id = %node_id,
                            error = %e,
                            "workflow step failed"
                        );
                        Err(e.to_string())
                    }
                };

                (node_id, node_name, node_type, step_result)
            });
        };

    // Spawn initial ready nodes.
    for node_id in initial_ready {
        let step = match plan.steps.get(&node_id) {
            Some(s) => s.clone(),
            None => continue,
        };
        spawn_node(
            &mut join_set,
            node_id,
            step,
            Arc::clone(&dispatcher),
            Arc::clone(&outputs),
            Arc::clone(&node_statuses),
            plan.reverse_edges.clone(),
            status_tx.clone(),
        );
    }

    // Process completions as they arrive — immediately unblock and spawn dependents.
    while let Some(join_result) = join_set.join_next().await {
        let (node_id, node_name, node_type, step_result) =
            join_result.map_err(|e| anyhow::anyhow!("workflow task panicked: {e}"))?;

        match step_result {
            Ok(output) => {
                // Store outputs and update status.
                {
                    let mut out = outputs.lock().await;
                    for (port_id, value) in &output.port_values {
                        out.insert((node_id.clone(), port_id.clone()), value.clone());
                    }
                }

                // Handle gate routing synchronously before unblocking dependents.
                if node_type == NodeType::Gate {
                    let mut statuses = node_statuses.lock().await;
                    handle_gate_routing(&output, &node_id, &plan.edges, &mut statuses);
                }

                // Only mark Completed if the node wasn't already Skipped by gate routing.
                let final_status = {
                    let mut statuses = node_statuses.lock().await;
                    let current = statuses.get(&node_id).copied();
                    if current != Some(NodeStatus::Skipped) {
                        statuses.insert(node_id.clone(), NodeStatus::Completed);
                        NodeStatus::Completed
                    } else {
                        NodeStatus::Skipped
                    }
                };

                if let Some(ref tx) = status_tx {
                    let out_text = output
                        .port_values
                        .values()
                        .find_map(|v| v.as_str().map(String::from));
                    let _ = tx
                        .send(StatusUpdate {
                            node_id: node_id.clone(),
                            node_name: node_name.clone(),
                            status: final_status,
                            output: out_text,
                        })
                        .await;
                }
            }
            Err(err_msg) => {
                {
                    let mut statuses = node_statuses.lock().await;
                    statuses.insert(node_id.clone(), NodeStatus::Failed);
                }

                if let Some(ref tx) = status_tx {
                    let _ = tx
                        .send(StatusUpdate {
                            node_id: node_id.clone(),
                            node_name: node_name.clone(),
                            status: NodeStatus::Failed,
                            output: Some(err_msg),
                        })
                        .await;
                }
                // Failed nodes still resolve dependencies — downstream runs with empty input.
            }
        }

        // Unblock dependents and spawn newly ready nodes.
        if let Some(deps_of) = plan.dependents.get(&node_id) {
            for dependent_id in deps_of {
                if let Some(remaining) = pending_deps.get_mut(dependent_id) {
                    remaining.remove(&node_id);
                    if remaining.is_empty() {
                        if let Some(step) = plan.steps.get(dependent_id) {
                            spawn_node(
                                &mut join_set,
                                dependent_id.clone(),
                                step.clone(),
                                Arc::clone(&dispatcher),
                                Arc::clone(&outputs),
                                Arc::clone(&node_statuses),
                                plan.reverse_edges.clone(),
                                status_tx.clone(),
                            );
                        }
                    }
                }
            }
        }
    }

    // Assemble the final WorkflowRun from shared state.
    let final_outputs = match Arc::try_unwrap(outputs) {
        Ok(mutex) => mutex.into_inner(),
        Err(arc) => arc.lock().await.clone(),
    };
    let final_statuses = match Arc::try_unwrap(node_statuses) {
        Ok(mutex) => mutex.into_inner(),
        Err(arc) => arc.lock().await.clone(),
    };

    Ok(WorkflowRun {
        run_id,
        workflow_id: plan.workflow_id.clone(),
        outputs: final_outputs,
        node_statuses: final_statuses,
    })
}

/// Thin wrapper around a borrowed output map for use inside spawned tasks.
struct OutputView<'a>(&'a HashMap<(String, String), serde_json::Value>);

/// Collect input text from an output map (for use inside spawned tasks).
fn collect_input_text_from(
    view: &OutputView<'_>,
    reverse_edges: &HashMap<(String, String), Vec<(String, String)>>,
    node_id: &str,
) -> String {
    for port in &[
        "input", "task", "send", "request", "content", "path", "body",
    ] {
        let key = (node_id.to_string(), port.to_string());
        if let Some(sources) = reverse_edges.get(&key) {
            let mut parts = Vec::new();
            for (src_node, src_port) in sources {
                if let Some(val) = view.0.get(&(src_node.clone(), src_port.clone())) {
                    match val {
                        serde_json::Value::String(s) => parts.push(s.clone()),
                        other => parts.push(other.to_string()),
                    }
                }
            }
            if !parts.is_empty() {
                return parts.join("\n");
            }
        }
    }

    if let Some(val) = view.0.get(&(node_id.to_string(), "input".to_string())) {
        return val.as_str().unwrap_or_default().to_string();
    }

    String::new()
}

/// Collect context JSON from an output map (for use inside spawned tasks).
fn collect_context_from(
    view: &OutputView<'_>,
    reverse_edges: &HashMap<(String, String), Vec<(String, String)>>,
    node_id: &str,
) -> Option<serde_json::Value> {
    let key = (node_id.to_string(), "context".to_string());
    let sources = reverse_edges.get(&key)?;

    let mut context_parts = Vec::new();
    for (src_node, src_port) in sources {
        if let Some(val) = view.0.get(&(src_node.clone(), src_port.clone())) {
            context_parts.push(val.clone());
        }
    }

    if context_parts.is_empty() {
        None
    } else if context_parts.len() == 1 {
        Some(context_parts.into_iter().next().expect("just checked len"))
    } else {
        Some(serde_json::Value::Array(context_parts))
    }
}

/// Output from a single step execution.
#[derive(Clone)]
struct StepOutput {
    /// Values keyed by output port ID.
    port_values: HashMap<String, serde_json::Value>,
}

/// Dispatch a single step to the appropriate handler.
async fn dispatch_step(
    dispatcher: &dyn StepDispatcher,
    step: &ExecutionStep,
    input: &str,
    context: Option<&serde_json::Value>,
) -> anyhow::Result<StepOutput> {
    match step.node_type {
        NodeType::Agent | NodeType::SubAgent => {
            let response = dispatcher.run_agent(step, input, context).await?;
            let mut ports = HashMap::new();
            ports.insert(
                "response".to_string(),
                serde_json::Value::String(response.clone()),
            );
            ports.insert("tool_calls".to_string(), serde_json::Value::Array(vec![]));
            Ok(StepOutput { port_values: ports })
        }
        NodeType::Tool => {
            let tool_name = step.metadata["tool_name"].as_str().unwrap_or_default();
            let tool_input = if input.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(input).unwrap_or(serde_json::json!({ "input": input }))
            };
            let result = dispatcher.run_tool(tool_name, &tool_input).await?;
            let mut ports = HashMap::new();
            ports.insert("result".to_string(), serde_json::Value::String(result));
            Ok(StepOutput { port_values: ports })
        }
        NodeType::Channel => {
            let channel_type = step.metadata["channel_type"].as_str().unwrap_or_default();
            // Channel as sink — send the input
            if !input.is_empty() {
                dispatcher.send_channel(channel_type, input).await?;
            }
            // Channel as trigger — output the input as trigger/message
            let mut ports = HashMap::new();
            ports.insert(
                "trigger".to_string(),
                serde_json::Value::String(input.to_string()),
            );
            ports.insert(
                "message".to_string(),
                serde_json::Value::String(input.to_string()),
            );
            Ok(StepOutput { port_values: ports })
        }
        NodeType::HumanInput => {
            // Human-in-the-loop: behaves like a channel trigger.
            // In interactive mode the executor would pause for user input;
            // for batch/API execution, pass through the initial input.
            let mut ports = HashMap::new();
            ports.insert(
                "response".to_string(),
                serde_json::Value::String(input.to_string()),
            );
            Ok(StepOutput { port_values: ports })
        }
        NodeType::SaveFile => {
            // Dispatch as the built-in write_file tool.
            let path = step.metadata["path"].as_str().unwrap_or("output.txt");
            let tool_input =
                serde_json::json!({ "path": path, "content": input, "overwrite": true });
            let result = dispatcher.run_tool("write_file", &tool_input).await?;
            let mut ports = HashMap::new();
            ports.insert(
                "path".to_string(),
                serde_json::Value::String(path.to_string()),
            );
            ports.insert("done".to_string(), serde_json::Value::String(result));
            Ok(StepOutput { port_values: ports })
        }
        NodeType::ReadFile => {
            let path = step.metadata["path"].as_str().unwrap_or_default();
            // If there's an incoming path from the port, prefer it.
            let effective_path = if input.is_empty() {
                path.to_string()
            } else {
                input.to_string()
            };
            let tool_input = serde_json::json!({ "path": effective_path });
            let result = dispatcher.run_tool("read_file", &tool_input).await?;
            let mut ports = HashMap::new();
            ports.insert("content".to_string(), serde_json::Value::String(result));
            Ok(StepOutput { port_values: ports })
        }
        NodeType::HttpRequest => {
            let url = step.metadata["url"].as_str().unwrap_or_default();
            let method = step.metadata["method"].as_str().unwrap_or("GET");
            let tool_input = serde_json::json!({
                "url": url,
                "method": method,
                "body": input,
            });
            let result = dispatcher.run_tool("http_request", &tool_input).await?;
            let mut ports = HashMap::new();
            ports.insert("response".to_string(), serde_json::Value::String(result));
            Ok(StepOutput { port_values: ports })
        }
        NodeType::Gate => {
            // Suspend and wait for human decision via the dispatcher.
            let decision = dispatcher.suspend_gate("", &step.node_id, &step.name).await;

            let mut ports = HashMap::new();
            if decision == "approved" {
                ports.insert(
                    "approved".to_string(),
                    serde_json::Value::String(input.to_string()),
                );
            } else {
                ports.insert(
                    "denied".to_string(),
                    serde_json::Value::String(input.to_string()),
                );
            }
            Ok(StepOutput { port_values: ports })
        }
        NodeType::Schedule => {
            // Schedule nodes are triggers — they just pass through.
            let mut ports = HashMap::new();
            ports.insert(
                "trigger".to_string(),
                serde_json::Value::String(input.to_string()),
            );
            Ok(StepOutput { port_values: ports })
        }
        NodeType::Provider | NodeType::Role => {
            // Config nodes should never be in the execution plan.
            anyhow::bail!(
                "config node {} should not be in execution plan",
                step.node_id
            );
        }
    }
}

/// Handle gate routing — mark downstream nodes on the inactive port as Skipped.
fn handle_gate_routing(
    output: &StepOutput,
    gate_node_id: &str,
    edges: &HashMap<(String, String), Vec<(String, String)>>,
    node_statuses: &mut HashMap<String, NodeStatus>,
) {
    // Determine which output port was activated (has a value)
    let approved = output.port_values.contains_key("approved");
    let inactive_port = if approved { "denied" } else { "approved" };

    // Skip all downstream nodes connected to the inactive port
    let inactive_key = (gate_node_id.to_string(), inactive_port.to_string());
    if let Some(targets) = edges.get(&inactive_key) {
        for (target_node, _) in targets {
            node_statuses.insert(target_node.clone(), NodeStatus::Skipped);
            // TODO: recursively skip descendants of skipped nodes
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn agent_node(id: &str, name: &str) -> serde_json::Value {
        json!({
            "id": id,
            "data": {
                "name": name,
                "nodeType": "agent",
                "metadata": { "system_prompt": "test" }
            }
        })
    }

    fn tool_node(id: &str, name: &str) -> serde_json::Value {
        json!({
            "id": id,
            "data": {
                "name": name,
                "nodeType": "tool",
                "metadata": { "tool_name": "shell" }
            }
        })
    }

    fn provider_node(id: &str, provider: &str, model: &str) -> serde_json::Value {
        json!({
            "id": id,
            "data": {
                "name": "provider",
                "nodeType": "provider",
                "metadata": { "provider_name": provider, "model_name": model }
            }
        })
    }

    fn role_node(id: &str, name: &str, desc: &str) -> serde_json::Value {
        json!({
            "id": id,
            "data": {
                "name": "role",
                "nodeType": "role",
                "metadata": { "role_name": name, "role_description": desc }
            }
        })
    }

    fn gate_node(id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "data": {
                "name": "gate",
                "nodeType": "gate",
                "metadata": {}
            }
        })
    }

    fn edge(id: &str, source: &str, target: &str) -> serde_json::Value {
        json!({
            "id": id,
            "source": source,
            "target": target,
            "sourceHandle": "response",
            "targetHandle": "input"
        })
    }

    fn config_edge(id: &str, source: &str, target: &str, port: &str) -> serde_json::Value {
        json!({
            "id": id,
            "source": source,
            "target": target,
            "sourceHandle": "provider_config",
            "targetHandle": port
        })
    }

    #[test]
    fn compile_linear_graph() {
        let nodes = vec![agent_node("a1", "first"), agent_node("a2", "second")];
        let edges = vec![edge("e1", "a1", "a2")];

        let plan = compile("wf-1", &nodes, &edges).expect("should compile");
        assert_eq!(plan.levels.len(), 2);
        assert_eq!(plan.levels[0].len(), 1);
        assert_eq!(plan.levels[0][0].node_id, "a1");
        assert_eq!(plan.levels[1].len(), 1);
        assert_eq!(plan.levels[1][0].node_id, "a2");
    }

    #[test]
    fn compile_parallel_graph() {
        // a1 → a3, a2 → a3 (a1 and a2 are parallel)
        let nodes = vec![
            agent_node("a1", "left"),
            agent_node("a2", "right"),
            agent_node("a3", "merge"),
        ];
        let edges = vec![edge("e1", "a1", "a3"), edge("e2", "a2", "a3")];

        let plan = compile("wf-2", &nodes, &edges).expect("should compile");
        assert_eq!(plan.levels.len(), 2);
        assert_eq!(plan.levels[0].len(), 2); // a1 and a2 in parallel
        assert_eq!(plan.levels[1].len(), 1); // a3 after both
        assert_eq!(plan.levels[1][0].node_id, "a3");
    }

    #[test]
    fn compile_detects_cycle() {
        let nodes = vec![agent_node("a1", "one"), agent_node("a2", "two")];
        let edges = vec![edge("e1", "a1", "a2"), edge("e2", "a2", "a1")];

        let err = compile("wf-cycle", &nodes, &edges).expect_err("should detect cycle");
        assert!(matches!(err, CompileError::CycleDetected(_)));
    }

    #[test]
    fn compile_resolves_provider_config() {
        let nodes = vec![
            agent_node("a1", "agent"),
            provider_node("p1", "anthropic", "claude-sonnet-4-20250514"),
        ];
        let edges = vec![config_edge("e1", "p1", "a1", "config")];

        let plan = compile("wf-prov", &nodes, &edges).expect("should compile");
        // Provider node is excluded from execution levels
        assert_eq!(plan.levels.len(), 1);
        assert_eq!(plan.levels[0][0].node_id, "a1");
        // Config is resolved
        assert_eq!(
            plan.levels[0][0].config.provider.as_deref(),
            Some("anthropic")
        );
        assert_eq!(
            plan.levels[0][0].config.model.as_deref(),
            Some("claude-sonnet-4-20250514")
        );
    }

    #[test]
    fn compile_resolves_role_config() {
        let nodes = vec![
            agent_node("a1", "agent"),
            role_node("r1", "Researcher", "Research deeply"),
        ];
        let edges = vec![config_edge("e1", "r1", "a1", "role")];

        let plan = compile("wf-role", &nodes, &edges).expect("should compile");
        assert_eq!(plan.levels.len(), 1);
        assert_eq!(
            plan.levels[0][0].config.role_name.as_deref(),
            Some("Researcher")
        );
        assert_eq!(
            plan.levels[0][0].config.role_description.as_deref(),
            Some("Research deeply")
        );
    }

    #[test]
    fn compile_empty_graph_errors() {
        let nodes: Vec<serde_json::Value> = vec![];
        let edges: Vec<serde_json::Value> = vec![];

        let err = compile("wf-empty", &nodes, &edges).expect_err("should error");
        assert!(matches!(err, CompileError::EmptyGraph));
    }

    #[test]
    fn compile_config_only_graph_errors() {
        let nodes = vec![provider_node("p1", "openai", "gpt-4")];
        let edges: Vec<serde_json::Value> = vec![];

        let err = compile("wf-config-only", &nodes, &edges).expect_err("should error");
        assert!(matches!(err, CompileError::EmptyGraph));
    }

    #[test]
    fn compile_diamond_graph() {
        // a1 → a2, a1 → a3, a2 → a4, a3 → a4
        let nodes = vec![
            agent_node("a1", "start"),
            agent_node("a2", "left"),
            agent_node("a3", "right"),
            agent_node("a4", "merge"),
        ];
        let edges = vec![
            edge("e1", "a1", "a2"),
            edge("e2", "a1", "a3"),
            edge("e3", "a2", "a4"),
            edge("e4", "a3", "a4"),
        ];

        let plan = compile("wf-diamond", &nodes, &edges).expect("should compile");
        assert_eq!(plan.levels.len(), 3);
        assert_eq!(plan.levels[0].len(), 1); // a1
        assert_eq!(plan.levels[1].len(), 2); // a2, a3 parallel
        assert_eq!(plan.levels[2].len(), 1); // a4
    }

    #[test]
    fn compile_gate_node_included() {
        let nodes = vec![
            agent_node("a1", "check"),
            gate_node("g1"),
            agent_node("a2", "proceed"),
        ];
        let edges = vec![edge("e1", "a1", "g1"), edge("e2", "g1", "a2")];

        let plan = compile("wf-gate", &nodes, &edges).expect("should compile");
        assert_eq!(plan.levels.len(), 3);
        assert_eq!(plan.levels[1][0].node_type, NodeType::Gate);
    }

    #[test]
    fn compile_mixed_node_types() {
        let nodes = vec![
            agent_node("a1", "analyzer"),
            tool_node("t1", "shell"),
            agent_node("a2", "summarizer"),
        ];
        let edges = vec![edge("e1", "a1", "t1"), edge("e2", "t1", "a2")];

        let plan = compile("wf-mixed", &nodes, &edges).expect("should compile");
        assert_eq!(plan.levels.len(), 3);
        assert_eq!(plan.levels[0][0].node_type, NodeType::Agent);
        assert_eq!(plan.levels[1][0].node_type, NodeType::Tool);
        assert_eq!(plan.levels[2][0].node_type, NodeType::Agent);
    }

    #[test]
    fn edge_maps_populated() {
        let nodes = vec![agent_node("a1", "src"), agent_node("a2", "dst")];
        let edges = vec![edge("e1", "a1", "a2")];

        let plan = compile("wf-edges", &nodes, &edges).expect("should compile");

        let forward = plan.edges.get(&("a1".to_string(), "response".to_string()));
        assert!(forward.is_some());
        assert_eq!(
            forward.expect("edge exists")[0],
            ("a2".to_string(), "input".to_string())
        );

        let reverse = plan
            .reverse_edges
            .get(&("a2".to_string(), "input".to_string()));
        assert!(reverse.is_some());
        assert_eq!(
            reverse.expect("edge exists")[0],
            ("a1".to_string(), "response".to_string())
        );
    }

    // ── Executor tests ───────────────────────────────────────────────────

    struct MockDispatcher;

    #[async_trait::async_trait]
    impl super::StepDispatcher for MockDispatcher {
        async fn run_agent(
            &self,
            step: &super::ExecutionStep,
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
            Ok(format!("tool:{} result for {}", tool_name, input))
        }

        async fn send_channel(&self, channel_type: &str, _message: &str) -> anyhow::Result<()> {
            tracing::info!(channel = channel_type, "mock channel send");
            Ok(())
        }
    }

    #[tokio::test]
    async fn execute_single_agent() {
        let nodes = vec![agent_node("a1", "analyzer")];
        let edges: Vec<serde_json::Value> = vec![];
        let plan = compile("wf-1", &nodes, &edges).expect("compile");

        let run = super::execute(&plan, "Hello world", Arc::new(MockDispatcher))
            .await
            .expect("execute");

        assert_eq!(
            run.node_statuses.get("a1"),
            Some(&super::NodeStatus::Completed)
        );
        let output = run.outputs.get(&("a1".to_string(), "response".to_string()));
        assert!(output.is_some());
        let text = output.expect("output").as_str().expect("string");
        assert!(text.contains("analyzer"));
        assert!(text.contains("Hello world"));
    }

    #[tokio::test]
    async fn execute_agent_chain() {
        let nodes = vec![agent_node("a1", "first"), agent_node("a2", "second")];
        let edges = vec![edge("e1", "a1", "a2")];
        let plan = compile("wf-2", &nodes, &edges).expect("compile");

        let run = super::execute(&plan, "start", Arc::new(MockDispatcher))
            .await
            .expect("execute");

        // Both should complete
        assert_eq!(
            run.node_statuses.get("a1"),
            Some(&super::NodeStatus::Completed)
        );
        assert_eq!(
            run.node_statuses.get("a2"),
            Some(&super::NodeStatus::Completed)
        );

        // a2's response should contain a1's output
        let a2_out = run
            .outputs
            .get(&("a2".to_string(), "response".to_string()))
            .expect("a2 output")
            .as_str()
            .expect("string");
        assert!(a2_out.contains("[first] processed: start"));
    }

    #[tokio::test]
    async fn execute_tool_node() {
        let nodes = vec![tool_node("t1", "shell")];
        let edges: Vec<serde_json::Value> = vec![];
        let plan = compile("wf-tool", &nodes, &edges).expect("compile");

        let run = super::execute(&plan, "ls -la", Arc::new(MockDispatcher))
            .await
            .expect("execute");

        assert_eq!(
            run.node_statuses.get("t1"),
            Some(&super::NodeStatus::Completed)
        );
        let output = run
            .outputs
            .get(&("t1".to_string(), "result".to_string()))
            .expect("tool output")
            .as_str()
            .expect("string");
        assert!(output.contains("tool:shell"));
    }

    #[tokio::test]
    async fn execute_gate_routes_to_approved() {
        // a1 → gate → a2 (approved path)
        //              a3 (denied path — should be skipped)
        let nodes = vec![
            agent_node("a1", "check"),
            gate_node("g1"),
            agent_node("a2", "proceed"),
            agent_node("a3", "reject"),
        ];
        let edges = vec![
            edge("e1", "a1", "g1"),
            json!({
                "id": "e2", "source": "g1", "target": "a2",
                "sourceHandle": "approved", "targetHandle": "input"
            }),
            json!({
                "id": "e3", "source": "g1", "target": "a3",
                "sourceHandle": "denied", "targetHandle": "input"
            }),
        ];
        let plan = compile("wf-gate", &nodes, &edges).expect("compile");

        let run = super::execute(&plan, "review this", Arc::new(MockDispatcher))
            .await
            .expect("execute");

        assert_eq!(
            run.node_statuses.get("a2"),
            Some(&super::NodeStatus::Completed)
        );
        assert_eq!(
            run.node_statuses.get("a3"),
            Some(&super::NodeStatus::Skipped)
        );
    }

    #[tokio::test]
    async fn execute_parallel_fan_out() {
        // a1 → a2, a1 → a3 (both should execute)
        let nodes = vec![
            agent_node("a1", "splitter"),
            agent_node("a2", "left"),
            agent_node("a3", "right"),
        ];
        let edges = vec![edge("e1", "a1", "a2"), edge("e2", "a1", "a3")];
        let plan = compile("wf-parallel", &nodes, &edges).expect("compile");

        let run = super::execute(&plan, "split me", Arc::new(MockDispatcher))
            .await
            .expect("execute");

        assert_eq!(
            run.node_statuses.get("a2"),
            Some(&super::NodeStatus::Completed)
        );
        assert_eq!(
            run.node_statuses.get("a3"),
            Some(&super::NodeStatus::Completed)
        );
    }

    struct FailingDispatcher;

    #[async_trait::async_trait]
    impl super::StepDispatcher for FailingDispatcher {
        async fn run_agent(
            &self,
            _step: &super::ExecutionStep,
            _input: &str,
            _context: Option<&serde_json::Value>,
        ) -> anyhow::Result<String> {
            anyhow::bail!("agent failed")
        }

        async fn run_tool(&self, _: &str, _: &serde_json::Value) -> anyhow::Result<String> {
            anyhow::bail!("tool failed")
        }

        async fn send_channel(&self, _: &str, _: &str) -> anyhow::Result<()> {
            anyhow::bail!("channel failed")
        }
    }

    #[tokio::test]
    async fn execute_failed_node_marked_as_failed() {
        let nodes = vec![agent_node("a1", "fail")];
        let edges: Vec<serde_json::Value> = vec![];
        let plan = compile("wf-fail", &nodes, &edges).expect("compile");

        let run = super::execute(&plan, "boom", Arc::new(FailingDispatcher))
            .await
            .expect("execute completes even with failures");

        assert_eq!(
            run.node_statuses.get("a1"),
            Some(&super::NodeStatus::Failed)
        );
    }

    #[tokio::test]
    async fn execute_diamond_event_driven_unblocking() {
        // Diamond: a1 → a2, a1 → a3, a2 → a4, a3 → a4
        // Plus an independent node a5 at level 0 with a5 → a6.
        // a4 should complete as soon as a2 + a3 are done,
        // without waiting for a5/a6 — verifying event-driven unblocking.
        let nodes = vec![
            agent_node("a1", "start"),
            agent_node("a2", "left"),
            agent_node("a3", "right"),
            agent_node("a4", "merge"),
            agent_node("a5", "independent"),
            agent_node("a6", "after-independent"),
        ];
        let edges = vec![
            edge("e1", "a1", "a2"),
            edge("e2", "a1", "a3"),
            edge("e3", "a2", "a4"),
            edge("e4", "a3", "a4"),
            edge("e5", "a5", "a6"),
        ];

        let plan = compile("wf-diamond-event", &nodes, &edges).expect("compile");

        // Verify the dep graph is correct
        assert_eq!(plan.deps.get("a1").map(|s| s.len()), Some(0));
        assert_eq!(plan.deps.get("a5").map(|s| s.len()), Some(0));
        assert!(plan
            .deps
            .get("a4")
            .is_some_and(|s| s.contains("a2") && s.contains("a3")));
        assert!(plan.deps.get("a6").is_some_and(|s| s.contains("a5")));

        let run = super::execute(&plan, "diamond test", Arc::new(MockDispatcher))
            .await
            .expect("execute");

        // All nodes should complete
        for nid in &["a1", "a2", "a3", "a4", "a5", "a6"] {
            assert_eq!(
                run.node_statuses.get(*nid),
                Some(&super::NodeStatus::Completed),
                "node {} should be completed",
                nid
            );
        }

        // a4's output should contain a2 and a3's outputs (merged via input collection)
        let a4_out = run
            .outputs
            .get(&("a4".to_string(), "response".to_string()))
            .expect("a4 output")
            .as_str()
            .expect("string");
        assert!(a4_out.contains("merge"));
    }

    #[test]
    fn compile_populates_dep_graph() {
        // a1 → a2 → a3
        let nodes = vec![
            agent_node("a1", "first"),
            agent_node("a2", "second"),
            agent_node("a3", "third"),
        ];
        let edges = vec![edge("e1", "a1", "a2"), edge("e2", "a2", "a3")];

        let plan = compile("wf-deps", &nodes, &edges).expect("compile");

        // a1 has no deps
        assert!(plan.deps.get("a1").is_some_and(|s| s.is_empty()));
        // a2 depends on a1
        assert!(plan.deps.get("a2").is_some_and(|s| s.contains("a1")));
        // a3 depends on a2
        assert!(plan.deps.get("a3").is_some_and(|s| s.contains("a2")));

        // Dependents: a1 → {a2}, a2 → {a3}
        assert!(plan.dependents.get("a1").is_some_and(|s| s.contains("a2")));
        assert!(plan.dependents.get("a2").is_some_and(|s| s.contains("a3")));
    }

    #[tokio::test]
    async fn execute_failed_node_unblocks_dependents() {
        // a1 → a2 where a1 fails. a2 should still run (with empty input).
        let nodes = vec![agent_node("a1", "fail"), agent_node("a2", "after")];
        let edges = vec![edge("e1", "a1", "a2")];
        let plan = compile("wf-fail-chain", &nodes, &edges).expect("compile");

        let run = super::execute(&plan, "boom", Arc::new(FailingDispatcher))
            .await
            .expect("execute completes");

        assert_eq!(
            run.node_statuses.get("a1"),
            Some(&super::NodeStatus::Failed)
        );
        // a2 should also run (and fail, since FailingDispatcher fails everything)
        assert_eq!(
            run.node_statuses.get("a2"),
            Some(&super::NodeStatus::Failed)
        );
    }

    /// Verifies that independent nodes execute concurrently via JoinSet.
    ///
    /// Uses a dispatcher with a sleep to prove that two independent nodes
    /// overlap in time — total duration should be ~sleep_time, not ~2x.
    #[tokio::test]
    async fn execute_concurrent_independent_nodes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        /// Dispatcher that sleeps and tracks peak concurrency.
        struct ConcurrencyTracker {
            active: AtomicUsize,
            peak: AtomicUsize,
        }

        #[async_trait::async_trait]
        impl super::StepDispatcher for ConcurrencyTracker {
            async fn run_agent(
                &self,
                step: &super::ExecutionStep,
                input: &str,
                _context: Option<&serde_json::Value>,
            ) -> anyhow::Result<String> {
                let prev = self.active.fetch_add(1, Ordering::SeqCst);
                let current = prev + 1;
                // Update peak if this is the highest so far.
                self.peak.fetch_max(current, Ordering::SeqCst);

                tokio::time::sleep(Duration::from_millis(50)).await;

                self.active.fetch_sub(1, Ordering::SeqCst);
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

        // Three independent root nodes — should all run concurrently.
        let nodes = vec![
            agent_node("a1", "alpha"),
            agent_node("a2", "beta"),
            agent_node("a3", "gamma"),
        ];
        let edges: Vec<serde_json::Value> = vec![];
        let plan = compile("wf-concurrent", &nodes, &edges).expect("compile");

        let tracker = Arc::new(ConcurrencyTracker {
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        });
        let tracker_ref = Arc::clone(&tracker);

        let start = Instant::now();
        let run = super::execute(&plan, "go", tracker_ref)
            .await
            .expect("execute");
        let elapsed = start.elapsed();

        // All nodes should complete.
        for nid in &["a1", "a2", "a3"] {
            assert_eq!(
                run.node_statuses.get(*nid),
                Some(&super::NodeStatus::Completed),
                "node {nid} should be completed"
            );
        }

        // Peak concurrency should be > 1, proving parallel execution.
        let peak = tracker.peak.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            peak >= 2,
            "expected peak concurrency >= 2, got {peak} (nodes ran sequentially)"
        );

        // Total time should be closer to 50ms than 150ms (3 * 50ms sequential).
        assert!(
            elapsed < Duration::from_millis(120),
            "expected parallel execution under 120ms, took {:?}",
            elapsed
        );
    }
}
