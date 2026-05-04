//! MCP server implementation.
//!
//! Reads JSON-RPC 2.0 messages from stdin, dispatches tool calls through
//! the AgentZero session engine (with policy enforcement), and writes
//! responses to stdout.

use agentzero_core::Capability;
use agentzero_policy::PolicyEngine;
use agentzero_session::{Session, SessionConfig, SessionMode, ToolExecutor};
use agentzero_tracing::{info, warn};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::protocol::{JsonRpcRequest, JsonRpcResponse, McpError, McpToolDef, McpToolResult};

/// MCP server configuration.
pub struct McpServerConfig {
    pub project_root: Option<String>,
    pub policy: PolicyEngine,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            project_root: None,
            policy: PolicyEngine::deny_by_default(),
        }
    }
}

/// MCP server that exposes AgentZero tools to MCP clients.
pub struct McpServer {
    session: Session,
}

impl McpServer {
    /// Create a new MCP server with the given config.
    pub fn new(config: McpServerConfig) -> Result<Self, String> {
        let tool_policy = PolicyEngine::with_rules(vec![
            agentzero_policy::PolicyRule::allow(
                Capability::FileRead,
                agentzero_core::DataClassification::Private,
            ),
            agentzero_policy::PolicyRule::allow(
                Capability::FileRead,
                agentzero_core::DataClassification::Public,
            ),
            agentzero_policy::PolicyRule::allow(
                Capability::FileRead,
                agentzero_core::DataClassification::Internal,
            ),
            agentzero_policy::PolicyRule::require_approval(
                Capability::FileWrite,
                "file writes require approval",
            ),
            agentzero_policy::PolicyRule::require_approval(
                Capability::ShellCommand,
                "shell commands require approval",
            ),
        ]);

        let mut tool_executor = ToolExecutor::new(tool_policy);
        if let Some(ref root) = config.project_root {
            tool_executor = tool_executor.with_project_root(root.clone());
        }

        let session_config = SessionConfig {
            mode: SessionMode::LocalOnly,
            project_root: config.project_root,
        };

        let session = Session::new(session_config, config.policy)
            .map_err(|e| format!("failed to create session: {e}"))?
            .with_tool_executor(tool_executor);

        Ok(Self { session })
    }

