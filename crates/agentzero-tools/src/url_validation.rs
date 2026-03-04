use agentzero_core::common::url_policy::UrlAccessPolicy;
use agentzero_core::common::util::parse_http_url_with_policy;
use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::anyhow;
use async_trait::async_trait;

#[derive(Default)]
pub struct UrlValidationTool {
    url_policy: UrlAccessPolicy,
}

impl UrlValidationTool {
    pub fn with_url_policy(mut self, policy: UrlAccessPolicy) -> Self {
        self.url_policy = policy;
        self
    }
}

#[async_trait]
impl Tool for UrlValidationTool {
    fn name(&self) -> &'static str {
        "url_validation"
    }

    fn description(&self) -> &'static str {
        "Validate a URL against the access policy (private IPs, domain allowlist, etc.)."
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("url input is required"));
        }

        let parsed = parse_http_url_with_policy(trimmed, &self.url_policy)?;

        Ok(ToolResult {
            output: parsed.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::UrlValidationTool;
    use agentzero_core::{Tool, ToolContext};

    #[tokio::test]
    async fn url_validation_accepts_https_success_path() {
        let tool = UrlValidationTool::default();
        let result = tool
            .execute(
                "https://example.com/path?q=1",
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect("validation should succeed");
        assert!(result.output.starts_with("https://example.com/path"));
    }

    #[tokio::test]
    async fn url_validation_rejects_unsupported_scheme_negative_path() {
        let tool = UrlValidationTool::default();
        let err = tool
            .execute("file:///tmp/test.txt", &ToolContext::new(".".to_string()))
            .await
            .expect_err("unsupported scheme should fail");
        assert!(err.to_string().contains("unsupported url scheme"));
    }

    #[tokio::test]
    async fn url_validation_blocks_private_ip_negative_path() {
        let tool = UrlValidationTool::default();
        let err = tool
            .execute(
                "http://172.16.0.1/admin",
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("private IP should be blocked");
        assert!(err.to_string().contains("URL access denied"));
    }
}
