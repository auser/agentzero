//! Tool registry and built-in tools for AgentZero.
//!
//! Tools are explicit capabilities granted by policy (ADR 0003).
//! Every tool call is auditable and policy-checked.

use agentzero_core::{Capability, DataClassification, RuntimeTier, ToolId, ToolSchema};
use agentzero_policy::{PolicyEngine, PolicyRequest};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("tool execution denied: {0}")]
    Denied(String),
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: Vec<ToolSchema>,
    policy: PolicyEngine,
}

impl ToolRegistry {
    /// Create a new tool registry with deny-by-default policy.
    pub fn new(policy: PolicyEngine) -> Self {
        Self {
            tools: Vec::new(),
            policy,
        }
    }

    /// Register a tool schema.
    pub fn register(&mut self, schema: ToolSchema) {
        self.tools.push(schema);
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&ToolSchema> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// List all registered tools.
    pub fn list(&self) -> &[ToolSchema] {
        &self.tools
    }

    /// Check whether a tool invocation would be allowed by policy.
    pub fn check_permission(&self, tool_id: &ToolId, capability: Capability) -> bool {
        let request = PolicyRequest {
            capability,
            classification: DataClassification::Private,
            runtime: RuntimeTier::HostReadonly,
            context: format!("tool:{tool_id}"),
        };
        self.policy.evaluate(&request).is_allowed()
    }
}

/// Built-in tool definitions for the minimal tool set.
pub fn builtin_tool_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            id: ToolId::from_string("read"),
            name: "read".into(),
            description: "Read file contents".into(),
            capabilities: vec![Capability::FileRead],
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read" }
                },
                "required": ["path"]
            }),
        },
        ToolSchema {
            id: ToolId::from_string("list"),
            name: "list".into(),
            description: "List directory contents".into(),
            capabilities: vec![Capability::FileRead],
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to list" }
                },
                "required": ["path"]
            }),
        },
        ToolSchema {
            id: ToolId::from_string("search"),
            name: "search".into(),
            description: "Search file contents with a pattern".into(),
            capabilities: vec![Capability::FileRead],
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Search pattern" },
                    "path": { "type": "string", "description": "Directory to search" }
                },
                "required": ["pattern"]
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_registers_and_finds_tools() {
        let policy = PolicyEngine::deny_by_default();
        let mut registry = ToolRegistry::new(policy);
        let schemas = builtin_tool_schemas();
        for schema in schemas {
            registry.register(schema);
        }
        assert_eq!(registry.list().len(), 3);
        assert!(registry.get("read").is_some());
        assert!(registry.get("list").is_some());
        assert!(registry.get("search").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn deny_by_default_blocks_tool_permission() {
        let policy = PolicyEngine::deny_by_default();
        let registry = ToolRegistry::new(policy);
        let tool_id = ToolId::from_string("read");
        assert!(!registry.check_permission(&tool_id, Capability::FileRead));
    }

    #[test]
    fn builtin_schemas_have_correct_structure() {
        let schemas = builtin_tool_schemas();
        for schema in &schemas {
            assert!(!schema.name.is_empty());
            assert!(!schema.description.is_empty());
            assert!(!schema.capabilities.is_empty());
        }
    }
}
