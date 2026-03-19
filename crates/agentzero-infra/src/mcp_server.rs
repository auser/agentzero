//! MCP Server — Exposes AgentZero's tools via the Model Context Protocol.
//!
//! Implements the server side of MCP JSON-RPC 2.0:
//! - `initialize` → return server capabilities
//! - `tools/list` → enumerate all registered tools with schemas
//! - `tools/call` → execute a tool and return the result
//!
//! Two transports are supported:
//! - **stdio**: for Claude Desktop / Cursor / Windsurf integration
//! - **HTTP**: via gateway routes (`POST /mcp/message`)

use agentzero_core::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use std::sync::Arc;

/// Protocol version supported by this server.
const PROTOCOL_VERSION: &str = "2025-11-05";

/// MCP Server that wraps a set of `Tool` implementations.
pub struct McpServer {
    tools: Vec<Arc<dyn Tool>>,
    workspace_root: String,
    server_name: String,
    server_version: String,
}

impl McpServer {
    /// Create a new MCP server wrapping the given tools.
    pub fn new(tools: Vec<Box<dyn Tool>>, workspace_root: String) -> Self {
        Self {
            tools: tools.into_iter().map(Arc::from).collect(),
            workspace_root,
            server_name: "agentzero".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Handle a JSON-RPC 2.0 request and return a response.
    ///
    /// Returns `None` for notifications (no `id` field).
    pub async fn handle_message(&self, message: &Value) -> Option<Value> {
        // Notifications have no id — don't send a response.
        let id = message.get("id").cloned()?;
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(json!({}));

        let result = match method {
            "initialize" => self.handle_initialize(&params),
            "tools/list" => self.handle_tools_list(&params),
            "tools/call" => self.handle_tools_call(&params).await,
            "ping" => Ok(json!({})),
            _ => Err(json!({
                "code": -32601,
                "message": format!("method not found: {method}"),
            })),
        };

        let response = match result {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": error,
            }),
        };

        Some(response)
    }

    fn handle_initialize(&self, _params: &Value) -> Result<Value, Value> {
        Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": {
                    "listChanged": false
                }
            },
            "serverInfo": {
                "name": self.server_name,
                "version": self.server_version
            }
        }))
    }

    fn handle_tools_list(&self, _params: &Value) -> Result<Value, Value> {
        let tools: Vec<Value> = self
            .tools
            .iter()
            .map(|tool| {
                let schema = tool.input_schema().unwrap_or_else(|| {
                    json!({
                        "type": "object",
                        "properties": {},
                        "additionalProperties": true
                    })
                });
                json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "inputSchema": schema,
                })
            })
            .collect();

        Ok(json!({ "tools": tools }))
    }

    async fn handle_tools_call(&self, params: &Value) -> Result<Value, Value> {
        let tool_name = params.get("name").and_then(Value::as_str).ok_or_else(|| {
            json!({
                "code": -32602,
                "message": "missing required parameter: name",
            })
        })?;

        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| {
                json!({
                    "code": -32602,
                    "message": format!("unknown tool: {tool_name}"),
                })
            })?;

        let input = if arguments.is_object() {
            serde_json::to_string(&arguments).unwrap_or_default()
        } else {
            arguments.as_str().unwrap_or("").to_string()
        };

        let ctx = ToolContext::new(self.workspace_root.clone());

        match tool.execute(&input, &ctx).await {
            Ok(ToolResult { output }) => Ok(json!({
                "content": [{
                    "type": "text",
                    "text": output,
                }],
                "isError": false,
            })),
            Err(e) => Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("tool execution error: {e}"),
                }],
                "isError": true,
            })),
        }
    }

    /// List all tool names (for diagnostics / logging).
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    /// Number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Find a tool by name and execute it directly (used by gateway tool_execute).
    pub async fn execute_tool(&self, tool_name: &str, input: &str) -> anyhow::Result<ToolResult> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| anyhow::anyhow!("unknown tool: {tool_name}"))?;

        let ctx = ToolContext::new(self.workspace_root.clone());
        tool.execute(input, &ctx).await
    }
}

