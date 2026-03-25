use agentzero_core::common::url_policy::UrlAccessPolicy;
use agentzero_core::common::util::parse_http_url_with_policy;
use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use reqwest::Method;
use serde::Deserialize;

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct HttpRequestInput {
    /// HTTP method (GET, POST, PUT, DELETE)
    method: String,
    /// The URL to request
    url: String,
    /// Optional request body
    #[serde(default)]
    body: Option<String>,
}

#[tool(
    name = "http_request",
    description = "Send an HTTP request (GET, POST, PUT, DELETE) and return the response. Input format: \"METHOD URL [BODY]\"."
)]
pub struct HttpRequestTool {
    client: reqwest::Client,
    url_policy: UrlAccessPolicy,
}

impl Default for HttpRequestTool {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
            url_policy: UrlAccessPolicy::default(),
        }
    }
}

impl HttpRequestTool {
    pub fn with_url_policy(mut self, policy: UrlAccessPolicy) -> Self {
        self.url_policy = policy;
        self
    }
}

#[async_trait]
impl Tool for HttpRequestTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(HttpRequestInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let mut parts = input.trim().splitn(3, ' ');
        let method = parts.next().unwrap_or_default().to_ascii_uppercase();
        let url = parts.next().unwrap_or_default();
        let body = parts.next();

        if method.is_empty() || url.is_empty() {
            return Err(anyhow!(
                "usage: <METHOD> <URL> [JSON_BODY], e.g. `GET https://example.com`"
            ));
        }
        let method = Method::from_bytes(method.as_bytes())
            .with_context(|| format!("invalid method `{method}`"))?;
        let parsed = parse_http_url_with_policy(url, &self.url_policy)?;

        let mut request = self.client.request(method, parsed);
        if let Some(body) = body {
            let json_value: serde_json::Value =
                serde_json::from_str(body).context("body must be valid JSON when provided")?;
            request = request.json(&json_value);
        }

        let response = request.send().await.context("request failed")?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .await
            .context("failed to read response body")?;
        Ok(ToolResult {
            output: format!("status={status}\n{text}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::HttpRequestTool;
    use agentzero_core::common::url_policy::UrlAccessPolicy;
    use agentzero_core::{Tool, ToolContext};

    #[tokio::test]
    async fn http_request_rejects_invalid_usage_negative_path() {
        let tool = HttpRequestTool::default();
        let err = tool
            .execute("", &ToolContext::new(".".to_string()))
            .await
            .expect_err("empty input should fail");
        assert!(err.to_string().contains("usage:"));
    }

    #[tokio::test]
    async fn http_request_rejects_non_http_scheme_negative_path() {
        let tool = HttpRequestTool::default();
        let err = tool
            .execute("GET ftp://example.com", &ToolContext::new(".".to_string()))
            .await
            .expect_err("non-http scheme should fail");
        assert!(err.to_string().contains("unsupported url scheme"));
    }

    #[tokio::test]
    async fn http_request_blocks_private_ip_negative_path() {
        let tool = HttpRequestTool::default();
        let err = tool
            .execute(
                "GET http://192.168.1.1/api",
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("private IP should be blocked");
        assert!(err.to_string().contains("URL access denied"));
    }

    #[tokio::test]
    async fn http_request_blocks_loopback_negative_path() {
        let tool = HttpRequestTool::default();
        let err = tool
            .execute(
                "GET http://127.0.0.1:8080/api",
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("loopback should be blocked");
        assert!(err.to_string().contains("URL access denied"));
    }

    #[tokio::test]
    async fn http_request_allows_loopback_when_configured() {
        let tool = HttpRequestTool::default().with_url_policy(UrlAccessPolicy {
            allow_loopback: true,
            ..Default::default()
        });
        // Won't succeed (no server), but should NOT fail with policy error
        let err = tool
            .execute(
                "GET http://127.0.0.1:19999/api",
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("connection should fail (no server)");
        // Should fail with connection error, NOT policy error
        assert!(!err.to_string().contains("URL access denied"));
    }

    // ---- Mock HTTP server tests ----

    async fn start_mock_server(response: &'static str) -> (tokio::task::JoinHandle<()>, u16) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        (handle, port)
    }

    /// Echo server: reads the full request, extracts the body after \r\n\r\n, and echoes it back.
    async fn start_echo_server() -> (tokio::task::JoinHandle<()>, u16) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 8192];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let body = request
                    .find("\r\n\r\n")
                    .map(|i| &request[i + 4..])
                    .unwrap_or("")
                    .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body,
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        (handle, port)
    }

    #[tokio::test]
    async fn http_request_get_success() {
        let (handle, port) =
            start_mock_server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await;
        let tool = HttpRequestTool::default().with_url_policy(UrlAccessPolicy {
            allow_loopback: true,
            ..Default::default()
        });
        let result = tool
            .execute(
                &format!("GET http://127.0.0.1:{port}"),
                &ToolContext::new("/tmp".to_string()),
            )
            .await
            .expect("GET should succeed");
        assert!(
            result.output.contains("status=200"),
            "status missing: {}",
            result.output
        );
        assert!(
            result.output.contains("ok"),
            "body missing: {}",
            result.output
        );
        handle.abort();
    }

    #[tokio::test]
    async fn http_request_post_with_body() {
        let (handle, port) = start_echo_server().await;
        let tool = HttpRequestTool::default().with_url_policy(UrlAccessPolicy {
            allow_loopback: true,
            ..Default::default()
        });
        let result = tool
            .execute(
                &format!("POST http://127.0.0.1:{port} {{\"key\":\"value\"}}"),
                &ToolContext::new("/tmp".to_string()),
            )
            .await
            .expect("POST should succeed");
        assert!(
            result.output.contains("status=200"),
            "status missing: {}",
            result.output
        );
        assert!(
            result.output.contains("key"),
            "echoed body missing: {}",
            result.output
        );
        assert!(
            result.output.contains("value"),
            "echoed body missing: {}",
            result.output
        );
        handle.abort();
    }

    #[tokio::test]
    async fn http_request_server_error_returns_500() {
        let (handle, port) = start_mock_server(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 5\r\n\r\nerror",
        )
        .await;
        let tool = HttpRequestTool::default().with_url_policy(UrlAccessPolicy {
            allow_loopback: true,
            ..Default::default()
        });
        let result = tool
            .execute(
                &format!("GET http://127.0.0.1:{port}"),
                &ToolContext::new("/tmp".to_string()),
            )
            .await
            .expect("request should succeed even on 500");
        assert!(
            result.output.contains("status=500"),
            "status missing: {}",
            result.output
        );
        handle.abort();
    }
}
