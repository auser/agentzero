use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_tools::McpServerDef;
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

const DEFAULT_MCP_TIMEOUT_MS: u64 = 10_000;

// ---------------------------------------------------------------------------
// MCP session (subprocess I/O) — kept from original implementation
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// MCP server connection — shared across all tools from the same server
// ---------------------------------------------------------------------------

/// Metadata about a single tool discovered from an MCP server.
struct McpToolInfo {
    name: String,
    description: String,
    input_schema: Value,
}

/// Shared connection to a single MCP server subprocess.
///
/// Multiple [`McpIndividualTool`] instances from the same server hold an
/// `Arc` to the same `McpServerConnection`. The `Mutex<Option<McpSession>>`
/// serializes access to the subprocess stdin/stdout.
pub(crate) struct McpServerConnection {
    server_name: String,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    timeout_ms: u64,
    session: Mutex<Option<McpSession>>,
}

impl McpServerConnection {
    fn new(server_name: String, def: &McpServerDef, timeout_ms: u64) -> Self {
        Self {
            server_name,
            command: def.command.clone(),
            args: def.args.clone(),
            env: def.env.clone(),
            timeout_ms,
            session: Mutex::new(None),
        }
    }

    /// Connect to the MCP server, perform the protocol handshake, discover
    /// all tools via `tools/list`, cache the session, and return tool metadata
    /// (name, description, input schema) for each discovered tool.
    async fn connect_and_discover(&self) -> anyhow::Result<Vec<McpToolInfo>> {
        let mut session = self.spawn_session().await?;

        // tools/list was already called in spawn_session (which caches schemas),
        // but we also need descriptions. Do a second tools/list call to get
        // the full metadata.
        let result = session
            .request("tools/list", json!({}), self.timeout_ms)
            .await?;

        let mut tool_infos = Vec::new();
        if let Some(tools) = result.get("tools").and_then(Value::as_array) {
            for tool in tools {
                let name = match tool.get("name").and_then(Value::as_str) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let description = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let input_schema = tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or(json!({"type": "object"}));

                session
                    .tool_schemas
                    .insert(name.clone(), input_schema.clone());

                tool_infos.push(McpToolInfo {
                    name,
                    description,
                    input_schema,
                });
            }
        }

        // Cache the session for future tool calls.
        let mut guard = self.session.lock().await;
        *guard = Some(session);

        Ok(tool_infos)
    }