// --- stdio transport helpers ---

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Write a JSON-RPC message with Content-Length framing to a writer.
pub async fn write_message<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    payload: &Value,
) -> anyhow::Result<()> {
    let bytes =
        serde_json::to_vec(payload).map_err(|e| anyhow::anyhow!("failed to encode: {e}"))?;
    let header = format!("Content-Length: {}\r\n\r\n", bytes.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("failed to write header: {e}"))?;
    writer
        .write_all(&bytes)
        .await
        .map_err(|e| anyhow::anyhow!("failed to write body: {e}"))?;
    writer
        .flush()
        .await
        .map_err(|e| anyhow::anyhow!("failed to flush: {e}"))?;
    Ok(())
}

/// Read a JSON-RPC message with Content-Length framing from a reader.
pub async fn read_message<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> anyhow::Result<Value> {
    let mut content_length = None::<usize>;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .map_err(|e| anyhow::anyhow!("failed reading header: {e}"))?;

        if read == 0 {
            return Err(anyhow::anyhow!("EOF"));
        }

        if line == "\r\n" || line == "\n" {
            break;
        }

        let lower = line.to_ascii_lowercase();
        if lower.starts_with("content-length:") {
            if let Some((_, value)) = lower.split_once(':') {
                let len = value
                    .trim()
                    .parse::<usize>()
                    .map_err(|e| anyhow::anyhow!("invalid content-length: {e}"))?;
                content_length = Some(len);
            }
        }
    }

    let len = content_length.ok_or_else(|| anyhow::anyhow!("missing content-length"))?;
    let mut body = vec![0_u8; len];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|e| anyhow::anyhow!("failed reading body: {e}"))?;

    serde_json::from_slice::<Value>(&body).map_err(|e| anyhow::anyhow!("invalid JSON: {e}"))
}

