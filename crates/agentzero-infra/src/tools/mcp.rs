use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

const DEFAULT_MCP_TIMEOUT_MS: u64 = 10_000;

#[derive(Debug, Clone, Deserialize)]
struct McpServerConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct McpCallInput {
    server: String,
    tool: String,
    #[serde(default)]
    arguments: Value,
}

impl McpCallInput {
    fn parse(raw: &str) -> anyhow::Result<Self> {
        if raw.trim().is_empty() {
            return Err(anyhow!(
                "mcp input must be JSON with server/tool/arguments object"
            ));
        }

        let parsed: Self = serde_json::from_str(raw).context("invalid mcp JSON input")?;
        if parsed.server.trim().is_empty() {
            return Err(anyhow!("mcp server is required"));
        }
        if parsed.tool.trim().is_empty() {
            return Err(anyhow!("mcp tool is required"));
        }
        if !parsed.arguments.is_object() {
            return Err(anyhow!("mcp arguments must be a JSON object"));
        }

        Ok(parsed)
    }
}

/// A cached MCP server session holding the subprocess and its I/O handles.
struct McpSession {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    child: Child,
    next_id: u64,
    /// Tool schemas captured from `tools/list` response.
    tool_schemas: HashMap<String, Value>,
}

impl McpSession {
    /// Send a JSON-RPC request and return the result, using an auto-incrementing ID.
    async fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout_ms: u64,
    ) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        write_message(&mut self.stdin, &req).await?;
        read_response_for_id(&mut self.stdout, id, timeout_ms).await
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        // Best-effort kill when session is dropped.
        let _ = self.child.start_kill();
    }
}

pub struct McpTool {
    servers: HashMap<String, McpServerConfig>,
    timeout_ms: u64,
    /// Cached sessions per server name. Each slot is `None` until first connect.
    sessions: HashMap<String, Arc<Mutex<Option<McpSession>>>>,
}

impl McpTool {
    pub fn from_env(allowed_server_names: &[String]) -> anyhow::Result<Self> {
        let raw = std::env::var("AGENTZERO_MCP_SERVERS").unwrap_or_else(|_| "{}".to_string());
        Self::from_servers_json(&raw, allowed_server_names)
    }

    fn from_servers_json(raw: &str, allowed_server_names: &[String]) -> anyhow::Result<Self> {
        if allowed_server_names.is_empty() {
            return Err(anyhow!(
                "mcp is enabled but no allowed servers were configured"
            ));
        }

        let all_servers: HashMap<String, McpServerConfig> =
            serde_json::from_str(raw).context("AGENTZERO_MCP_SERVERS must be valid JSON")?;
        let mut servers = HashMap::new();

        for allowed in allowed_server_names {
            if let Some(cfg) = all_servers.get(allowed) {
                servers.insert(allowed.clone(), cfg.clone());
            }
        }

        if servers.is_empty() {
            return Err(anyhow!(
                "no allowed MCP servers were found in AGENTZERO_MCP_SERVERS"
            ));
        }

        for (name, cfg) in &servers {
            if name.trim().is_empty() {
                return Err(anyhow!("mcp server name cannot be empty"));
            }
            if cfg.command.trim().is_empty() {
                return Err(anyhow!("mcp server `{name}` command cannot be empty"));
            }
            if cfg.args.len() > 64 {
                return Err(anyhow!("mcp server `{name}` has too many startup args"));
            }
        }

        let sessions = servers
            .keys()
            .map(|name| (name.clone(), Arc::new(Mutex::new(None))))
            .collect();

        Ok(Self {
            servers,
            timeout_ms: DEFAULT_MCP_TIMEOUT_MS,
            sessions,
        })
    }

