//! ACP stdio server backed by a real AgentZero session with full agentic loop.

use std::sync::Arc;

use agentzero_core::Capability;
use agentzero_policy::PolicyEngine;
use agentzero_session::agent_loop::{AgentLoop, AgentLoopConfig, AutoApprove, ProgressHandler};
use agentzero_session::ollama::OllamaProvider;
use agentzero_session::router::ProviderRouter;
use agentzero_session::{Session, SessionConfig, SessionMode, ToolExecutor};
use agentzero_tracing::{info, warn};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::protocol::{AcpMethod, AcpNotification, AcpRequest, AcpResponse};

/// ACP server configuration.
pub struct AcpServerConfig {
    pub project_root: Option<String>,
    pub policy: PolicyEngine,
    /// Model name for the provider (default: "llama3.2").
    pub model: String,
    /// Ollama base URL (default: "http://localhost:11434").
    pub ollama_url: String,
}

impl Default for AcpServerConfig {
    fn default() -> Self {
        Self {
            project_root: None,
            policy: PolicyEngine::deny_by_default(),
            model: "llama3.2".into(),
            ollama_url: "http://localhost:11434".into(),
        }
    }
}

/// Progress handler that sends notifications over stdout.
struct AcpProgressHandler {
    stdout: Arc<Mutex<tokio::io::Stdout>>,
}

impl ProgressHandler for AcpProgressHandler {
    fn on_tool_start(&self, tool_name: &str, args: &serde_json::Value) {
        let notification = AcpNotification::tool_start(tool_name, args);
        if let Ok(json) = serde_json::to_string(&notification) {
            // Best-effort notification — don't block the agent loop
            let stdout = self.stdout.clone();
            let line = format!("{json}\n");
            tokio::spawn(async move {
                let mut out = stdout.lock().await;
                out.write_all(line.as_bytes()).await.ok();
                out.flush().await.ok();
            });
        }
    }

    fn on_tool_result(&self, tool_name: &str, success: bool, output_len: usize) {
        let notification = AcpNotification::tool_result(tool_name, success, output_len);
        if let Ok(json) = serde_json::to_string(&notification) {
            let stdout = self.stdout.clone();
            let line = format!("{json}\n");
            tokio::spawn(async move {
                let mut out = stdout.lock().await;
                out.write_all(line.as_bytes()).await.ok();
                out.flush().await.ok();
            });
        }
    }

    fn on_context_compacted(&self, before: usize, after: usize) {
        let notification = AcpNotification::context_compacted(before, after);
        if let Ok(json) = serde_json::to_string(&notification) {
            let stdout = self.stdout.clone();
            let line = format!("{json}\n");
            tokio::spawn(async move {
                let mut out = stdout.lock().await;
                out.write_all(line.as_bytes()).await.ok();
                out.flush().await.ok();
            });
        }
    }
}

/// ACP server that communicates over stdio, backed by an agent loop.
pub struct AcpServer {
    name: String,
    version: String,
    agent_loop: AgentLoop,
    /// Shared stdout for sending notifications alongside responses.
    stdout: Arc<Mutex<tokio::io::Stdout>>,
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

        // Build provider router
        let router = ProviderRouter::local_only(&config.model);

        // Build agent loop
        let tools = OllamaProvider::agentzero_tool_definitions();
        let loop_config = AgentLoopConfig::default();
        let agent_loop = AgentLoop::new(router, session, tools, loop_config)
            .with_system_prompt(&default_system_prompt());

