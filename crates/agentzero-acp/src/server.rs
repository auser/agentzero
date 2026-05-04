//! ACP stdio server backed by a real AgentZero session.

use agentzero_core::Capability;
use agentzero_policy::PolicyEngine;
use agentzero_session::{Session, SessionConfig, SessionMode, ToolExecutor};
use agentzero_tracing::{info, warn};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::protocol::{AcpMethod, AcpRequest, AcpResponse};

/// ACP server configuration.
pub struct AcpServerConfig {
    pub project_root: Option<String>,
    pub policy: PolicyEngine,
}

impl Default for AcpServerConfig {
    fn default() -> Self {
        Self {
            project_root: None,
            policy: PolicyEngine::deny_by_default(),
        }
    }
}

/// ACP server that communicates over stdio, backed by a session engine.
pub struct AcpServer {
    name: String,
    version: String,
    session: Session,
}

impl AcpServer {
    /// Create a new ACP server with default deny-all policy.
    pub fn new() -> Self {
        Self::with_config(AcpServerConfig::default()).expect("default config should work")
    }

    /// Create a server with custom configuration.
    pub fn with_config(config: AcpServerConfig) -> Result<Self, String> {
        let tool_policy = PolicyEngine::with_rules(vec![
            agentzero_policy::PolicyRule::allow(
                Capability::FileRead,
                agentzero_core::DataClassification::Private,
            ),
            agentzero_policy::PolicyRule::allow(
                Capability::FileRead,
                agentzero_core::DataClassification::Public,
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

        Ok(Self {
            name: "agentzero".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            session,
        })
    }

    /// Run the ACP server, reading from stdin and writing to stdout.
    pub async fn run(&self) -> Result<(), String> {
        info!("ACP server starting on stdio");

        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    info!("ACP server: stdin closed");
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(error = %e, "ACP server: read error");
                    break;
                }
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let request: AcpRequest = match serde_json::from_str(trimmed) {
                Ok(r) => r,
                Err(e) => {
                    let resp = AcpResponse::err("unknown", &format!("invalid request: {e}"));
                    let json = serde_json::to_string(&resp).expect("response should serialize");
                    stdout.write_all(format!("{json}\n").as_bytes()).await.ok();
                    continue;
                }
            };

            let response = self.handle(&request);
            let json = serde_json::to_string(&response).expect("response should serialize");
            stdout.write_all(format!("{json}\n").as_bytes()).await.ok();
            stdout.flush().await.ok();

            if matches!(request.method, AcpMethod::Shutdown) {
                info!("ACP server: shutdown requested");
                break;
            }
        }

        self.session.end().ok();
        Ok(())
    }

    fn handle(&self, request: &AcpRequest) -> AcpResponse {
        match request.method {
            AcpMethod::Initialize => AcpResponse::ok(
                &request.id,
                serde_json::json!({
                    "name": self.name,
                    "version": self.version,
                    "capabilities": ["chat", "tool_call", "session_info", "list_tools"]
                }),
            ),
            AcpMethod::SessionInfo => AcpResponse::ok(
                &request.id,
                serde_json::json!({
                    "session_id": self.session.id().as_str(),
                    "status": "ready",
                    "tools": ["read", "list", "search", "write", "shell"]
                }),
            ),
            AcpMethod::ListTools => AcpResponse::ok(
                &request.id,
                serde_json::json!({
                    "tools": [
                        {"name": "read", "description": "Read file contents"},
                        {"name": "list", "description": "List directory contents"},
                        {"name": "search", "description": "Search file contents"},
                        {"name": "write", "description": "Write file (requires approval)"},
                        {"name": "shell", "description": "Shell command (requires approval)"}
                    ]
                }),
            ),
            AcpMethod::ToolCall => self.handle_tool_call(request),
            AcpMethod::Chat => {
                // Chat via ACP — for now return a message explaining usage
                let message = request
                    .params
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                AcpResponse::ok(
                    &request.id,
                    serde_json::json!({
                        "message": format!("Received: \"{message}\". Chat requires a model provider — use `agentzero chat` for full chat, or use tool_call for direct tool access.")
                    }),
                )
            }
            AcpMethod::Shutdown => {
                AcpResponse::ok(&request.id, serde_json::json!({"status": "shutdown"}))
            }
        }
    }

    fn handle_tool_call(&self, request: &AcpRequest) -> AcpResponse {
        let tool_name = match request.params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => {
                return AcpResponse::err(&request.id, "missing 'name' in params");
            }
        };

        let arguments = request
            .params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        info!(tool = tool_name, "ACP tool_call");

        match self.session.execute_tool(tool_name, &arguments) {
            Ok(output) => AcpResponse::ok(
                &request.id,
                serde_json::json!({
                    "success": true,
                    "output": output
                }),
            ),
            Err(e) => AcpResponse::ok(
                &request.id,
                serde_json::json!({
                    "success": false,
                    "error": e.to_string()
                }),
            ),
        }
    }
}

impl Default for AcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_server() -> AcpServer {
        AcpServer::with_config(AcpServerConfig {
            project_root: Some(".".into()),
            ..AcpServerConfig::default()
        })
        .expect("should create")
    }

    #[test]
    fn handle_initialize() {
        let server = test_server();
        let req = AcpRequest {
            id: "1".into(),
            method: AcpMethod::Initialize,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req);
        assert!(resp.success);
        assert!(resp.result.as_ref().expect("should have result")["name"]
            .as_str()
            .expect("should be string")
            .contains("agentzero"));
    }

    #[test]
    fn handle_list_tools() {
        let server = test_server();
        let req = AcpRequest {
            id: "2".into(),
            method: AcpMethod::ListTools,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req);
        assert!(resp.success);
        let tools = &resp.result.expect("should have result")["tools"];
        assert!(tools.as_array().expect("should be array").len() >= 5);
    }

    #[test]
    fn handle_tool_call_read() {
        let server = test_server();
        let req = AcpRequest {
            id: "3".into(),
            method: AcpMethod::ToolCall,
            params: serde_json::json!({
                "name": "read",
                "arguments": {"path": "Cargo.toml"}
            }),
        };
        let resp = server.handle(&req);
        assert!(resp.success);
        let result = resp.result.expect("should have result");
        assert_eq!(result["success"], true);
        assert!(result["output"]
            .as_str()
            .expect("should be string")
            .contains("[package]"));
    }

    #[test]
    fn handle_tool_call_list() {
        let server = test_server();
        let req = AcpRequest {
            id: "4".into(),
            method: AcpMethod::ToolCall,
            params: serde_json::json!({
                "name": "list",
                "arguments": {"path": "."}
            }),
        };
        let resp = server.handle(&req);
        assert!(resp.success);
        let result = resp.result.expect("should have result");
        assert!(result["output"]
            .as_str()
            .expect("should be string")
            .contains("Cargo.toml"));
    }

    #[test]
    fn handle_tool_call_missing_name() {
        let server = test_server();
        let req = AcpRequest {
            id: "5".into(),
            method: AcpMethod::ToolCall,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req);
        assert!(!resp.success);
    }

    #[test]
    fn handle_shutdown() {
        let server = test_server();
        let req = AcpRequest {
            id: "6".into(),
            method: AcpMethod::Shutdown,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req);
        assert!(resp.success);
    }

    #[test]
    fn handle_session_info() {
        let server = test_server();
        let req = AcpRequest {
            id: "7".into(),
            method: AcpMethod::SessionInfo,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req);
        assert!(resp.success);
        let result = resp.result.expect("should have result");
        assert!(result["session_id"].as_str().is_some());
        assert_eq!(result["status"], "ready");
    }
}