    /// Spawn a new MCP server process, initialize the session, and cache tool schemas.
    async fn spawn_session(&self, server_name: &str) -> anyhow::Result<McpSession> {
        let config = self
            .servers
            .get(server_name)
            .ok_or_else(|| anyhow!("unknown mcp server `{server_name}`"))?;

        let mut child = Command::new(&config.command)
            .args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn mcp server command: {} {}",
                    config.command,
                    config.args.join(" ")
                )
            })?;

        let child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("mcp server stdin unavailable"))?;
        let child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("mcp server stdout unavailable"))?;

        let mut session = McpSession {
            stdin: child_stdin,
            stdout: BufReader::new(child_stdout),
            child,
            next_id: 1,
            tool_schemas: HashMap::new(),
        };

        // Initialize handshake.
        let init_params = json!({
            "protocolVersion": "2025-11-05",
            "capabilities": {},
            "clientInfo": {"name": "agentzero", "version": "0.1.0"}
        });
        session
            .request("initialize", init_params, self.timeout_ms)
            .await?;

        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        write_message(&mut session.stdin, &notification).await?;

        // List tools and cache schemas.
        let result = session
            .request("tools/list", json!({}), self.timeout_ms)
            .await?;
        if let Some(tools) = result.get("tools").and_then(Value::as_array) {
            for tool in tools {
                if let Some(name) = tool.get("name").and_then(Value::as_str) {
                    let schema = tool
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or(json!({"type": "object"}));
                    session.tool_schemas.insert(name.to_string(), schema);
                }
            }
        }

        Ok(session)
    }

    /// Execute a tool call on a cached session.
    async fn call_tool_cached(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> anyhow::Result<String> {
        let slot = self
            .sessions
            .get(server_name)
            .ok_or_else(|| anyhow!("unknown mcp server `{server_name}`"))?
            .clone();

        let mut guard = slot.lock().await;
        let session = guard
            .as_mut()
            .ok_or_else(|| anyhow!("mcp session not initialized"))?;

        // Validate tool exists in cached schema list.
        if !session.tool_schemas.contains_key(tool_name) {
            return Err(anyhow!(
                "mcp tool `{tool_name}` not exposed by selected server"
            ));
        }

        let params = json!({
            "name": tool_name,
            "arguments": arguments,
        });
        let result = session
            .request("tools/call", params, self.timeout_ms)
            .await?;

        if let Some(content) = result.get("content").and_then(Value::as_array) {
            let text_chunks = content
                .iter()
                .filter_map(|entry| entry.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>();
            if !text_chunks.is_empty() {
                return Ok(text_chunks.join("\n"));
            }
        }

        Ok(result.to_string())
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &'static str {
        "mcp"
    }

    fn description(&self) -> &'static str {
        "Call a tool on an MCP (Model Context Protocol) server."
    }

    fn input_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "required": ["server", "tool"],
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Name of the MCP server to call"
                },
                "tool": {
                    "type": "string",
                    "description": "Tool name to invoke on the server"
                },
                "arguments": {
                    "type": "object",
                    "description": "JSON object with tool arguments"
                }
            }
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed = McpCallInput::parse(input)?;
        let server_name = parsed.server.clone();
        let tool_name = parsed.tool.clone();
        let arguments = parsed.arguments.clone();

        let server_name_ref = server_name.as_str();

        // Ensure session exists (lazy connect).
        {
            let slot = self
                .sessions
                .get(server_name_ref)
                .ok_or_else(|| anyhow!("unknown mcp server `{}`", parsed.server))?
                .clone();
            let mut guard = slot.lock().await;
            if guard.is_none() {
                let session = self.spawn_session(server_name_ref).await?;
                *guard = Some(session);
            }
        }

        // First attempt.
        match self
            .call_tool_cached(server_name_ref, &tool_name, arguments.clone())
            .await
        {
            Ok(output) => return Ok(ToolResult { output }),
            Err(e) => {
                tracing::warn!(
                    server = %server_name_ref,
                    tool = %tool_name,
                    error = %e,
                    "mcp call failed, reconnecting"
                );
                // Clear session for reconnect.
                let slot = self.sessions.get(server_name_ref).unwrap().clone();
                let mut guard = slot.lock().await;
                *guard = None;
            }
        }

        // Reconnect attempt.
        {
            let slot = self.sessions.get(server_name_ref).unwrap().clone();
            let mut guard = slot.lock().await;
            if guard.is_none() {
                let session = self.spawn_session(server_name_ref).await?;
                *guard = Some(session);
            }
        }

        let output = self
            .call_tool_cached(server_name_ref, &tool_name, arguments)
            .await
            .with_context(|| {
                format!(
                    "mcp call failed for server `{}` tool `{}`",
                    parsed.server, parsed.tool
                )
            })?;

        Ok(ToolResult { output })
    }
}

async fn write_message(stdin: &mut ChildStdin, payload: &Value) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(payload).context("failed to encode mcp payload")?;
    let header = format!("Content-Length: {}\r\n\r\n", bytes.len());

    stdin
        .write_all(header.as_bytes())
        .await
        .context("failed to write mcp header")?;
    stdin
        .write_all(&bytes)
        .await
        .context("failed to write mcp body")?;
    stdin.flush().await.context("failed to flush mcp stdin")?;
    Ok(())
}

