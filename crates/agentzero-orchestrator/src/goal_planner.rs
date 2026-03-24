//! Goal decomposition — turn a natural language goal into a workflow graph.
//!
//! The [`GoalPlanner`] takes a goal string and uses an LLM to produce a
//! [`PlannedWorkflow`] — a set of agent nodes with dependencies that can
//! be compiled into an [`ExecutionPlan`] and run by the workflow executor.

use agentzero_core::{Provider, ToolSummary};
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
    /// Optional tool names this agent should have access to.
    /// When present, the dispatcher filters the tool set to these + always-on tools.
    /// When empty, all tools are available (or keyword-matched by task description).
    #[serde(default)]
    pub tool_hints: Vec<String>,
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
                        "tool_hints": node.tool_hints,
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
      "sandbox_level": "worktree",
      "tool_hints": ["shell", "read_file", "web_fetch"]
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
- tool_hints: list of tool names this agent needs from the available tools catalog below. Include only tools relevant to the task. Leave empty if unsure or if the agent only needs the LLM (no tools).
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

// ── GoalPlanner ─────────────────────────────────────────────────────────────

/// Decomposes a natural-language goal into a workflow graph via an LLM call.
///
/// The planner sends the goal plus a catalog of available tools to the LLM,
/// which returns a [`PlannedWorkflow`] with per-node `tool_hints` so each
/// agent gets only the tools it needs.
pub struct GoalPlanner {
    provider: Box<dyn Provider>,
}

impl GoalPlanner {
    /// Create a planner backed by the given LLM provider.
    pub fn new(provider: Box<dyn Provider>) -> Self {
        Self { provider }
    }

    /// Decompose `goal` into a multi-agent workflow.
    ///
    /// `available_tools` is included in the prompt so the LLM can assign
    /// `tool_hints` per node from real tool names.
    pub async fn plan(
        &self,
        goal: &str,
        available_tools: &[ToolSummary],
    ) -> anyhow::Result<PlannedWorkflow> {
        let tool_catalog: String = available_tools
            .iter()
            .map(|t| format!("- {}: {}", t.name, t.description))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = if tool_catalog.is_empty() {
            format!("{GOAL_PLANNER_PROMPT}\n\nGoal: {goal}")
        } else {
            format!("{GOAL_PLANNER_PROMPT}\n\nAvailable tools:\n{tool_catalog}\n\nGoal: {goal}")
        };

        let result = self.provider.complete(&prompt).await?;
        parse_planner_response(&result.output_text)
    }
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
                    tool_hints: vec![],
                },
                PlannedNode {
                    id: "n2".to_string(),
                    name: "Second".to_string(),
                    task: "Do second".to_string(),
                    depends_on: vec!["n1".to_string()],
                    file_scopes: vec![],
                    sandbox_level: "worktree".to_string(),
                    tool_hints: vec![],
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
                    tool_hints: vec![],
                },
                PlannedNode {
                    id: "b".to_string(),
                    name: "B".to_string(),
                    task: "Task B".to_string(),
                    depends_on: vec![],
                    file_scopes: vec![],
                    sandbox_level: "worktree".to_string(),
                    tool_hints: vec![],
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
        assert!(node.tool_hints.is_empty());
    }

    #[test]
    fn tool_hints_deserialized_when_present() {
        let json = r#"{"id":"n1","name":"X","task":"Y","tool_hints":["shell","web_fetch"]}"#;
        let node: PlannedNode = serde_json::from_str(json).expect("parse");
        assert_eq!(node.tool_hints, vec!["shell", "web_fetch"]);
    }

    #[test]
    fn tool_hints_default_empty_when_missing() {
        let response = r#"{"title":"T","nodes":[{"id":"n1","name":"X","task":"Y"}]}"#;
        let plan = parse_planner_response(response).expect("parse");
        assert!(plan.nodes[0].tool_hints.is_empty());
    }

    #[test]
    fn tool_hints_in_workflow_json_metadata() {
        let plan = PlannedWorkflow {
            title: "T".to_string(),
            nodes: vec![PlannedNode {
                id: "n1".to_string(),
                name: "Worker".to_string(),
                task: "Do work".to_string(),
                depends_on: vec![],
                file_scopes: vec![],
                sandbox_level: "worktree".to_string(),
                tool_hints: vec!["shell".to_string(), "read_file".to_string()],
            }],
        };
        let (nodes, _) = plan.to_workflow_json();
        let hints = &nodes[0]["data"]["metadata"]["tool_hints"];
        assert_eq!(hints, &serde_json::json!(["shell", "read_file"]));
    }

    #[tokio::test]
    async fn goal_planner_calls_provider_and_parses() {
        use agentzero_core::ChatResult;
        use async_trait::async_trait;

        struct MockProvider;

        #[async_trait]
        impl Provider for MockProvider {
            async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
                Ok(ChatResult {
                    output_text: r#"{
                        "title": "Summarize Video",
                        "nodes": [
                            {"id":"n1","name":"Downloader","task":"Download the video","tool_hints":["shell","web_fetch"]},
                            {"id":"n2","name":"Summarizer","task":"Summarize the transcript","depends_on":["n1"],"tool_hints":[]}
                        ]
                    }"#
                    .to_string(),
                    tool_calls: vec![],
                    stop_reason: None,
                    input_tokens: 0,
                    output_tokens: 0,
                })
            }
        }

        let planner = GoalPlanner::new(Box::new(MockProvider));
        let plan = planner
            .plan("summarize this video", &[])
            .await
            .expect("plan");
        assert_eq!(plan.title, "Summarize Video");
        assert_eq!(plan.nodes.len(), 2);
        assert_eq!(plan.nodes[0].tool_hints, vec!["shell", "web_fetch"]);
        assert!(plan.nodes[1].tool_hints.is_empty());
        assert_eq!(plan.nodes[1].depends_on, vec!["n1"]);
    }

    #[tokio::test]
    async fn goal_planner_includes_tool_catalog_in_prompt() {
        use agentzero_core::ChatResult;
        use async_trait::async_trait;
        use std::sync::{Arc, Mutex};

        struct CapturingProvider {
            captured: Arc<Mutex<String>>,
        }

        #[async_trait]
        impl Provider for CapturingProvider {
            async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
                *self.captured.lock().expect("lock poisoned") = prompt.to_string();
                Ok(ChatResult {
                    output_text: r#"{"title":"T","nodes":[]}"#.to_string(),
                    tool_calls: vec![],
                    stop_reason: None,
                    input_tokens: 0,
                    output_tokens: 0,
                })
            }
        }

        let captured = Arc::new(Mutex::new(String::new()));
        let provider = CapturingProvider {
            captured: Arc::clone(&captured),
        };
        let planner = GoalPlanner::new(Box::new(provider));

        let tools = vec![
            ToolSummary {
                name: "shell".to_string(),
                description: "Execute a shell command".to_string(),
            },
            ToolSummary {
                name: "web_fetch".to_string(),
                description: "Fetch a URL".to_string(),
            },
        ];

        planner.plan("test goal", &tools).await.expect("plan");

        let prompt = captured.lock().expect("lock poisoned").clone();
        assert!(
            prompt.contains("- shell: Execute a shell command"),
            "prompt should include tool catalog"
        );
        assert!(
            prompt.contains("- web_fetch: Fetch a URL"),
            "prompt should include tool catalog"
        );
        assert!(
            prompt.contains("Goal: test goal"),
            "prompt should include the goal"
        );
    }

    #[test]
    fn invalid_json_returns_error() {
        let result = parse_planner_response("not json at all");
        assert!(result.is_err());
    }
}
