use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Describes a type of node that can appear in the config graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTypeDescriptor {
    pub type_id: String,
    pub display_name: String,
    pub color: String,
    pub category: String,
    pub ports: Vec<PortDescriptor>,
    pub properties: Vec<PropertyDescriptor>,
}

/// A port on a node where edges can connect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortDescriptor {
    pub id: String,
    pub label: String,
    pub direction: PortDirection,
    pub accepts: Vec<String>,
    pub cardinality: Cardinality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    One,
    Many,
}

/// Describes a configurable property on a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyDescriptor {
    pub key: String,
    pub label: String,
    pub kind: PropertyKind,
    pub default_value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyKind {
    Bool,
    String,
    Number,
    Text,
    Enum,
    StringList,
    KeyValueMap,
}

/// Summary of a tool for display in the graph palette.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSummary {
    pub name: String,
    pub description: String,
    pub category: String,
    pub always_available: bool,
    pub gate_flag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
}

/// Builds the full set of node type descriptors.
pub fn build_node_type_descriptors() -> Vec<NodeTypeDescriptor> {
    vec![
        build_tool_node_descriptor(),
        build_security_policy_descriptor(),
        build_agent_descriptor(),
        build_model_route_descriptor(),
        build_classification_rule_descriptor(),
        build_depth_policy_descriptor(),
        build_provider_descriptor(),
        build_autonomy_descriptor(),
        build_plugin_descriptor(),
    ]
}

fn build_tool_node_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "tool".to_string(),
        display_name: "Tool".to_string(),
        color: "#3b82f6".to_string(),
        category: "tools".to_string(),
        ports: vec![
            PortDescriptor {
                id: "policy_in".to_string(),
                label: "Security Policy".to_string(),
                direction: PortDirection::Input,
                accepts: vec!["security_policy".to_string()],
                cardinality: Cardinality::One,
            },
            PortDescriptor {
                id: "agent_out".to_string(),
                label: "Agent".to_string(),
                direction: PortDirection::Output,
                accepts: vec!["agent".to_string()],
                cardinality: Cardinality::Many,
            },
        ],
        properties: vec![
            PropertyDescriptor {
                key: "name".to_string(),
                label: "Tool Name".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String(String::new()),
                description: Some("Unique tool identifier".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "enabled".to_string(),
                label: "Enabled".to_string(),
                kind: PropertyKind::Bool,
                default_value: Value::Bool(true),
                description: Some("Whether this tool is available".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "description".to_string(),
                label: "Description".to_string(),
                kind: PropertyKind::Text,
                default_value: Value::String(String::new()),
                description: None,
                group: None,
                enum_values: None,
            },
        ],
    }
}

