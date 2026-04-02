use agentzero_config::AgentZeroConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The graph model exchanged between backend and frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphModel {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub viewport: Viewport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub type_id: String,
    pub position: Position,
    pub data: Value,
    /// When set, this node is a child of the parent node (group containment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Width hint for group/container nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// Height hint for group/container nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub source_port: String,
    pub target: String,
    pub target_port: String,
    pub edge_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// Validation error returned by the backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub node_id: Option<String>,
    pub field: Option<String>,
    pub message: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

impl Default for GraphModel {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            viewport: Viewport {
                x: 0.0,
                y: 0.0,
                zoom: 1.0,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// AgentZeroConfig -> GraphModel (import)
// ---------------------------------------------------------------------------

/// Convert an `AgentZeroConfig` into an agent-centric visual graph.
///
/// The agent is placed at the center. Provider, security, and autonomy nodes
/// surround it, with tool nodes fanning out to the left.
pub fn config_to_graph(config: &AgentZeroConfig) -> GraphModel {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut next_id: u32 = 1;

    let mut id = || -> String {
        let s = format!("n{next_id}");
        next_id += 1;
        s
    };

    // ── Center: Agent node(s) ──────────────────────────────────
    // If no agents defined, create a starter agent from provider defaults
    let agent_configs: Vec<(String, serde_json::Value)> = if config.agents.is_empty() {
        vec![(
            "main".to_string(),
            serde_json::json!({
                "name": "main",
                "provider": config.provider.base_url,
                "model": config.provider.model,
                "system_prompt": "",
                "max_depth": 3,
                "agentic": true,
                "max_iterations": config.agent.max_tool_iterations,
                "privacy_boundary": "inherit",
                "max_tokens": null,
                "max_cost_usd": null,
                "allowed_tools": [],
            }),
        )]
    } else {
        config
            .agents
            .iter()
            .map(|(name, ac)| {
                (
                    name.clone(),
                    serde_json::json!({
                        "name": name,
                        "provider": ac.provider,
                        "model": ac.model,
                        "system_prompt": ac.system_prompt.as_deref().unwrap_or(""),
                        "max_depth": ac.max_depth,
                        "agentic": ac.agentic,
                        "max_iterations": ac.max_iterations,
                        "privacy_boundary": ac.privacy_boundary,
                        "max_tokens": ac.max_tokens,
                        "max_cost_usd": ac.max_cost_usd,
                        "allowed_tools": ac.allowed_tools,
                    }),
                )
            })
            .collect()
    };

    let mut agent_ids = Vec::new();
    for (i, (_name, data)) in agent_configs.iter().enumerate() {
        let agent_id = id();
        nodes.push(GraphNode {
            id: agent_id.clone(),
            type_id: "agent".to_string(),
            position: Position {
                x: 100.0 + (i as f64) * 700.0,
                y: 50.0,
            },
            data: data.clone(),
            parent_id: None,
            width: Some(560.0),
            height: Some(420.0),
        });
        agent_ids.push(agent_id);
    }

    // ── Left: Provider node ────────────────────────────────────
    let provider_id = id();
    nodes.push(GraphNode {
        id: provider_id.clone(),
        type_id: "provider".to_string(),
        position: Position { x: -250.0, y: 80.0 },
        data: serde_json::json!({
            "kind": config.provider.kind,
            "base_url": config.provider.base_url,
            "model": config.provider.model,
            "default_temperature": config.provider.default_temperature,
        }),
        parent_id: None,
        width: None,
        height: None,
    });
    for aid in &agent_ids {
        edges.push(GraphEdge {
            id: format!("e{}", edges.len() + 1),
            source: provider_id.clone(),
            source_port: "agent_out".to_string(),
            target: aid.clone(),
            target_port: "route_in".to_string(),
            edge_type: "provider".to_string(),
        });
    }

    // ── Below-left: Security policy ────────────────────────────
    let security_id = id();
    nodes.push(GraphNode {
        id: security_id.clone(),
        type_id: "security_policy".to_string(),
        position: Position {
            x: -250.0,
            y: 300.0,
        },
        data: serde_json::json!({
            "enable_write_file": config.security.write_file.enabled,
            "enable_git": !config.security.allowed_commands.is_empty(),
            "enable_web_search": false,
            "enable_browser": false,
            "enable_browser_open": false,
            "enable_http_request": false,
            "enable_web_fetch": false,
            "enable_url_validation": false,
            "enable_html_extract": false,
            "enable_cron": false,
            "enable_agents_ipc": true,
            "enable_mcp": config.security.mcp.enabled,
            "enable_composio": false,
            "enable_pushover": false,
            "enable_wasm_plugins": config.security.plugin.wasm_enabled,
            "read_file_max_bytes": config.security.read_file.max_read_bytes,
            "read_file_allow_binary": config.security.read_file.allow_binary,
            "write_file_max_bytes": config.security.write_file.max_write_bytes,
            "shell_max_args": config.security.shell.max_args,
            "shell_max_arg_length": config.security.shell.max_arg_length,
            "shell_max_output_bytes": config.security.shell.max_output_bytes,
            "allowed_commands": config.security.allowed_commands,
        }),
        parent_id: None,
        width: None,
        height: None,
    });

    // ── Right: Autonomy ────────────────────────────────────────
    nodes.push(GraphNode {
        id: id(),
        type_id: "autonomy".to_string(),
        position: Position {
            x: 400.0 + (agent_configs.len() as f64) * 400.0,
            y: 350.0,
        },
        data: serde_json::json!({
            "level": config.autonomy.level,
            "max_actions_per_hour": config.autonomy.max_actions_per_hour,
            "max_cost_per_day_cents": config.autonomy.max_cost_per_day_cents,
            "require_approval_for_medium_risk": config.autonomy.require_approval_for_medium_risk,
            "block_high_risk_commands": config.autonomy.block_high_risk_commands,
        }),
        parent_id: None,
        width: None,
        height: None,
    });

    // ── Starter tools as children of first agent ────────────────
    let starter_tools = [
        ("read_file", "Read file contents", "file"),
        ("shell", "Execute shell commands", "system"),
        ("glob_search", "Find files by glob pattern", "file"),
        ("content_search", "Search file contents", "file"),
        ("memory_store", "Store a memory", "memory"),
        ("memory_recall", "Recall memories", "memory"),
        ("task_plan", "Create and manage task plans", "orchestration"),
    ];

    if let Some(first_agent) = agent_ids.first() {
        for (i, (name, desc, category)) in starter_tools.iter().enumerate() {
            let tool_id = id();
            let col = i % 3;
            let row = i / 3;
            // Positions are relative to the parent agent node
            nodes.push(GraphNode {
                id: tool_id.clone(),
                type_id: "tool".to_string(),
                position: Position {
                    x: 20.0 + (col as f64) * 175.0,
                    y: 70.0 + (row as f64) * 75.0,
                },
                data: serde_json::json!({
                    "name": name,
                    "enabled": true,
                    "description": desc,
                    "category": category,
                }),
                parent_id: Some(first_agent.clone()),
                width: None,
                height: None,
            });
        }

        // Security -> first agent (external edge, not containment)
        edges.push(GraphEdge {
            id: format!("e{}", edges.len() + 1),
            source: security_id,
            source_port: "tools_out".to_string(),
            target: first_agent.clone(),
            target_port: "security_in".to_string(),
            edge_type: "security".to_string(),
        });
    }

    // ── Model routes ───────────────────────────────────────────
    for (i, route) in config.model_routes.iter().enumerate() {
        let route_id = id();
        nodes.push(GraphNode {
            id: route_id.clone(),
            type_id: "model_route".to_string(),
            position: Position {
                x: 700.0 + (i as f64) * 200.0,
                y: -50.0,
            },
            data: serde_json::json!({
                "hint": route.hint,
                "provider": route.provider,
                "model": route.model,
                "max_tokens": route.max_tokens,
            }),
            parent_id: None,
            width: None,
            height: None,
        });

        for (j, rule) in config.query_classification.rules.iter().enumerate() {
            if rule.hint == route.hint {
                let existing_rule = nodes.iter().find(|n| {
                    n.type_id == "classification_rule"
                        && n.data.get("hint").and_then(|v| v.as_str()) == Some(&rule.hint)
                        && n.data.get("_rule_index").and_then(|v| v.as_u64()) == Some(j as u64)
                });

                let source_id = if let Some(rule_node) = existing_rule {
                    rule_node.id.clone()
                } else {
                    let cid = id();
                    nodes.push(GraphNode {
                        id: cid.clone(),
                        type_id: "classification_rule".to_string(),
                        position: Position {
                            x: 700.0 + (j as f64) * 200.0,
                            y: -200.0,
                        },
                        data: serde_json::json!({
                            "hint": rule.hint,
                            "keywords": rule.keywords,
                            "patterns": rule.patterns,
                            "priority": rule.priority,
                            "_rule_index": j,
                        }),
                        parent_id: None,
                        width: None,
                        height: None,
                    });
                    cid
                };

                edges.push(GraphEdge {
                    id: format!("rule_{}_{}", i, j),
                    source: source_id,
                    source_port: "route_out".to_string(),
                    target: route_id.clone(),
                    target_port: "rule_in".to_string(),
                    edge_type: "classification".to_string(),
                });
            }
        }
    }

    GraphModel {
        nodes,
        edges,
        viewport: Viewport {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        },
    }
}

// ---------------------------------------------------------------------------
// GraphModel -> AgentZeroConfig (export)
// ---------------------------------------------------------------------------

/// Convert a graph back into an `AgentZeroConfig`.
///
/// Starts from `AgentZeroConfig::default()` and overlays the graph values.
pub fn graph_to_config(graph: &GraphModel) -> anyhow::Result<AgentZeroConfig> {
    let mut config = AgentZeroConfig::default();

    for node in &graph.nodes {
        match node.type_id.as_str() {
            "provider" => apply_provider_node(&mut config, &node.data),
            "security_policy" => apply_security_policy_node(&mut config, &node.data),
            "autonomy" => apply_autonomy_node(&mut config, &node.data),
            "agent" => apply_agent_node(&mut config, &node.data),
            "model_route" => apply_model_route_node(&mut config, &node.data),
            "classification_rule" => apply_classification_rule_node(&mut config, &node.data),
            "tool" | "depth_policy" => { /* tools and depth policies are derived from edges/security */
            }
            _ => { /* unknown node types are silently ignored */ }
        }
    }

    Ok(config)
}

fn apply_provider_node(config: &mut AgentZeroConfig, data: &Value) {
    if let Some(kind) = data.get("kind").and_then(|v| v.as_str()) {
        config.provider.kind = kind.to_string();
    }
    if let Some(url) = data.get("base_url").and_then(|v| v.as_str()) {
        config.provider.base_url = url.to_string();
    }
    if let Some(model) = data.get("model").and_then(|v| v.as_str()) {
        config.provider.model = model.to_string();
    }
    if let Some(temp) = data.get("default_temperature").and_then(|v| v.as_f64()) {
        config.provider.default_temperature = temp;
    }
}

fn apply_security_policy_node(config: &mut AgentZeroConfig, data: &Value) {
    if let Some(v) = data.get("enable_write_file").and_then(|v| v.as_bool()) {
        config.security.write_file.enabled = v;
    }
    if let Some(v) = data.get("enable_mcp").and_then(|v| v.as_bool()) {
        config.security.mcp.enabled = v;
    }
    if let Some(v) = data.get("enable_wasm_plugins").and_then(|v| v.as_bool()) {
        config.security.plugin.wasm_enabled = v;
    }
    if let Some(v) = data.get("read_file_max_bytes").and_then(|v| v.as_u64()) {
        config.security.read_file.max_read_bytes = v;
    }
    if let Some(v) = data.get("read_file_allow_binary").and_then(|v| v.as_bool()) {
        config.security.read_file.allow_binary = v;
    }
    if let Some(v) = data.get("write_file_max_bytes").and_then(|v| v.as_u64()) {
        config.security.write_file.max_write_bytes = v;
    }
    if let Some(v) = data.get("shell_max_args").and_then(|v| v.as_u64()) {
        config.security.shell.max_args = v as usize;
    }
    if let Some(v) = data.get("shell_max_arg_length").and_then(|v| v.as_u64()) {
        config.security.shell.max_arg_length = v as usize;
    }
    if let Some(v) = data.get("shell_max_output_bytes").and_then(|v| v.as_u64()) {
        config.security.shell.max_output_bytes = v as usize;
    }
    if let Some(arr) = data.get("allowed_commands").and_then(|v| v.as_array()) {
        config.security.allowed_commands = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    }
}

fn apply_autonomy_node(config: &mut AgentZeroConfig, data: &Value) {
    if let Some(level) = data.get("level").and_then(|v| v.as_str()) {
        config.autonomy.level = level.to_string();
    }
    if let Some(v) = data.get("max_actions_per_hour").and_then(|v| v.as_u64()) {
        config.autonomy.max_actions_per_hour = v as u32;
    }
    if let Some(v) = data.get("max_cost_per_day_cents").and_then(|v| v.as_u64()) {
        config.autonomy.max_cost_per_day_cents = v as u32;
    }
    if let Some(v) = data
        .get("require_approval_for_medium_risk")
        .and_then(|v| v.as_bool())
    {
        config.autonomy.require_approval_for_medium_risk = v;
    }
    if let Some(v) = data
        .get("block_high_risk_commands")
        .and_then(|v| v.as_bool())
    {
        config.autonomy.block_high_risk_commands = v;
    }
}

fn apply_agent_node(config: &mut AgentZeroConfig, data: &Value) {
    let name = data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unnamed")
        .to_string();

    let agent = agentzero_config::DelegateAgentConfig {
        provider: data
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        model: data
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        system_prompt: data
            .get("system_prompt")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        max_depth: data.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize,
        agentic: data
            .get("agentic")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        allowed_tools: data
            .get("allowed_tools")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        max_iterations: data
            .get("max_iterations")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize,
        privacy_boundary: data
            .get("privacy_boundary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        max_tokens: data.get("max_tokens").and_then(|v| v.as_u64()),
        max_cost_usd: data.get("max_cost_usd").and_then(|v| v.as_f64()),
        temperature: data.get("temperature").and_then(|v| v.as_f64()),
        api_key: None,
        allowed_providers: Vec::new(),
        blocked_providers: Vec::new(),
        instruction_method: agentzero_core::delegation::InstructionMethod::default(),
    };

    config.agents.insert(name, agent);
}

fn apply_model_route_node(config: &mut AgentZeroConfig, data: &Value) {
    config.model_routes.push(agentzero_config::ModelRoute {
        hint: data
            .get("hint")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        provider: data
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        model: data
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        max_tokens: data
            .get("max_tokens")
            .and_then(|v| v.as_u64().map(|n| n as usize)),
        api_key: None,
        transport: None,
        privacy_level: data
            .get("privacy_level")
            .and_then(|v| v.as_str())
            .map(String::from),
    });
}

fn apply_classification_rule_node(config: &mut AgentZeroConfig, data: &Value) {
    config
        .query_classification
        .rules
        .push(agentzero_config::QueryClassificationRule {
            hint: data
                .get("hint")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            keywords: data
                .get("keywords")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            patterns: data
                .get("patterns")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            min_length: None,
            max_length: None,
            priority: data.get("priority").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        });
}

/// Convert a graph to TOML.
pub fn graph_to_toml(graph: &GraphModel) -> anyhow::Result<String> {
    let config = graph_to_config(graph)?;
    let toml_str = toml::to_string_pretty(&config)
        .map_err(|e| anyhow::anyhow!("TOML serialization failed: {e}"))?;
    Ok(toml_str)
}

/// Parse TOML into a graph.
pub fn toml_to_graph(toml_str: &str) -> anyhow::Result<GraphModel> {
    let config: AgentZeroConfig =
        toml::from_str(toml_str).map_err(|e| anyhow::anyhow!("TOML parse failed: {e}"))?;
    Ok(config_to_graph(&config))
}

/// Validate a graph model by converting to config and running validation.
pub fn validate_graph(graph: &GraphModel) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    match graph_to_config(graph) {
        Ok(config) => {
            if let Err(e) = config.validate() {
                errors.push(ValidationError {
                    node_id: None,
                    field: None,
                    message: e.to_string(),
                    severity: Severity::Error,
                });
            }
        }
        Err(e) => {
            errors.push(ValidationError {
                node_id: None,
                field: None,
                message: format!("Failed to build config from graph: {e}"),
                severity: Severity::Error,
            });
        }
    }

    // Check for nodes with missing required fields
    for node in &graph.nodes {
        match node.type_id.as_str() {
            "agent" => {
                if node
                    .data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
                {
                    errors.push(ValidationError {
                        node_id: Some(node.id.clone()),
                        field: Some("name".to_string()),
                        message: "Agent name is required".to_string(),
                        severity: Severity::Error,
                    });
                }
            }
            "model_route" => {
                if node
                    .data
                    .get("hint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
                {
                    errors.push(ValidationError {
                        node_id: Some(node.id.clone()),
                        field: Some("hint".to_string()),
                        message: "Model route hint is required".to_string(),
                        severity: Severity::Error,
                    });
                }
            }
            _ => {}
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrip() {
        let config = AgentZeroConfig::default();
        let graph = config_to_graph(&config);

        // Should have at minimum: provider, security_policy, autonomy
        assert!(graph.nodes.len() >= 3, "expected at least 3 nodes");

        let reconstructed = graph_to_config(&graph).expect("graph_to_config should succeed");

        // Key fields should survive the roundtrip
        assert_eq!(reconstructed.provider.kind, config.provider.kind);
        assert_eq!(reconstructed.provider.model, config.provider.model);
        assert_eq!(reconstructed.autonomy.level, config.autonomy.level);
    }

    #[test]
    fn graph_to_toml_produces_valid_toml() {
        let config = AgentZeroConfig::default();
        let graph = config_to_graph(&config);
        let toml_str = graph_to_toml(&graph).expect("should serialize to TOML");

        // Should be parseable back
        let _parsed: AgentZeroConfig =
            toml::from_str(&toml_str).expect("produced TOML should be parseable");
    }

    #[test]
    fn toml_to_graph_parses_default() {
        let config = AgentZeroConfig::default();
        let toml_str = toml::to_string_pretty(&config).expect("should serialize default config");
        let graph = toml_to_graph(&toml_str).expect("should parse TOML to graph");
        assert!(!graph.nodes.is_empty());
    }

    #[test]
    fn validate_graph_catches_empty_provider() {
        let mut graph = GraphModel::default();
        graph.nodes.push(GraphNode {
            id: "p1".to_string(),
            type_id: "provider".to_string(),
            position: Position::default(),
            data: serde_json::json!({
                "kind": "",
                "base_url": "",
                "model": "",
                "default_temperature": 0.7,
            }),
            parent_id: None,
            width: None,
            height: None,
        });

        let errors = validate_graph(&graph);
        assert!(
            errors.iter().any(|e| e.message.contains("provider")),
            "should report provider validation error"
        );
    }
}
