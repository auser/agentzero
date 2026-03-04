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
}
