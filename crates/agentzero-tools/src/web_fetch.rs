use agentzero_core::common::url_policy::UrlAccessPolicy;
use agentzero_core::common::util::parse_http_url_with_policy;
use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;

const DEFAULT_MAX_BYTES: usize = 64 * 1024;

pub struct WebFetchTool {
    client: reqwest::Client,
    max_bytes: usize,
    url_policy: UrlAccessPolicy,
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
            max_bytes: DEFAULT_MAX_BYTES,
            url_policy: UrlAccessPolicy::default(),
        }
    }
}

impl WebFetchTool {
    pub fn with_url_policy(mut self, policy: UrlAccessPolicy) -> Self {
        self.url_policy = policy;
        self
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &'static str {
        "web_fetch"
    }

    fn description(&self) -> &'static str {
        "Fetch a URL and return its content as text (HTML converted to plain text)."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to fetch" }
            },
            "required": ["url"]
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let url = input.trim();
        if url.is_empty() {
            return Err(anyhow!("url is required"));
        }

        let parsed = parse_http_url_with_policy(url, &self.url_policy)?;

        let response = self
            .client
            .get(parsed)
            .send()
            .await
            .context("web fetch request failed")?;

        let status = response.status().as_u16();
        let body = response.text().await.context("failed reading response")?;
        let output = if body.len() > self.max_bytes {
            format!(
                "status={status}\n{}\n<truncated at {} bytes>",
                &body[..self.max_bytes],
                self.max_bytes
            )
        } else {
            format!("status={status}\n{body}")
        };

        Ok(ToolResult { output })
    }
}

#[cfg(test)]
mod tests {
    use super::WebFetchTool;
    use agentzero_core::{Tool, ToolContext};

    #[tokio::test]
    async fn web_fetch_rejects_missing_url_negative_path() {
        let tool = WebFetchTool::default();
        let err = tool
            .execute("   ", &ToolContext::new(".".to_string()))
            .await
            .expect_err("missing url should fail");
        assert!(err.to_string().contains("url is required"));
    }

    #[tokio::test]
    async fn web_fetch_rejects_invalid_url_negative_path() {
        let tool = WebFetchTool::default();
        let err = tool
            .execute("not-a-url", &ToolContext::new(".".to_string()))
            .await
            .expect_err("invalid url should fail");
        assert!(err.to_string().contains("invalid url"));
    }

    #[tokio::test]
    async fn web_fetch_blocks_private_ip_negative_path() {
        let tool = WebFetchTool::default();
        let err = tool
            .execute(
                "http://10.0.0.1/internal",
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("private IP should be blocked");
        assert!(err.to_string().contains("URL access denied"));
    }

    #[tokio::test]
    async fn web_fetch_blocks_blocklisted_domain_negative_path() {
        use agentzero_core::common::url_policy::UrlAccessPolicy;
        let tool = WebFetchTool::default().with_url_policy(UrlAccessPolicy {
            domain_blocklist: vec!["evil.example".to_string()],
            ..Default::default()
        });
        let err = tool
            .execute(
                "https://evil.example/phish",
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("blocklisted domain should be blocked");
        assert!(err.to_string().contains("URL access denied"));
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

    #[tokio::test]
    async fn web_fetch_success_returns_body() {
        use agentzero_core::common::url_policy::UrlAccessPolicy;
        let (handle, port) =
            start_mock_server("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello").await;
        let tool = WebFetchTool::default().with_url_policy(UrlAccessPolicy {
            allow_loopback: true,
            ..Default::default()
        });
        let result = tool
            .execute(
                &format!("http://127.0.0.1:{port}/"),
                &ToolContext::new("/tmp".to_string()),
            )
            .await
            .expect("request should succeed");
        assert!(
            result.output.contains("hello"),
            "body missing: {}",
            result.output
        );
        assert!(
            result.output.contains("status=200"),
            "status missing: {}",
            result.output
        );
        handle.abort();
    }

    #[tokio::test]
    async fn web_fetch_404_returns_status() {
        use agentzero_core::common::url_policy::UrlAccessPolicy;
        let (handle, port) =
            start_mock_server("HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nnot found").await;
        let tool = WebFetchTool::default().with_url_policy(UrlAccessPolicy {
            allow_loopback: true,
            ..Default::default()
        });
        let result = tool
            .execute(
                &format!("http://127.0.0.1:{port}/missing"),
                &ToolContext::new("/tmp".to_string()),
            )
            .await
            .expect("request should succeed even on 404");
        assert!(
            result.output.contains("status=404"),
            "status missing: {}",
            result.output
        );
        handle.abort();
    }

    #[tokio::test]
    async fn web_fetch_truncates_large_response() {
        use agentzero_core::common::url_policy::UrlAccessPolicy;
        // Build a response larger than 64KB
        let body_size = 70_000;
        let body: String = "A".repeat(body_size);
        let response_str = format!("HTTP/1.1 200 OK\r\nContent-Length: {body_size}\r\n\r\n{body}");
        // Leak the string so we get a &'static str for the mock server
        let response_static: &'static str = Box::leak(response_str.into_boxed_str());
        let (handle, port) = start_mock_server(response_static).await;
        let tool = WebFetchTool::default().with_url_policy(UrlAccessPolicy {
            allow_loopback: true,
            ..Default::default()
        });
        let result = tool
            .execute(
                &format!("http://127.0.0.1:{port}/big"),
                &ToolContext::new("/tmp".to_string()),
            )
            .await
            .expect("request should succeed");
        assert!(
            result.output.contains("truncated"),
            "truncation missing: {}",
            result.output
        );
        handle.abort();
    }
}
