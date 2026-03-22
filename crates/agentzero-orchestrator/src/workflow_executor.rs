//! Workflow execution engine — compiles visual workflow graphs into executable
//! plans and runs them step-by-step with topological ordering.
//!
//! The compiler resolves config nodes (Provider, Role) at build time, produces
//! parallelizable execution levels, and the executor dispatches each node type
//! to the appropriate runtime (agent loop, tool execute, channel send).

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

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
}

impl NodeType {
    /// Parse from the node_type string used in the UI.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "agent" => Some(Self::Agent),
            "tool" => Some(Self::Tool),
            "channel" => Some(Self::Channel),
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
        matches!(self, Self::Schedule | Self::Channel)
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

    Ok(ExecutionPlan {
        workflow_id: workflow_id.to_string(),
        levels,
        edges: edge_map,
        reverse_edges: reverse_edge_map,
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
