//! ACP stdio server.
//!
//! Reads newline-delimited JSON from stdin, dispatches to handlers,
//! writes responses to stdout. This is the bridge between editor
//! integrations and the AgentZero session engine.

use agentzero_tracing::{info, warn};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::protocol::{AcpMethod, AcpRequest, AcpResponse};

/// ACP server that communicates over stdio.
pub struct AcpServer {
    name: String,
    version: String,
}

impl AcpServer {
    pub fn new() -> Self {
        Self {
            name: "agentzero".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
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
            AcpMethod::Chat => {
                // Stub: will wire to session engine
                AcpResponse::ok(
                    &request.id,
                    serde_json::json!({
                        "message": "ACP chat not yet wired to session engine"
                    }),
                )
            }
            AcpMethod::ToolCall => {
                // Stub: will wire to session engine
                AcpResponse::ok(
                    &request.id,
                    serde_json::json!({
                        "message": "ACP tool_call not yet wired to session engine"
                    }),
                )
            }
            AcpMethod::Shutdown => {
                AcpResponse::ok(&request.id, serde_json::json!({"status": "shutdown"}))
            }
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

    #[test]
    fn handle_initialize() {
        let server = AcpServer::new();
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
        let server = AcpServer::new();
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
    fn handle_shutdown() {
        let server = AcpServer::new();
        let req = AcpRequest {
            id: "3".into(),
            method: AcpMethod::Shutdown,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req);
        assert!(resp.success);
    }
}
