//! Lightweight HTTP server that exposes host tools as REST endpoints for WASM
//! plugin guests. Each tool is available as `POST /tools/{name}` accepting
//! JSON input and returning JSON output.
//!
//! The server binds to `127.0.0.1:0` (OS-assigned port) and requires a
//! per-execution bearer token for authentication. It shuts down when the
//! provided cancellation token is dropped.
//!
//! WASM guests access these tools via generated shell shims that curl the
//! local server, avoiding the need for custom ABI host functions.

use agentzero_core::{Tool, ToolContext};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;

/// A running shim server instance.
pub struct ShimServer {
    /// The local address the server is listening on.
    pub addr: SocketAddr,
    /// The bearer token required for all requests.
    pub token: String,
    /// Send a signal to shut down the server.
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl ShimServer {
    /// Start a new shim server exposing the given tools.
    ///
    /// The server runs in a background tokio task and shuts down when the
    /// returned `ShimServer` is dropped or `shutdown()` is called.
    pub async fn start(tools: Vec<Arc<dyn Tool>>, ctx: ToolContext) -> anyhow::Result<Self> {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        // Generate a random bearer token.
        let token = format!("shim-{:016x}{:016x}", rand_u64(), rand_u64(),);

        let tool_map: HashMap<String, Arc<dyn Tool>> = tools
            .into_iter()
            .map(|t| (t.name().to_string(), t))
            .collect();

        let state = Arc::new(ShimState {
            tools: tool_map,
            ctx,
            token: token.clone(),
        });

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        tokio::spawn(run_server(listener, state, shutdown_rx));

        Ok(Self {
            addr,
            token,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// The port the server is listening on.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Explicitly shut down the server.
    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ShimServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

struct ShimState {
    tools: HashMap<String, Arc<dyn Tool>>,
    ctx: ToolContext,
    token: String,
}

/// Run the server loop until shutdown is signalled.
async fn run_server(
    listener: tokio::net::TcpListener,
    state: Arc<ShimState>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _)) => {
                        let state = Arc::clone(&state);
                        tokio::spawn(handle_connection(stream, state));
                    }
                    Err(_) => break,
                }
            }
            _ = &mut shutdown_rx => break,
        }
    }
}