/// Run the MCP server over stdio (stdin/stdout).
///
/// Reads JSON-RPC messages from stdin, processes them, writes responses to stdout.
/// Exits when stdin is closed (EOF).
pub async fn run_stdio(server: Arc<McpServer>) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);

    loop {
        let message = match read_message(&mut reader).await {
            Ok(msg) => msg,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("EOF") {
                    tracing::info!("mcp-serve: stdin closed, shutting down");
                    break;
                }
                tracing::warn!("mcp-serve: read error: {e}");
                continue;
            }
        };

        if let Some(response) = server.handle_message(&message).await {
            if let Err(e) = write_message(&mut stdout, &response).await {
                tracing::error!("mcp-serve: write error: {e}");
                break;
            }
        }

        // Handle notifications/initialized — no response needed, just log.
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        if method == "notifications/initialized" {
            tracing::info!("mcp-serve: client initialized");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolResult;
    use async_trait::async_trait;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn description(&self) -> &'static str {
            "Echoes back the input"
        }
        fn input_schema(&self) -> Option<Value> {
            Some(json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                },
                "required": ["text"]
            }))
        }
        async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            let parsed: Value = serde_json::from_str(input).unwrap_or(json!({}));
            let text = parsed.get("text").and_then(Value::as_str).unwrap_or(input);
            Ok(ToolResult {
                output: text.to_string(),
            })
        }
    }

    struct FailTool;

    #[async_trait]
    impl Tool for FailTool {
        fn name(&self) -> &'static str {
            "fail"
        }
        fn description(&self) -> &'static str {
            "Always fails"
        }
        async fn execute(&self, _input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Err(anyhow::anyhow!("intentional failure"))
        }
    }

    fn test_server() -> McpServer {
        McpServer::new(
            vec![Box::new(EchoTool), Box::new(FailTool)],
            "/tmp".to_string(),
        )
    }

    #[tokio::test]
    async fn initialize_returns_capabilities() {
        let server = test_server();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1.0"}
            }
        });
        let resp = server.handle_message(&msg).await.expect("should respond");
        let result = &resp["result"];
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["serverInfo"]["name"], "agentzero");
    }

    #[tokio::test]
    async fn tools_list_returns_all_tools() {
        let server = test_server();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        let resp = server.handle_message(&msg).await.expect("should respond");
        let tools = resp["result"]["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "echo");
        assert_eq!(tools[0]["description"], "Echoes back the input");
        assert!(tools[0]["inputSchema"]["properties"]["text"].is_object());
        assert_eq!(tools[1]["name"], "fail");
    }

    #[tokio::test]
    async fn tools_call_executes_tool() {
        let server = test_server();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "echo",
                "arguments": { "text": "hello world" }
            }
        });
        let resp = server.handle_message(&msg).await.expect("should respond");
        let result = &resp["result"];
        assert_eq!(result["isError"], false);
        let content = result["content"].as_array().expect("content array");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "hello world");
    }

    #[tokio::test]
    async fn tools_call_returns_error_for_unknown_tool() {
        let server = test_server();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "nonexistent",
                "arguments": {}
            }
        });
        let resp = server.handle_message(&msg).await.expect("should respond");
        assert!(resp["error"].is_object());
        assert_eq!(resp["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn tools_call_handles_tool_failure() {
        let server = test_server();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "fail",
                "arguments": {}
            }
        });
        let resp = server.handle_message(&msg).await.expect("should respond");
        let result = &resp["result"];
        assert_eq!(result["isError"], true);
        let text = result["content"][0]["text"].as_str().expect("text");
        assert!(text.contains("intentional failure"));
    }

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let server = test_server();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "nonexistent/method",
            "params": {}
        });
        let resp = server.handle_message(&msg).await.expect("should respond");
        assert!(resp["error"].is_object());
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn notification_returns_none() {
        let server = test_server();
        // Notifications have no "id" field.
        let msg = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        let resp = server.handle_message(&msg).await;
        assert!(
            resp.is_none(),
            "notifications should not produce a response"
        );
    }

    #[tokio::test]
    async fn ping_returns_empty_result() {
        let server = test_server();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "ping",
            "params": {}
        });
        let resp = server.handle_message(&msg).await.expect("should respond");
        assert!(resp["result"].is_object());
        assert!(resp["error"].is_null());
    }

    #[tokio::test]
    async fn tools_call_missing_name_returns_error() {
        let server = test_server();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {
                "arguments": {}
            }
        });
        let resp = server.handle_message(&msg).await.expect("should respond");
        assert_eq!(resp["error"]["code"], -32602);
        assert!(resp["error"]["message"]
            .as_str()
            .expect("message")
            .contains("missing"));
    }

    #[tokio::test]
    async fn execute_tool_directly() {
        let server = test_server();
        let result = server
            .execute_tool("echo", r#"{"text":"direct"}"#)
            .await
            .expect("should succeed");
        assert_eq!(result.output, "direct");
    }

    #[tokio::test]
    async fn execute_tool_unknown_returns_error() {
        let server = test_server();
        let err = server
            .execute_tool("nope", "{}")
            .await
            .expect_err("should fail");
        assert!(err.to_string().contains("unknown tool"));
    }

    #[test]
    fn tool_count_and_names() {
        let server = test_server();
        assert_eq!(server.tool_count(), 2);
        let names = server.tool_names();
        assert_eq!(names, vec!["echo", "fail"]);
    }

    #[tokio::test]
    async fn write_and_read_message_roundtrip() {
        let payload = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
        let mut buf = Vec::new();
        write_message(&mut buf, &payload).await.expect("write");

        let mut reader = BufReader::new(buf.as_slice());
        let read_back = read_message(&mut reader).await.expect("read");
        assert_eq!(read_back, payload);
    }
}