        Ok(Self {
            name: "agentzero".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            agent_loop,
            stdout: Arc::new(Mutex::new(tokio::io::stdout())),
        })
    }

    /// Run the ACP server, reading from stdin and writing to stdout.
    pub async fn run(&mut self) -> Result<(), String> {
        info!("ACP server starting on stdio");

        let stdin = tokio::io::stdin();
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
                    self.send_response(&resp).await;
                    continue;
                }
            };

            let response = self.handle(&request).await;
            self.send_response(&response).await;

            if matches!(request.method, AcpMethod::Shutdown) {
                info!("ACP server: shutdown requested");
                break;
            }
        }

        self.agent_loop.end().ok();
        Ok(())
    }

    async fn send_response(&self, response: &AcpResponse) {
        let json = serde_json::to_string(response).expect("response should serialize");
        let mut out = self.stdout.lock().await;
        out.write_all(format!("{json}\n").as_bytes()).await.ok();
        out.flush().await.ok();
    }

    async fn handle(&mut self, request: &AcpRequest) -> AcpResponse {
        match request.method {
            AcpMethod::Initialize => AcpResponse::ok(
                &request.id,
                serde_json::json!({
                    "name": self.name,
                    "version": self.version,
                    "capabilities": [
                        "chat", "tool_call", "session_info",
                        "list_tools", "list_models", "switch_model"
                    ]
                }),
            ),
            AcpMethod::SessionInfo => AcpResponse::ok(
                &request.id,
                serde_json::json!({
                    "session_id": self.agent_loop.session_id(),
                    "model": self.agent_loop.model_name(),
                    "status": "ready",
                    "tools": self.agent_loop.tools().iter()
                        .map(|t| t.function.name.as_str())
                        .collect::<Vec<_>>()
                }),
            ),
            AcpMethod::ListTools => {
                let tools: Vec<serde_json::Value> = self
                    .agent_loop
                    .tools()
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "name": t.function.name,
                            "description": t.function.description
                        })
                    })
                    .collect();
                AcpResponse::ok(&request.id, serde_json::json!({ "tools": tools }))
            }
            AcpMethod::ToolCall => self.handle_tool_call(request).await,
            AcpMethod::Chat => self.handle_chat(request).await,
            AcpMethod::ListModels => {
                // For now, return the current model. Phase 3 will add multi-provider.
                AcpResponse::ok(
                    &request.id,
                    serde_json::json!({
                        "models": [{
                            "name": self.agent_loop.model_name(),
                            "provider": "ollama",
                            "active": true
                        }]
                    }),
                )
            }
            AcpMethod::SwitchModel => {
                // Phase 3 will implement this fully
                AcpResponse::err(&request.id, "model switching not yet implemented")
            }
            AcpMethod::ApproveAction => {
                // This is handled inline during chat — see AcpApprovalHandler
                AcpResponse::err(
                    &request.id,
                    "approve_action should be sent in response to a requires_approval notification",
                )
            }
            AcpMethod::Cancel => {
                // TODO: implement cancellation
                AcpResponse::ok(&request.id, serde_json::json!({"status": "cancelled"}))
            }
            AcpMethod::Shutdown => {
                AcpResponse::ok(&request.id, serde_json::json!({"status": "shutdown"}))
            }
        }
    }

    async fn handle_chat(&mut self, request: &AcpRequest) -> AcpResponse {
        let message = match request.params.get("message").and_then(|v| v.as_str()) {
            Some(m) => m,
            None => {
                return AcpResponse::err(&request.id, "missing 'message' in params");
            }
        };

        info!(message_len = message.len(), "ACP chat request");

        // For now, use AutoApprove for ACP (Phase 2.5 will add bidirectional approval).
        // The progress handler sends notifications over stdout.
        let approver = AutoApprove;
        let progress = AcpProgressHandler {
            stdout: self.stdout.clone(),
        };

        match self.agent_loop.send(message, &approver, &progress).await {
            Ok(response) => AcpResponse::ok(
                &request.id,
                serde_json::json!({
                    "content": response.content,
                    "model": response.model,
                    "rounds": response.rounds,
                    "tool_calls": response.tool_calls_made.iter().map(|tc| {
                        serde_json::json!({
                            "name": tc.name,
                            "success": tc.success,
                            "output_bytes": tc.output.len()
                        })
                    }).collect::<Vec<_>>()
                }),
            ),
            Err(e) => AcpResponse::err(&request.id, &e.to_string()),
        }
    }

    async fn handle_tool_call(&mut self, request: &AcpRequest) -> AcpResponse {
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

        // Direct tool call wraps it as a chat message so it goes through the agent loop
        // For backward compat, also support direct tool execution
        let approver = AutoApprove;
        let progress = AcpProgressHandler {
            stdout: self.stdout.clone(),
        };

        let prompt = format!(
            "Use the {tool_name} tool with these arguments: {}",
            serde_json::to_string(&arguments).unwrap_or_default()
        );

        match self.agent_loop.send(&prompt, &approver, &progress).await {
            Ok(response) => AcpResponse::ok(
                &request.id,
                serde_json::json!({
                    "success": true,
                    "content": response.content,
                    "tool_calls": response.tool_calls_made.iter().map(|tc| {
                        serde_json::json!({
                            "name": tc.name,
                            "success": tc.success,
                            "output": tc.output
                        })
                    }).collect::<Vec<_>>()
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

fn default_system_prompt() -> String {
    "You are AgentZero, a local-first secure AI coding assistant. \
     You have tools available: read (read files), list (list directories), \
     search (search file contents), write (write files, requires approval), \
     and shell (execute commands, requires approval). \
     Use tools to help the user with their coding tasks. \
     Always explain what you're doing before using tools."
        .to_string()
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

    #[tokio::test]
    async fn handle_initialize() {
        let mut server = test_server();
        let req = AcpRequest {
            id: "1".into(),
            method: AcpMethod::Initialize,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req).await;
        assert!(resp.success);
        let result = resp.result.expect("should have result");
        assert!(result["name"]
            .as_str()
            .expect("should be string")
            .contains("agentzero"));
        // Check new capabilities are advertised
        let caps = result["capabilities"].as_array().expect("should be array");
        let cap_strs: Vec<&str> = caps.iter().filter_map(|v| v.as_str()).collect();
        assert!(cap_strs.contains(&"chat"));
        assert!(cap_strs.contains(&"list_models"));
    }

    #[tokio::test]
    async fn handle_list_tools() {
        let mut server = test_server();
        let req = AcpRequest {
            id: "2".into(),
            method: AcpMethod::ListTools,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req).await;
        assert!(resp.success);
        let tools = &resp.result.expect("should have result")["tools"];
        assert!(tools.as_array().expect("should be array").len() >= 6);
    }

    #[tokio::test]
    async fn handle_shutdown() {
        let mut server = test_server();
        let req = AcpRequest {
            id: "6".into(),
            method: AcpMethod::Shutdown,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req).await;
        assert!(resp.success);
    }

    #[tokio::test]
    async fn handle_session_info() {
        let mut server = test_server();
        let req = AcpRequest {
            id: "7".into(),
            method: AcpMethod::SessionInfo,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req).await;
        assert!(resp.success);
        let result = resp.result.expect("should have result");
        assert!(result["session_id"].as_str().is_some());
        assert_eq!(result["status"], "ready");
        // New: includes model name
        assert!(result["model"].as_str().is_some());
    }

    #[tokio::test]
    async fn handle_list_models() {
        let mut server = test_server();
        let req = AcpRequest {
            id: "8".into(),
            method: AcpMethod::ListModels,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req).await;
        assert!(resp.success);
        let result = resp.result.expect("should have result");
        let models = result["models"].as_array().expect("should be array");
        assert!(!models.is_empty());
        assert_eq!(models[0]["active"], true);
    }

    #[tokio::test]
    async fn handle_chat_missing_message() {
        let mut server = test_server();
        let req = AcpRequest {
            id: "9".into(),
            method: AcpMethod::Chat,
            params: serde_json::json!({}),
        };
        let resp = server.handle(&req).await;
        assert!(!resp.success);
        assert!(resp.error.expect("should have error").contains("missing"));
    }

    #[tokio::test]
    async fn handle_switch_model_not_yet() {
        let mut server = test_server();
        let req = AcpRequest {
            id: "10".into(),
            method: AcpMethod::SwitchModel,
            params: serde_json::json!({"model": "codellama"}),
        };
        let resp = server.handle(&req).await;
        assert!(!resp.success);
        assert!(resp.error.expect("should have error").contains("not yet"));
    }
}