fn build_security_policy_descriptor() -> NodeTypeDescriptor {
    let mut properties = Vec::new();

    // File tools group
    for (key, label, default) in [
        ("enable_write_file", "Enable Write File", false),
        ("enable_git", "Enable Git", false),
    ] {
        properties.push(PropertyDescriptor {
            key: key.to_string(),
            label: label.to_string(),
            kind: PropertyKind::Bool,
            default_value: Value::Bool(default),
            description: None,
            group: Some("File & VCS".to_string()),
            enum_values: None,
        });
    }

    // Web tools group
    for (key, label, default) in [
        ("enable_web_search", "Enable Web Search", false),
        ("enable_browser", "Enable Browser", false),
        ("enable_browser_open", "Enable Browser Open", false),
        ("enable_http_request", "Enable HTTP Request", false),
        ("enable_web_fetch", "Enable Web Fetch", false),
        ("enable_url_validation", "Enable URL Validation", false),
        ("enable_html_extract", "Enable HTML Extract", false),
    ] {
        properties.push(PropertyDescriptor {
            key: key.to_string(),
            label: label.to_string(),
            kind: PropertyKind::Bool,
            default_value: Value::Bool(default),
            description: None,
            group: Some("Web & Network".to_string()),
            enum_values: None,
        });
    }

    // Automation group
    for (key, label, default) in [
        ("enable_cron", "Enable Cron", false),
        ("enable_agents_ipc", "Enable Agents IPC", true),
        ("enable_agent_manage", "Enable Agent Manage", false),
        ("enable_domain_tools", "Enable Domain Tools", false),
        ("enable_mcp", "Enable MCP", false),
        ("enable_pushover", "Enable Pushover", false),
        ("enable_wasm_plugins", "Enable WASM Plugins", false),
    ] {
        properties.push(PropertyDescriptor {
            key: key.to_string(),
            label: label.to_string(),
            kind: PropertyKind::Bool,
            default_value: Value::Bool(default),
            description: None,
            group: Some("Automation & Integrations".to_string()),
            enum_values: None,
        });
    }

    // Sub-policy properties
    properties.push(PropertyDescriptor {
        key: "read_file_max_bytes".to_string(),
        label: "Max Read Bytes".to_string(),
        kind: PropertyKind::Number,
        default_value: serde_json::json!(262144),
        description: Some("Maximum bytes for file reads".to_string()),
        group: Some("Read File Policy".to_string()),
        enum_values: None,
    });
    properties.push(PropertyDescriptor {
        key: "read_file_allow_binary".to_string(),
        label: "Allow Binary".to_string(),
        kind: PropertyKind::Bool,
        default_value: Value::Bool(false),
        description: None,
        group: Some("Read File Policy".to_string()),
        enum_values: None,
    });
    properties.push(PropertyDescriptor {
        key: "write_file_max_bytes".to_string(),
        label: "Max Write Bytes".to_string(),
        kind: PropertyKind::Number,
        default_value: serde_json::json!(65536),
        description: None,
        group: Some("Write File Policy".to_string()),
        enum_values: None,
    });
    properties.push(PropertyDescriptor {
        key: "shell_max_args".to_string(),
        label: "Max Args".to_string(),
        kind: PropertyKind::Number,
        default_value: serde_json::json!(32),
        description: None,
        group: Some("Shell Policy".to_string()),
        enum_values: None,
    });
    properties.push(PropertyDescriptor {
        key: "shell_max_arg_length".to_string(),
        label: "Max Arg Length".to_string(),
        kind: PropertyKind::Number,
        default_value: serde_json::json!(4096),
        description: None,
        group: Some("Shell Policy".to_string()),
        enum_values: None,
    });
    properties.push(PropertyDescriptor {
        key: "shell_max_output_bytes".to_string(),
        label: "Max Output Bytes".to_string(),
        kind: PropertyKind::Number,
        default_value: serde_json::json!(65536),
        description: None,
        group: Some("Shell Policy".to_string()),
        enum_values: None,
    });
    properties.push(PropertyDescriptor {
        key: "allowed_commands".to_string(),
        label: "Allowed Commands".to_string(),
        kind: PropertyKind::StringList,
        default_value: serde_json::json!([
            "ls", "pwd", "cat", "echo", "grep", "find", "git", "cargo"
        ]),
        description: Some("Shell commands the agent may execute".to_string()),
        group: Some("Shell Policy".to_string()),
        enum_values: None,
    });

    NodeTypeDescriptor {
        type_id: "security_policy".to_string(),
        display_name: "Security Policy".to_string(),
        color: "#ef4444".to_string(),
        category: "security".to_string(),
        ports: vec![PortDescriptor {
            id: "tools_out".to_string(),
            label: "Tools".to_string(),
            direction: PortDirection::Output,
            accepts: vec!["tool".to_string()],
            cardinality: Cardinality::Many,
        }],
        properties,
    }
}