async fn read_message(stdout: &mut BufReader<ChildStdout>) -> anyhow::Result<Value> {
    let mut content_length = None::<usize>;
    loop {
        let mut line = String::new();
        let read = stdout
            .read_line(&mut line)
            .await
            .context("failed reading mcp header line")?;

        if read == 0 {
            return Err(anyhow!("unexpected EOF from mcp server"));
        }

        if line == "\r\n" || line == "\n" {
            break;
        }

        let lower = line.to_ascii_lowercase();
        if let Some((_, value)) = lower.split_once(':') {
            if lower.starts_with("content-length:") {
                let len = value
                    .trim()
                    .parse::<usize>()
                    .context("invalid mcp content-length")?;
                content_length = Some(len);
            }
        }
    }

    let len = content_length.ok_or_else(|| anyhow!("missing mcp content-length header"))?;
    let mut body = vec![0_u8; len];
    stdout
        .read_exact(&mut body)
        .await
        .context("failed reading mcp message body")?;

    let value = serde_json::from_slice::<Value>(&body).context("invalid mcp JSON body")?;
    Ok(value)
}

async fn read_response_for_id(
    stdout: &mut BufReader<ChildStdout>,
    id: u64,
    timeout_ms: u64,
) -> anyhow::Result<Value> {
    let deadline = Duration::from_millis(timeout_ms);

    timeout(deadline, async move {
        loop {
            let message = read_message(stdout).await?;

            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }

            if let Some(error) = message.get("error") {
                return Err(anyhow!("mcp error: {}", error));
            }

            let result = message
                .get("result")
                .cloned()
                .ok_or_else(|| anyhow!("mcp response missing result"))?;
            return Ok(result);
        }
    })
    .await
    .context("timed out waiting for mcp response")?
}

#[cfg(test)]
mod tests {
    use super::{McpCallInput, McpTool};
    use agentzero_core::{Tool, ToolContext};

    #[test]
    fn parses_valid_mcp_input() {
        let input = r#"{"server":"fs","tool":"read_file","arguments":{"path":"README.md"}}"#;
        let parsed = McpCallInput::parse(input).expect("valid mcp input should parse");

        assert_eq!(parsed.server, "fs");
        assert_eq!(parsed.tool, "read_file");
        assert_eq!(parsed.arguments["path"], "README.md");
    }

    #[tokio::test]
    async fn rejects_unknown_server_fail_closed() {
        let tool = McpTool::from_servers_json("{}", &[String::from("fs")]);
        assert!(tool.is_err());
        let err = tool
            .err()
            .expect("missing allowed server definitions should fail");

        assert!(err
            .to_string()
            .contains("no allowed MCP servers were found"));
    }

    #[tokio::test]
    async fn rejects_unknown_server_in_request() {
        let tool = McpTool::from_servers_json(
            r#"{"fs":{"command":"cat","args":[]}}"#,
            &[String::from("fs")],
        )
        .expect("mcp config should parse");

        let result = tool
            .execute(
                r#"{"server":"missing","tool":"read_file","arguments":{}}"#,
                &ToolContext::new(".".to_string()),
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("unknown server should fail")
            .to_string()
            .contains("unknown mcp server"));
    }

    #[test]
    fn input_schema_returns_dispatcher_schema() {
        let tool = McpTool::from_servers_json(
            r#"{"fs":{"command":"echo","args":[]}}"#,
            &[String::from("fs")],
        )
        .expect("mcp config should parse");

        let schema = tool.input_schema().expect("should return schema");
        assert_eq!(schema["type"], "object");
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("server")));
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("tool")));
        assert!(schema["properties"]["server"]["type"] == "string");
        assert!(schema["properties"]["tool"]["type"] == "string");
        assert!(schema["properties"]["arguments"]["type"] == "object");
    }

    #[test]
    fn session_slots_created_for_each_server() {
        let tool = McpTool::from_servers_json(
            r#"{"fs":{"command":"echo","args":[]},"git":{"command":"echo","args":[]}}"#,
            &[String::from("fs"), String::from("git")],
        )
        .expect("mcp config should parse");

        assert_eq!(tool.sessions.len(), 2);
        assert!(tool.sessions.contains_key("fs"));
        assert!(tool.sessions.contains_key("git"));
    }
}