    /// Spawn a new MCP server process and perform the protocol handshake.
    async fn spawn_session(&self) -> anyhow::Result<McpSession> {
        let mut cmd = Command::new(&self.command);
        cmd.args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Set custom environment variables.
        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "failed to spawn mcp server command: {} {}",
                self.command,
                self.args.join(" ")
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

    /// Execute a tool call, with lazy connect and one reconnect attempt on failure.
    async fn call_tool(&self, tool_name: &str, arguments: Value) -> anyhow::Result<String> {
        // Ensure session exists (lazy connect).
        {
            let mut guard = self.session.lock().await;
            if guard.is_none() {
                let session = self.spawn_session().await?;
                *guard = Some(session);
            }
        }

        // First attempt.
        match self.call_tool_inner(tool_name, &arguments).await {
            Ok(output) => return Ok(output),
            Err(e) => {
                tracing::warn!(
                    server = %self.server_name,
                    tool = %tool_name,
                    error = %e,
                    "mcp call failed, reconnecting"
                );
                // Clear session for reconnect.
                let mut guard = self.session.lock().await;
                *guard = None;
            }
        }

        // Reconnect attempt.
        {
            let mut guard = self.session.lock().await;
            if guard.is_none() {
                let session = self.spawn_session().await?;
                *guard = Some(session);
            }
        }

        self.call_tool_inner(tool_name, &arguments)
            .await
            .with_context(|| {
                format!(
                    "mcp call failed for server `{}` tool `{}`",
                    self.server_name, tool_name
                )
            })
    }

    async fn call_tool_inner(&self, tool_name: &str, arguments: &Value) -> anyhow::Result<String> {
        let mut guard = self.session.lock().await;
        let session = guard
            .as_mut()
            .ok_or_else(|| anyhow!("mcp session not initialized"))?;

        // Validate tool exists in cached schema list.
        if !session.tool_schemas.contains_key(tool_name) {
            return Err(anyhow!(
                "mcp tool `{tool_name}` not exposed by server `{}`",
                self.server_name
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

// ---------------------------------------------------------------------------
// Individual MCP tool — one per tool per server
// ---------------------------------------------------------------------------

/// A single tool from an MCP server, registered as a first-class `Tool`.
///
/// The LLM sees this as a regular tool with its own name (e.g. `mcp__fs__read_file`),
/// description, and input schema.
pub struct McpIndividualTool {
    /// Leaked `"mcp__{server}__{tool}"` for the `&'static str` requirement.
    name: &'static str,
    /// Leaked description from the MCP server's `tools/list` response.
    description: &'static str,
    /// Input schema from the MCP server.
    schema: Value,
    /// Original tool name on the MCP server (before namespacing).
    mcp_tool_name: String,
    /// Shared connection to the MCP server subprocess.
    connection: Arc<McpServerConnection>,
}

#[async_trait]
impl Tool for McpIndividualTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn input_schema(&self) -> Option<Value> {
        Some(self.schema.clone())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let arguments: Value = if input.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(input).context("invalid JSON input for mcp tool")?
        };

        if !arguments.is_object() {
            return Err(anyhow!("mcp tool arguments must be a JSON object"));
        }

        let output = self
            .connection
            .call_tool(&self.mcp_tool_name, arguments)
            .await?;

        Ok(ToolResult { output })
    }
}

// ---------------------------------------------------------------------------
// Factory: create all MCP tools from server definitions
// ---------------------------------------------------------------------------

/// Create first-class `Tool` instances for every tool exposed by every
/// configured MCP server.
///
/// Each server is connected at startup via `tools/list` to discover its tools.
/// If a server fails to connect, it is skipped with a warning (graceful degradation).
pub fn create_mcp_tools(
    servers: &HashMap<String, McpServerDef>,
) -> anyhow::Result<Vec<Box<dyn Tool>>> {
    if servers.is_empty() {
        return Ok(vec![]);
    }

    let handle = tokio::runtime::Handle::current();
    let mut all_tools: Vec<Box<dyn Tool>> = Vec::new();

    for (server_name, def) in servers {
        if def.command.trim().is_empty() {
            tracing::warn!("mcp server `{server_name}` has empty command, skipping");
            continue;
        }

        let connection = Arc::new(McpServerConnection::new(
            server_name.clone(),
            def,
            DEFAULT_MCP_TIMEOUT_MS,
        ));

        // Connect and discover tools. Use block_in_place because default_tools()
        // is sync (required by ToolBuilder type alias).
        let tool_infos = match tokio::task::block_in_place(|| {
            handle.block_on(connection.connect_and_discover())
        }) {
            Ok(infos) => infos,
            Err(e) => {
                tracing::warn!(
                    server = %server_name,
                    error = %e,
                    "failed to connect to mcp server, skipping"
                );
                continue;
            }
        };

        if tool_infos.is_empty() {
            tracing::warn!(
                server = %server_name,
                "mcp server exposes no tools, skipping"
            );
            continue;
        }

        for info in tool_infos {
            let full_name = format!("mcp__{}__{}", server_name, sanitize_tool_name(&info.name));
            let leaked_name: &'static str = Box::leak(full_name.into_boxed_str());
            let leaked_desc: &'static str = Box::leak(info.description.into_boxed_str());

            all_tools.push(Box::new(McpIndividualTool {
                name: leaked_name,
                description: leaked_desc,
                schema: info.input_schema,
                mcp_tool_name: info.name,
                connection: Arc::clone(&connection),
            }));
        }

        tracing::info!(
            server = %server_name,
            tool_count = all_tools.len(),
            "registered mcp server tools"
        );
    }

    Ok(all_tools)
}

/// Replace non-alphanumeric/underscore characters with `_` to ensure tool
/// names are safe for LLM tool-use APIs.
fn sanitize_tool_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Transport helpers — kept from original implementation
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::Tool;

    #[test]
    fn sanitize_tool_name_replaces_special_chars() {
        assert_eq!(sanitize_tool_name("read_file"), "read_file");
        assert_eq!(sanitize_tool_name("read-file"), "read_file");
        assert_eq!(sanitize_tool_name("fs.read"), "fs_read");
        assert_eq!(sanitize_tool_name("tool/name"), "tool_name");
        assert_eq!(sanitize_tool_name("already_clean_123"), "already_clean_123");
    }

    #[test]
    fn mcp_individual_tool_name_format() {
        let server_name = "filesystem";
        let tool_name = "read-file";
        let full_name = format!("mcp__{}__{}", server_name, sanitize_tool_name(tool_name));
        assert_eq!(full_name, "mcp__filesystem__read_file");
    }

    #[test]
    fn mcp_individual_tool_trait_returns_correct_values() {
        let connection = Arc::new(McpServerConnection::new(
            "test".to_string(),
            &McpServerDef {
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
            10_000,
        ));

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        });

        let tool = McpIndividualTool {
            name: Box::leak("mcp__test__read_file".to_string().into_boxed_str()),
            description: Box::leak("Read a file from disk".to_string().into_boxed_str()),
            schema: schema.clone(),
            mcp_tool_name: "read_file".to_string(),
            connection,
        };

        assert_eq!(tool.name(), "mcp__test__read_file");
        assert_eq!(tool.description(), "Read a file from disk");
        assert_eq!(tool.input_schema().unwrap(), schema);
    }

    #[test]
    fn shared_connection_across_tools() {
        let connection = Arc::new(McpServerConnection::new(
            "fs".to_string(),
            &McpServerDef {
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
            10_000,
        ));

        let tool1 = McpIndividualTool {
            name: Box::leak("mcp__fs__read".to_string().into_boxed_str()),
            description: Box::leak(String::new().into_boxed_str()),
            schema: serde_json::json!({"type": "object"}),
            mcp_tool_name: "read".to_string(),
            connection: Arc::clone(&connection),
        };

        let tool2 = McpIndividualTool {
            name: Box::leak("mcp__fs__write".to_string().into_boxed_str()),
            description: Box::leak(String::new().into_boxed_str()),
            schema: serde_json::json!({"type": "object"}),
            mcp_tool_name: "write".to_string(),
            connection: Arc::clone(&connection),
        };

        // Both tools share the same connection (same Arc).
        assert!(Arc::ptr_eq(&tool1.connection, &tool2.connection));
    }

    #[test]
    fn create_mcp_tools_empty_servers_returns_empty() {
        // Can't use block_in_place outside tokio runtime, but empty servers
        // short-circuits before hitting async code.
        let result = create_mcp_tools(&HashMap::new()).expect("empty servers should succeed");
        assert!(result.is_empty());
    }
}
