//! Goal decomposition — turn a natural language goal into a workflow graph.
//!
//! The [`GoalPlanner`] takes a goal string and uses an LLM to produce a
//! [`PlannedWorkflow`] — a set of agent nodes with dependencies that can
//! be compiled into an [`ExecutionPlan`] and run by the workflow executor.

use serde::{Deserialize, Serialize};

// ── Types ────────────────────────────────────────────────────────────────────

/// A planned agent node produced by goal decomposition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedNode {
    /// Unique node identifier.
    pub id: String,
    /// Human-readable agent name/role.
    pub name: String,
    /// Task description for the agent's system prompt.
    pub task: String,
    /// IDs of nodes this node depends on (must complete first).
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Estimated file paths this agent will modify.
    #[serde(default)]
    pub file_scopes: Vec<String>,
    /// Sandbox isolation level for this node.
    #[serde(default = "default_sandbox_level")]
    pub sandbox_level: String,
}

fn default_sandbox_level() -> String {
    "worktree".to_string()
}

/// The output of goal decomposition — a complete workflow ready for compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedWorkflow {
    /// Human-readable title for the workflow.
    pub title: String,
    /// The decomposed agent nodes.
    pub nodes: Vec<PlannedNode>,
}

impl PlannedWorkflow {
    /// Convert to ReactFlow-compatible JSON (nodes + edges) for the workflow compiler.
    pub fn to_workflow_json(&self) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut edge_id = 0;

        for node in &self.nodes {
            nodes.push(serde_json::json!({
                "id": node.id,
                "data": {
                    "name": node.name,
                    "nodeType": "agent",
                    "metadata": {
                        "system_prompt": node.task,
                        "file_scopes": node.file_scopes,
                        "sandbox_level": node.sandbox_level,
                    }
                }
            }));

            for dep_id in &node.depends_on {
                edges.push(serde_json::json!({
                    "id": format!("e{edge_id}"),
                    "source": dep_id,
                    "target": node.id,
                    "sourceHandle": "response",
                    "targetHandle": "input"
                }));
                edge_id += 1;
            }
        }

        (nodes, edges)
    }
}

/// The system prompt template for goal decomposition.
pub const GOAL_PLANNER_PROMPT: &str = r#"You are a task decomposition agent. Given a goal, break it down into a set of agent tasks that can be executed in parallel where possible.

Output a JSON object with this exact structure:
{
  "title": "Short workflow title",
  "nodes": [
    {
      "id": "unique-id",
      "name": "Agent Role Name",
      "task": "Detailed task description for the agent",
      "depends_on": ["id-of-dependency"],
      "file_scopes": ["src/file.rs", "src/other.rs"],
      "sandbox_level": "worktree"
    }
  ]
}

Rules:
- Each node is an independent agent that will run in an isolated sandbox
- Use depends_on to express ordering constraints (agent B needs output from agent A)
- Nodes with no dependencies run in parallel
- Keep the number of nodes reasonable (2-8 for most goals)
- file_scopes should list files the agent is likely to modify (for conflict detection)
- sandbox_level is always "worktree" for now
- Output ONLY the JSON object, no markdown fences or explanation"#;

/// Parse a planner LLM response into a `PlannedWorkflow`.
///
/// Handles common LLM response quirks: markdown code fences, leading/trailing text.
pub fn parse_planner_response(response: &str) -> anyhow::Result<PlannedWorkflow> {
    // Strip markdown code fences if present.
    let json_str = extract_json(response);

    serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("failed to parse planner response as PlannedWorkflow: {e}"))
}