fn build_agent_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "agent".to_string(),
        display_name: "Agent".to_string(),
        color: "#8b5cf6".to_string(),
        category: "orchestration".to_string(),
        ports: vec![
            PortDescriptor {
                id: "tools_in".to_string(),
                label: "Tools".to_string(),
                direction: PortDirection::Input,
                accepts: vec!["tool".to_string()],
                cardinality: Cardinality::Many,
            },
            PortDescriptor {
                id: "parent_in".to_string(),
                label: "Parent Agent".to_string(),
                direction: PortDirection::Input,
                accepts: vec!["agent".to_string()],
                cardinality: Cardinality::One,
            },
            PortDescriptor {
                id: "delegate_out".to_string(),
                label: "Delegate To".to_string(),
                direction: PortDirection::Output,
                accepts: vec!["agent".to_string()],
                cardinality: Cardinality::Many,
            },
            PortDescriptor {
                id: "route_in".to_string(),
                label: "Model Route".to_string(),
                direction: PortDirection::Input,
                accepts: vec!["model_route".to_string()],
                cardinality: Cardinality::One,
            },
        ],
        properties: vec![
            PropertyDescriptor {
                key: "name".to_string(),
                label: "Agent Name".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String(String::new()),
                description: Some("Unique agent identifier".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "provider".to_string(),
                label: "Provider URL".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String(String::new()),
                description: Some("Provider base URL".to_string()),
                group: Some("Model".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "model".to_string(),
                label: "Model".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String("anthropic/claude-sonnet-4-6".to_string()),
                description: None,
                group: Some("Model".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "system_prompt".to_string(),
                label: "System Prompt".to_string(),
                kind: PropertyKind::Text,
                default_value: Value::String(String::new()),
                description: None,
                group: Some("Behavior".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "max_depth".to_string(),
                label: "Max Delegation Depth".to_string(),
                kind: PropertyKind::Number,
                default_value: serde_json::json!(3),
                description: None,
                group: Some("Limits".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "agentic".to_string(),
                label: "Agentic (can use tools)".to_string(),
                kind: PropertyKind::Bool,
                default_value: Value::Bool(true),
                description: None,
                group: Some("Behavior".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "max_iterations".to_string(),
                label: "Max Iterations".to_string(),
                kind: PropertyKind::Number,
                default_value: serde_json::json!(20),
                description: None,
                group: Some("Limits".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "privacy_boundary".to_string(),
                label: "Privacy Boundary".to_string(),
                kind: PropertyKind::Enum,
                default_value: Value::String("inherit".to_string()),
                description: None,
                group: Some("Privacy".to_string()),
                enum_values: Some(vec![
                    "inherit".to_string(),
                    "local_only".to_string(),
                    "encrypted_only".to_string(),
                    "any".to_string(),
                ]),
            },
            PropertyDescriptor {
                key: "max_tokens".to_string(),
                label: "Max Tokens".to_string(),
                kind: PropertyKind::Number,
                default_value: Value::Null,
                description: Some("Per-run token limit (optional)".to_string()),
                group: Some("Limits".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "max_cost_usd".to_string(),
                label: "Max Cost (USD)".to_string(),
                kind: PropertyKind::Number,
                default_value: Value::Null,
                description: Some("Per-run cost limit in USD (optional)".to_string()),
                group: Some("Limits".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "allowed_tools".to_string(),
                label: "Allowed Tools".to_string(),
                kind: PropertyKind::StringList,
                default_value: serde_json::json!([]),
                description: Some("Tool names this agent may use (empty = all)".to_string()),
                group: Some("Tools".to_string()),
                enum_values: None,
            },
        ],
    }
}

fn build_model_route_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "model_route".to_string(),
        display_name: "Model Route".to_string(),
        color: "#f59e0b".to_string(),
        category: "routing".to_string(),
        ports: vec![
            PortDescriptor {
                id: "rule_in".to_string(),
                label: "Classification Rule".to_string(),
                direction: PortDirection::Input,
                accepts: vec!["classification_rule".to_string()],
                cardinality: Cardinality::Many,
            },
            PortDescriptor {
                id: "agent_out".to_string(),
                label: "Agent".to_string(),
                direction: PortDirection::Output,
                accepts: vec!["agent".to_string()],
                cardinality: Cardinality::Many,
            },
        ],
        properties: vec![
            PropertyDescriptor {
                key: "hint".to_string(),
                label: "Hint".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String(String::new()),
                description: Some("Route hint (e.g. reasoning, fast, code)".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "provider".to_string(),
                label: "Provider".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String(String::new()),
                description: None,
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "model".to_string(),
                label: "Model".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String(String::new()),
                description: None,
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "max_tokens".to_string(),
                label: "Max Tokens".to_string(),
                kind: PropertyKind::Number,
                default_value: Value::Null,
                description: None,
                group: None,
                enum_values: None,
            },
        ],
    }
}

fn build_classification_rule_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "classification_rule".to_string(),
        display_name: "Classification Rule".to_string(),
        color: "#06b6d4".to_string(),
        category: "routing".to_string(),
        ports: vec![PortDescriptor {
            id: "route_out".to_string(),
            label: "Model Route".to_string(),
            direction: PortDirection::Output,
            accepts: vec!["model_route".to_string()],
            cardinality: Cardinality::One,
        }],
        properties: vec![
            PropertyDescriptor {
                key: "hint".to_string(),
                label: "Hint".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String(String::new()),
                description: Some("Matches the model route hint".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "keywords".to_string(),
                label: "Keywords".to_string(),
                kind: PropertyKind::StringList,
                default_value: serde_json::json!([]),
                description: Some("Keywords that trigger this rule".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "patterns".to_string(),
                label: "Regex Patterns".to_string(),
                kind: PropertyKind::StringList,
                default_value: serde_json::json!([]),
                description: Some("Regex patterns that trigger this rule".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "priority".to_string(),
                label: "Priority".to_string(),
                kind: PropertyKind::Number,
                default_value: serde_json::json!(0),
                description: Some("Higher priority rules are checked first".to_string()),
                group: None,
                enum_values: None,
            },
        ],
    }
}

fn build_depth_policy_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "depth_policy".to_string(),
        display_name: "Depth Policy".to_string(),
        color: "#10b981".to_string(),
        category: "security".to_string(),
        ports: vec![PortDescriptor {
            id: "tools_out".to_string(),
            label: "Restricted Tools".to_string(),
            direction: PortDirection::Output,
            accepts: vec!["tool".to_string()],
            cardinality: Cardinality::Many,
        }],
        properties: vec![
            PropertyDescriptor {
                key: "max_depth".to_string(),
                label: "Max Depth".to_string(),
                kind: PropertyKind::Number,
                default_value: serde_json::json!(5),
                description: Some("Depth threshold for this rule".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "allowed_tools".to_string(),
                label: "Allowed Tools".to_string(),
                kind: PropertyKind::StringList,
                default_value: serde_json::json!([]),
                description: Some("Whitelist (empty = allow all except denied)".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "denied_tools".to_string(),
                label: "Denied Tools".to_string(),
                kind: PropertyKind::StringList,
                default_value: serde_json::json!([]),
                description: Some("Blacklist at this depth".to_string()),
                group: None,
                enum_values: None,
            },
        ],
    }
}

fn build_provider_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "provider".to_string(),
        display_name: "Provider".to_string(),
        color: "#ec4899".to_string(),
        category: "infrastructure".to_string(),
        ports: vec![PortDescriptor {
            id: "agent_out".to_string(),
            label: "Agent".to_string(),
            direction: PortDirection::Output,
            accepts: vec!["agent".to_string()],
            cardinality: Cardinality::Many,
        }],
        properties: vec![
            PropertyDescriptor {
                key: "kind".to_string(),
                label: "Provider Kind".to_string(),
                kind: PropertyKind::Enum,
                default_value: Value::String("openrouter".to_string()),
                description: None,
                group: None,
                enum_values: Some(vec![
                    "openrouter".to_string(),
                    "anthropic".to_string(),
                    "openai".to_string(),
                    "ollama".to_string(),
                    "llamacpp".to_string(),
                    "lmstudio".to_string(),
                    "vllm".to_string(),
                    "sglang".to_string(),
                    "builtin".to_string(),
                ]),
            },
            PropertyDescriptor {
                key: "base_url".to_string(),
                label: "Base URL".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String("https://openrouter.ai/api".to_string()),
                description: None,
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "model".to_string(),
                label: "Model".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String("anthropic/claude-sonnet-4-6".to_string()),
                description: None,
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "default_temperature".to_string(),
                label: "Temperature".to_string(),
                kind: PropertyKind::Number,
                default_value: serde_json::json!(0.7),
                description: Some("0.0 to 2.0".to_string()),
                group: None,
                enum_values: None,
            },
        ],
    }
}

fn build_autonomy_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "autonomy".to_string(),
        display_name: "Autonomy".to_string(),
        color: "#f97316".to_string(),
        category: "security".to_string(),
        ports: vec![],
        properties: vec![
            PropertyDescriptor {
                key: "level".to_string(),
                label: "Autonomy Level".to_string(),
                kind: PropertyKind::Enum,
                default_value: Value::String("supervised".to_string()),
                description: None,
                group: None,
                enum_values: Some(vec![
                    "supervised".to_string(),
                    "semi".to_string(),
                    "autonomous".to_string(),
                    "locked".to_string(),
                ]),
            },
            PropertyDescriptor {
                key: "max_actions_per_hour".to_string(),
                label: "Max Actions/Hour".to_string(),
                kind: PropertyKind::Number,
                default_value: serde_json::json!(200),
                description: None,
                group: Some("Limits".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "max_cost_per_day_cents".to_string(),
                label: "Max Cost/Day (cents)".to_string(),
                kind: PropertyKind::Number,
                default_value: serde_json::json!(2000),
                description: None,
                group: Some("Limits".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "require_approval_for_medium_risk".to_string(),
                label: "Require Approval for Medium Risk".to_string(),
                kind: PropertyKind::Bool,
                default_value: Value::Bool(true),
                description: None,
                group: Some("Risk".to_string()),
                enum_values: None,
            },
            PropertyDescriptor {
                key: "block_high_risk_commands".to_string(),
                label: "Block High Risk Commands".to_string(),
                kind: PropertyKind::Bool,
                default_value: Value::Bool(true),
                description: None,
                group: Some("Risk".to_string()),
                enum_values: None,
            },
        ],
    }
}

fn build_plugin_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "plugin".to_string(),
        display_name: "Plugin".to_string(),
        color: "#a855f7".to_string(),
        category: "plugin".to_string(),
        ports: vec![PortDescriptor {
            id: "agent_out".to_string(),
            label: "Agent".to_string(),
            direction: PortDirection::Output,
            accepts: vec!["agent".to_string()],
            cardinality: Cardinality::Many,
        }],
        properties: vec![
            PropertyDescriptor {
                key: "name".to_string(),
                label: "Plugin Name".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String("".to_string()),
                description: Some("Name of the WASM plugin".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "enabled".to_string(),
                label: "Enabled".to_string(),
                kind: PropertyKind::Bool,
                default_value: Value::Bool(true),
                description: None,
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "source".to_string(),
                label: "Source Path".to_string(),
                kind: PropertyKind::String,
                default_value: Value::String("".to_string()),
                description: Some("Path or URL to the WASM module".to_string()),
                group: None,
                enum_values: None,
            },
            PropertyDescriptor {
                key: "description".to_string(),
                label: "Description".to_string(),
                kind: PropertyKind::Text,
                default_value: Value::String("".to_string()),
                description: None,
                group: None,
                enum_values: None,
            },
        ],
    }
}

/// Builds the canonical list of available tools with metadata.
pub fn build_tool_summaries() -> Vec<ToolSummary> {
    vec![
        // Always available
        tool("read_file", "Read file contents", "file", true, None),
        tool("shell", "Execute shell commands", "system", true, None),
        tool(
            "glob_search",
            "Find files by glob pattern",
            "file",
            true,
            None,
        ),
        tool(
            "content_search",
            "Search file contents with regex",
            "file",
            true,
            None,
        ),
        tool("memory_store", "Store a memory", "memory", true, None),
        tool("memory_recall", "Recall memories", "memory", true, None),
        tool("memory_forget", "Forget a memory", "memory", true, None),
        tool("image_info", "Get image metadata", "file", true, None),
        tool("pdf_read", "Read PDF documents", "file", true, None),
        tool("screenshot", "Take a screenshot", "system", true, None),
        tool(
            "task_plan",
            "Create and manage task plans",
            "orchestration",
            true,
            None,
        ),
        tool("process", "Manage system processes", "system", true, None),
        tool(
            "subagent_spawn",
            "Spawn a sub-agent",
            "orchestration",
            true,
            None,
        ),
        tool(
            "subagent_list",
            "List running sub-agents",
            "orchestration",
            true,
            None,
        ),
        tool(
            "subagent_manage",
            "Manage sub-agents",
            "orchestration",
            true,
            None,
        ),
        tool(
            "cli_discovery",
            "Discover CLI commands",
            "system",
            true,
            None,
        ),
        tool(
            "proxy_config",
            "Configure proxy settings",
            "system",
            true,
            None,
        ),
        tool(
            "delegate_coordination_status",
            "Check delegation status",
            "orchestration",
            true,
            None,
        ),
        tool(
            "sop_list",
            "List standard operating procedures",
            "sop",
            true,
            None,
        ),
        tool("sop_status", "Check SOP status", "sop", true, None),
        tool("sop_advance", "Advance SOP to next step", "sop", true, None),
        tool("sop_approve", "Approve SOP step", "sop", true, None),
        tool("sop_execute", "Execute SOP", "sop", true, None),
        tool(
            "hardware_board_info",
            "Get hardware board info",
            "hardware",
            true,
            None,
        ),
        tool(
            "hardware_memory_map",
            "Get hardware memory map",
            "hardware",
            true,
            None,
        ),
        tool(
            "hardware_memory_read",
            "Read hardware memory",
            "hardware",
            true,
            None,
        ),
        tool("wasm_module", "Manage WASM modules", "plugin", true, None),
        tool("wasm_tool_exec", "Execute WASM tool", "plugin", true, None),
        // Gated by security policy flags
        tool(
            "write_file",
            "Write file contents",
            "file",
            false,
            Some("enable_write_file"),
        ),
        tool(
            "apply_patch",
            "Apply a patch to files",
            "file",
            false,
            Some("enable_write_file"),
        ),
        tool(
            "file_edit",
            "Edit file contents",
            "file",
            false,
            Some("enable_write_file"),
        ),
        tool(
            "git_operations",
            "Git version control operations",
            "vcs",
            false,
            Some("enable_git"),
        ),
        tool(
            "cron_add",
            "Add a cron job",
            "automation",
            false,
            Some("enable_cron"),
        ),
        tool(
            "cron_list",
            "List cron jobs",
            "automation",
            false,
            Some("enable_cron"),
        ),
        tool(
            "cron_remove",
            "Remove a cron job",
            "automation",
            false,
            Some("enable_cron"),
        ),
        tool(
            "cron_update",
            "Update a cron job",
            "automation",
            false,
            Some("enable_cron"),
        ),
        tool(
            "cron_pause",
            "Pause a cron job",
            "automation",
            false,
            Some("enable_cron"),
        ),
        tool(
            "cron_resume",
            "Resume a cron job",
            "automation",
            false,
            Some("enable_cron"),
        ),
        tool(
            "schedule",
            "Schedule a one-shot task",
            "automation",
            false,
            Some("enable_cron"),
        ),
        tool(
            "web_search",
            "Search the web",
            "web",
            false,
            Some("enable_web_search"),
        ),
        tool(
            "browser",
            "Interactive browser automation",
            "web",
            false,
            Some("enable_browser"),
        ),
        tool(
            "browser_open",
            "Open a URL in the browser",
            "web",
            false,
            Some("enable_browser_open"),
        ),
        tool(
            "http_request",
            "Make HTTP requests",
            "web",
            false,
            Some("enable_http_request"),
        ),
        tool(
            "web_fetch",
            "Fetch web page content",
            "web",
            false,
            Some("enable_web_fetch"),
        ),
        tool(
            "url_validation",
            "Validate URLs",
            "web",
            false,
            Some("enable_url_validation"),
        ),
        tool(
            "agents_ipc",
            "Inter-agent communication",
            "orchestration",
            false,
            Some("enable_agents_ipc"),
        ),
        tool(
            "agent_manage",
            "Create, list, update, or delete persistent agents",
            "orchestration",
            false,
            Some("enable_agent_manage"),
        ),
        tool(
            "domain_search",
            "Domain-driven research: create domains, search sources, verify findings, run workflows",
            "research",
            false,
            Some("enable_domain_tools"),
        ),
        tool(
            "pushover",
            "Pushover notifications",
            "integration",
            false,
            Some("enable_pushover"),
        ),
    ]
}

fn tool(name: &str, desc: &str, category: &str, always: bool, gate: Option<&str>) -> ToolSummary {
    ToolSummary {
        name: name.to_string(),
        description: desc.to_string(),
        category: category.to_string(),
        always_available: always,
        gate_flag: gate.map(|s| s.to_string()),
        input_schema: None,
    }
}