/// Handle a single HTTP connection (one request per connection).
async fn handle_connection(stream: tokio::net::TcpStream, state: Arc<ShimState>) {
    use tokio::io::AsyncReadExt;

    let mut buf = vec![0u8; 65536];
    let mut stream = stream;
    let n = match stream.read(&mut buf).await {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the HTTP request minimally.
    let mut lines = request.lines();
    let request_line = match lines.next() {
        Some(l) => l,
        None => {
            let _ = write_response(&mut stream, 400, r#"{"error":"empty request"}"#).await;
            return;
        }
    };

    // Parse method and path.
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        let _ = write_response(&mut stream, 400, r#"{"error":"malformed request line"}"#).await;
        return;
    }
    let method = parts[0];
    let path = parts[1];

    // Check bearer token from headers.
    let mut authorized = false;
    for line in request.lines() {
        if let Some(value) = line.strip_prefix("Authorization: Bearer ") {
            if value.trim() == state.token {
                authorized = true;
            }
            break;
        }
    }
    if !authorized {
        let _ = write_response(&mut stream, 401, r#"{"error":"unauthorized"}"#).await;
        return;
    }

    if method != "POST" {
        let _ = write_response(&mut stream, 405, r#"{"error":"method not allowed"}"#).await;
        return;
    }

    // Extract tool name from path: /tools/{name}
    let tool_name = match path.strip_prefix("/tools/") {
        Some(name) if !name.is_empty() => name,
        _ => {
            let _ = write_response(&mut stream, 404, r#"{"error":"not found"}"#).await;
            return;
        }
    };

    let tool = match state.tools.get(tool_name) {
        Some(t) => Arc::clone(t),
        None => {
            let _ = write_response(
                &mut stream,
                404,
                &format!(r#"{{"error":"unknown tool: {tool_name}"}}"#),
            )
            .await;
            return;
        }
    };

    // Extract body (after the blank line).
    let body = request
        .split("\r\n\r\n")
        .nth(1)
        .or_else(|| request.split("\n\n").nth(1))
        .unwrap_or("");

    match tool.execute(body, &state.ctx).await {
        Ok(result) => {
            let json = serde_json::to_string(&result)
                .unwrap_or_else(|_| r#"{"output":"<serialization error>"}"#.to_string());
            let _ = write_response(&mut stream, 200, &json).await;
        }
        Err(e) => {
            let json = format!(r#"{{"error":"{}"}}"#, e.to_string().replace('"', "'"));
            let _ = write_response(&mut stream, 500, &json).await;
        }
    }
}

async fn write_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;

    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Unknown",
    };

    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await
}

/// Simple pseudo-random u64 using system time + process ID.
fn rand_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let pid = std::process::id() as u64;
    // xorshift-style mixing
    let mut x = ts ^ (pid << 32);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

/// Generate shell shim scripts for the given tools.
///
/// Each shim is a shell script that reads JSON from stdin and POSTs it to
/// the local shim server. Returns a map of `tool_name -> script_content`.
pub fn generate_shims(port: u16, token: &str, tool_names: &[String]) -> HashMap<String, String> {
    tool_names
        .iter()
        .map(|name| {
            let script = format!(
                "#!/bin/sh\n\
                 curl -s -X POST \"http://127.0.0.1:{port}/tools/{name}\" \\\n\
                   -H \"Authorization: Bearer {token}\" \\\n\
                   -H \"Content-Type: application/json\" \\\n\
                   --data @-\n"
            );
            (name.clone(), script)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::{ToolContext, ToolResult};
    use async_trait::async_trait;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn description(&self) -> &'static str {
            "echoes input back"
        }
        async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: format!("echoed: {input}"),
            })
        }
    }

    #[tokio::test]
    async fn shim_server_responds_to_tool_call() {
        let ctx = ToolContext::new("/tmp".to_string());
        let tool: Arc<dyn Tool> = Arc::new(EchoTool);
        let server = ShimServer::start(vec![tool], ctx)
            .await
            .expect("server should start");

        let port = server.port();
        let token = server.token.clone();

        // Make a request using tokio TCP.
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("should connect");

        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let body = r#"{"message":"hello"}"#;
        let request = format!(
            "POST /tools/echo HTTP/1.1\r\n\
             Host: 127.0.0.1:{port}\r\n\
             Authorization: Bearer {token}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.expect("write");

        let mut response = String::new();
        stream.read_to_string(&mut response).await.expect("read");

        assert!(response.contains("200 OK"), "response: {response}");
        assert!(response.contains("echoed:"), "response: {response}");

        server.shutdown();
    }

    #[tokio::test]
    async fn shim_server_rejects_bad_token() {
        let ctx = ToolContext::new("/tmp".to_string());
        let tool: Arc<dyn Tool> = Arc::new(EchoTool);
        let server = ShimServer::start(vec![tool], ctx)
            .await
            .expect("server should start");

        let port = server.port();

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("should connect");

        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let request = format!(
            "POST /tools/echo HTTP/1.1\r\n\
             Host: 127.0.0.1:{port}\r\n\
             Authorization: Bearer wrong-token\r\n\
             Content-Length: 2\r\n\
             \r\n\
             {{}}"
        );
        stream.write_all(request.as_bytes()).await.expect("write");

        let mut response = String::new();
        stream.read_to_string(&mut response).await.expect("read");

        assert!(response.contains("401"), "response: {response}");
        assert!(response.contains("unauthorized"), "response: {response}");

        server.shutdown();
    }

    #[tokio::test]
    async fn shim_server_returns_404_for_unknown_tool() {
        let ctx = ToolContext::new("/tmp".to_string());
        let tool: Arc<dyn Tool> = Arc::new(EchoTool);
        let server = ShimServer::start(vec![tool], ctx)
            .await
            .expect("server should start");

        let port = server.port();
        let token = server.token.clone();

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("should connect");

        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let request = format!(
            "POST /tools/nonexistent HTTP/1.1\r\n\
             Host: 127.0.0.1:{port}\r\n\
             Authorization: Bearer {token}\r\n\
             Content-Length: 2\r\n\
             \r\n\
             {{}}"
        );
        stream.write_all(request.as_bytes()).await.expect("write");

        let mut response = String::new();
        stream.read_to_string(&mut response).await.expect("read");

        assert!(response.contains("404"), "response: {response}");

        server.shutdown();
    }

    #[tokio::test]
    async fn shim_script_executes_tool_via_shell() {
        // End-to-end test: start server → generate shim → execute shim via sh → verify result.
        let ctx = ToolContext::new("/tmp".to_string());
        let tool: Arc<dyn Tool> = Arc::new(EchoTool);
        let server = ShimServer::start(vec![tool], ctx)
            .await
            .expect("server should start");

        let port = server.port();
        let token = server.token.clone();

        let shims = generate_shims(port, &token, &["echo".into()]);
        let shim_script = &shims["echo"];

        // Write the shim to a temp file.
        let shim_dir = std::env::temp_dir().join(format!(
            "az-shim-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&shim_dir).expect("create shim dir");
        let shim_path = shim_dir.join("echo");
        std::fs::write(&shim_path, shim_script).expect("write shim");

        // Execute the shim with JSON piped to stdin.
        let mut child = tokio::process::Command::new("sh")
            .arg(&shim_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("shim should spawn");

        {
            use tokio::io::AsyncWriteExt;
            let mut stdin = child.stdin.take().expect("stdin");
            stdin
                .write_all(br#"{"message":"hello from shim"}"#)
                .await
                .expect("write stdin");
            // Drop stdin to signal EOF.
        }

        let output = child
            .wait_with_output()
            .await
            .expect("shim should complete");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("echoed:"),
            "shim output should contain tool result, got: {stdout}"
        );
        assert!(
            stdout.contains("hello from shim"),
            "shim output should contain input, got: {stdout}"
        );

        server.shutdown();
        let _ = std::fs::remove_dir_all(shim_dir);
    }

    #[test]
    fn generate_shims_produces_valid_scripts() {
        let shims = generate_shims(12345, "tok-abc", &["read_file".into(), "shell".into()]);

        assert_eq!(shims.len(), 2);

        let rf = &shims["read_file"];
        assert!(rf.starts_with("#!/bin/sh"));
        assert!(rf.contains("127.0.0.1:12345"));
        assert!(rf.contains("/tools/read_file"));
        assert!(rf.contains("Bearer tok-abc"));
        assert!(rf.contains("--data @-"));

        let sh = &shims["shell"];
        assert!(sh.contains("/tools/shell"));
    }
}