/// Extract JSON from an LLM response that may be wrapped in markdown fences.
fn extract_json(response: &str) -> &str {
    let trimmed = response.trim();

    // Try to find ```json ... ``` block.
    if let Some(start) = trimmed.find("```json") {
        let after_fence = &trimmed[start + 7..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }

    // Try to find ``` ... ``` block.
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }

    // Try to find { ... } directly.
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return &trimmed[start..=end];
        }
    }

    trimmed
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clean_json_response() {
        let response = r#"{
            "title": "Build REST API",
            "nodes": [
                {
                    "id": "n1",
                    "name": "API Designer",
                    "task": "Design the REST API endpoints",
                    "depends_on": [],
                    "file_scopes": ["src/api.rs"],
                    "sandbox_level": "worktree"
                },
                {
                    "id": "n2",
                    "name": "Auth Developer",
                    "task": "Implement authentication middleware",
                    "depends_on": ["n1"],
                    "file_scopes": ["src/auth.rs"],
                    "sandbox_level": "worktree"
                }
            ]
        }"#;

        let plan = parse_planner_response(response).expect("should parse");
        assert_eq!(plan.title, "Build REST API");
        assert_eq!(plan.nodes.len(), 2);
        assert_eq!(plan.nodes[0].id, "n1");
        assert_eq!(plan.nodes[1].depends_on, vec!["n1"]);
    }

    #[test]
    fn parse_markdown_fenced_response() {
        let response = r#"Here's the decomposition:

```json
{
    "title": "Test Plan",
    "nodes": [
        {
            "id": "n1",
            "name": "Worker",
            "task": "Do work",
            "depends_on": [],
            "file_scopes": []
        }
    ]
}
```

This should work well."#;

        let plan = parse_planner_response(response).expect("should parse");
        assert_eq!(plan.title, "Test Plan");
        assert_eq!(plan.nodes.len(), 1);
    }

    #[test]
    fn parse_bare_fenced_response() {
        let response = "```\n{\"title\":\"T\",\"nodes\":[]}\n```";
        let plan = parse_planner_response(response).expect("should parse");
        assert_eq!(plan.title, "T");
    }

    #[test]
    fn parse_response_with_leading_text() {
        let response = "Sure! Here's the plan:\n{\"title\":\"Plan\",\"nodes\":[]}";
        let plan = parse_planner_response(response).expect("should parse");
        assert_eq!(plan.title, "Plan");
    }

    #[test]
    fn to_workflow_json_generates_nodes_and_edges() {
        let plan = PlannedWorkflow {
            title: "Test".to_string(),
            nodes: vec![
                PlannedNode {
                    id: "n1".to_string(),
                    name: "First".to_string(),
                    task: "Do first".to_string(),
                    depends_on: vec![],
                    file_scopes: vec!["src/a.rs".to_string()],
                    sandbox_level: "worktree".to_string(),
                },
                PlannedNode {
                    id: "n2".to_string(),
                    name: "Second".to_string(),
                    task: "Do second".to_string(),
                    depends_on: vec!["n1".to_string()],
                    file_scopes: vec![],
                    sandbox_level: "worktree".to_string(),
                },
            ],
        };

        let (nodes, edges) = plan.to_workflow_json();
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["source"], "n1");
        assert_eq!(edges[0]["target"], "n2");
    }

    #[test]
    fn to_workflow_json_parallel_nodes_no_edges() {
        let plan = PlannedWorkflow {
            title: "Parallel".to_string(),
            nodes: vec![
                PlannedNode {
                    id: "a".to_string(),
                    name: "A".to_string(),
                    task: "Task A".to_string(),
                    depends_on: vec![],
                    file_scopes: vec![],
                    sandbox_level: "worktree".to_string(),
                },
                PlannedNode {
                    id: "b".to_string(),
                    name: "B".to_string(),
                    task: "Task B".to_string(),
                    depends_on: vec![],
                    file_scopes: vec![],
                    sandbox_level: "worktree".to_string(),
                },
            ],
        };

        let (nodes, edges) = plan.to_workflow_json();
        assert_eq!(nodes.len(), 2);
        assert!(edges.is_empty(), "parallel nodes should have no edges");
    }

    #[test]
    fn default_sandbox_level_is_worktree() {
        let json = r#"{"id":"n1","name":"X","task":"Y"}"#;
        let node: PlannedNode = serde_json::from_str(json).expect("parse");
        assert_eq!(node.sandbox_level, "worktree");
        assert!(node.depends_on.is_empty());
        assert!(node.file_scopes.is_empty());
    }

    #[test]
    fn invalid_json_returns_error() {
        let result = parse_planner_response("not json at all");
        assert!(result.is_err());
    }
}