    /// Run the MCP server on stdio.
    pub async fn run(&self) -> Result<(), String> {
        info!("MCP server starting on stdio");

        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    info!("MCP server: stdin closed");
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(error = %e, "MCP server: read error");
                    break;
                }
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
                Ok(r) => r,
                Err(e) => {
                    let resp = JsonRpcResponse::error(
                        None,
                        &McpError::ParseError(format!("invalid JSON-RPC: {e}")),
                    );
                    let json = serde_json::to_string(&resp).expect("response should serialize");
                    stdout.write_all(format!("{json}\n").as_bytes()).await.ok();
                    continue;
                }
            };

            let response = self.handle(&request);
            let json = serde_json::to_string(&response).expect("response should serialize");
            stdout.write_all(format!("{json}\n").as_bytes()).await.ok();
            stdout.flush().await.ok();

            if request.method == "shutdown" {
                info!("MCP server: shutdown");
                break;
            }
        }

        self.session.end().ok();
        Ok(())
    }

    fn handle(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request),
            "tools/list" => self.handle_tools_list(request),
            "tools/call" => self.handle_tools_call(request),
            "shutdown" => JsonRpcResponse::success(
                request.id.clone(),
                serde_json::json!({"status": "shutdown"}),
            ),
            other => JsonRpcResponse::error(
                request.id.clone(),
                &McpError::MethodNotFound(other.to_string()),
            ),
        }
    }

    fn handle_initialize(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        info!("MCP initialize");
        JsonRpcResponse::success(
            request.id.clone(),
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "agentzero",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )
    }

    fn handle_tools_list(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let tools = vec![
            McpToolDef {
                name: "read_file".into(),
                description: "Read the contents of a file at the given path".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute or relative path to the file"
                        }
                    },
                    "required": ["path"]
                }),
            },
            McpToolDef {
                name: "list_directory".into(),
                description: "List files and directories at the given path".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Directory path to list (defaults to cwd)"
                        }
                    }
                }),
            },
            McpToolDef {
                name: "search_files".into(),
                description: "Search for a text pattern in files within a directory".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Text pattern to search for"
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory to search in (defaults to cwd)"
                        }
                    },
                    "required": ["pattern"]
                }),
            },
            McpToolDef {
                name: "write_file".into(),
                description: "Write content to a file (requires approval in policy)".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to write to"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write"
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
            McpToolDef {
                name: "run_command".into(),
                description: "Execute a shell command (requires approval in policy)".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Shell command to execute"
                        }
                    },
                    "required": ["command"]
                }),
            },
        ];

        JsonRpcResponse::success(request.id.clone(), serde_json::json!({ "tools": tools }))
    }

    fn handle_tools_call(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let tool_name = request
            .params
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let arguments = request
            .params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        info!(tool = tool_name, "MCP tools/call");

        // Map MCP tool names to internal tool names
        let (internal_name, args) = match tool_name {
            "read_file" => ("read", arguments),
            "list_directory" => ("list", arguments),
            "search_files" => ("search", arguments),
            "write_file" => ("write", arguments),
            "run_command" => {
                // Remap "command" arg to match internal schema
                let cmd = arguments
                    .get("command")
                    .cloned()
                    .unwrap_or(serde_json::json!(""));
                ("shell", serde_json::json!({"command": cmd}))
            }
            other => {
                let result = McpToolResult::error(&format!("unknown tool: {other}"));
                return JsonRpcResponse::success(
                    request.id.clone(),
                    serde_json::to_value(result).expect("should serialize"),
                );
            }
        };

        match self.session.execute_tool(internal_name, &args) {
            Ok(output) => {
                let result = McpToolResult::text(&output);
                JsonRpcResponse::success(
                    request.id.clone(),
                    serde_json::to_value(result).expect("should serialize"),
                )
            }
            Err(e) => {
                let result = McpToolResult::error(&e.to_string());
                JsonRpcResponse::success(
                    request.id.clone(),
                    serde_json::to_value(result).expect("should serialize"),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_server() -> McpServer {
        let config = McpServerConfig {
            project_root: Some(".".into()),
            ..McpServerConfig::default()
        };
        McpServer::new(config).expect("should create")
    }

    fn request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn handle_initialize() {
        let server = test_server();
        let resp = server.handle(&request("initialize", serde_json::json!({})));
        assert!(resp.result.is_some());
        let result = resp.result.expect("should have result");
        assert_eq!(result["serverInfo"]["name"], "agentzero");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[test]
    fn handle_tools_list() {
        let server = test_server();
        let resp = server.handle(&request("tools/list", serde_json::json!({})));
        assert!(resp.result.is_some());
        let result = resp.result.expect("should have result");
        let tools = result["tools"].as_array().expect("should be array");
        assert_eq!(tools.len(), 5);
        assert_eq!(tools[0]["name"], "read_file");
        assert_eq!(tools[1]["name"], "list_directory");
        assert_eq!(tools[2]["name"], "search_files");
        assert_eq!(tools[3]["name"], "write_file");
        assert_eq!(tools[4]["name"], "run_command");
    }

    #[test]
    fn handle_tools_call_read() {
        let server = test_server();
        let resp = server.handle(&request(
            "tools/call",
            serde_json::json!({
                "name": "read_file",
                "arguments": {"path": "Cargo.toml"}
            }),
        ));
        assert!(resp.result.is_some());
        let result = resp.result.expect("should have result");
        let text = result["content"][0]["text"]
            .as_str()
            .expect("should be string");
        assert!(text.contains("[package]"));
    }

    #[test]
    fn handle_tools_call_list() {
        let server = test_server();
        let resp = server.handle(&request(
            "tools/call",
            serde_json::json!({
                "name": "list_directory",
                "arguments": {"path": "."}
            }),
        ));
        assert!(resp.result.is_some());
        let result = resp.result.expect("should have result");
        let text = result["content"][0]["text"]
            .as_str()
            .expect("should be string");
        assert!(text.contains("Cargo.toml"));
    }

    #[test]
    fn handle_tools_call_search() {
        let server = test_server();
        let resp = server.handle(&request(
            "tools/call",
            serde_json::json!({
                "name": "search_files",
                "arguments": {"pattern": "agentzero", "path": "src"}
            }),
        ));
        assert!(resp.result.is_some());
        let result = resp.result.expect("should have result");
        let text = result["content"][0]["text"]
            .as_str()
            .expect("should be string");
        assert!(text.contains("agentzero"));
    }

    #[test]
    fn handle_tools_call_unknown() {
        let server = test_server();
        let resp = server.handle(&request(
            "tools/call",
            serde_json::json!({
                "name": "nonexistent_tool",
                "arguments": {}
            }),
        ));
        let result = resp.result.expect("should have result");
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn handle_unknown_method() {
        let server = test_server();
        let resp = server.handle(&request("unknown/method", serde_json::json!({})));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.expect("should have error").code, -32601);
    }

    #[test]
    fn handle_shutdown() {
        let server = test_server();
        let resp = server.handle(&request("shutdown", serde_json::json!({})));
        assert!(resp.result.is_some());
    }
}
