use agentzero_common::url_policy::UrlAccessPolicy;
use agentzero_common::util::parse_http_url_with_policy;
use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use reqwest::Method;

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
        "http_request"
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
    use agentzero_common::url_policy::UrlAccessPolicy;
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
}
