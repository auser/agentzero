use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};
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

pub struct McpTool {
    servers: HashMap<String, McpServerConfig>,
    timeout_ms: u64,
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

        Ok(Self {
            servers,
            timeout_ms: DEFAULT_MCP_TIMEOUT_MS,
        })
    }

    async fn call_server(
        &self,
        server: &McpServerConfig,
        tool_name: &str,
        arguments: Value,
    ) -> anyhow::Result<String> {
        let mut child = Command::new(&server.command)
            .args(&server.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn mcp server command: {} {}",
                    server.command,
                    server.args.join(" ")
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

        let mut stdin = child_stdin;
        let mut stdout = BufReader::new(child_stdout);

        self.initialize_session(&mut stdin, &mut stdout).await?;

        let listed_tools = self.list_tools(&mut stdin, &mut stdout).await?;
        if !listed_tools.iter().any(|name| name == tool_name) {
            return Err(anyhow!(
                "mcp tool `{tool_name}` not exposed by selected server"
            ));
        }

        let result = self
            .call_tool(&mut stdin, &mut stdout, tool_name, arguments)
            .await?;

        let _ = child.kill().await;
        Ok(result)
    }

    async fn initialize_session(
        &self,
        stdin: &mut ChildStdin,
        stdout: &mut BufReader<ChildStdout>,
    ) -> anyhow::Result<()> {
        let init = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-05",
                "capabilities": {},
                "clientInfo": {"name": "agentzero", "version": "0.1.0"}
            }
        });
        write_message(stdin, &init).await?;
        read_response_for_id(stdout, 1, self.timeout_ms).await?;

        let initialized = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        write_message(stdin, &initialized).await?;

        Ok(())
    }

    async fn list_tools(
        &self,
        stdin: &mut ChildStdin,
        stdout: &mut BufReader<ChildStdout>,
    ) -> anyhow::Result<Vec<String>> {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        write_message(stdin, &req).await?;
        let result = read_response_for_id(stdout, 2, self.timeout_ms).await?;

        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("invalid tools/list result shape"))?;

        Ok(tools
            .iter()
            .filter_map(|tool| {
                tool.get("name")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .collect())
    }

    async fn call_tool(
        &self,
        stdin: &mut ChildStdin,
        stdout: &mut BufReader<ChildStdout>,
        name: &str,
        arguments: Value,
    ) -> anyhow::Result<String> {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            }
        });
        write_message(stdin, &req).await?;
        let result = read_response_for_id(stdout, 3, self.timeout_ms).await?;

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

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed = McpCallInput::parse(input)?;
        let server = self
            .servers
            .get(&parsed.server)
            .ok_or_else(|| anyhow!("unknown mcp server `{}`", parsed.server))?;

        let output = self
            .call_server(server, &parsed.tool, parsed.arguments)
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
}
