use agentzero_common::url_policy::UrlAccessPolicy;
use agentzero_common::util::parse_http_url_with_policy;
use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct BrowserOpenInput {
    url: String,
}

#[derive(Default)]
pub struct BrowserOpenTool {
    url_policy: UrlAccessPolicy,
}

impl BrowserOpenTool {
    pub fn with_url_policy(mut self, policy: UrlAccessPolicy) -> Self {
        self.url_policy = policy;
        self
    }

    fn open_command() -> &'static str {
        if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(target_os = "windows") {
            "cmd"
        } else {
            "xdg-open"
        }
    }

    fn open_args(url: &str) -> Vec<String> {
        if cfg!(target_os = "windows") {
            vec![
                "/C".to_string(),
                "start".to_string(),
                String::new(),
                url.to_string(),
            ]
        } else {
            vec![url.to_string()]
        }
    }
}

#[async_trait]
impl Tool for BrowserOpenTool {
    fn name(&self) -> &'static str {
        "browser_open"
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: BrowserOpenInput =
            serde_json::from_str(input).context("browser_open expects JSON: {\"url\": \"...\"}")?;

        if req.url.trim().is_empty() {
            return Err(anyhow!("url must not be empty"));
        }

        let parsed = parse_http_url_with_policy(&req.url, &self.url_policy)?;

        let cmd = Self::open_command();
        let args = Self::open_args(parsed.as_str());

        let status = Command::new(cmd)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .with_context(|| format!("failed to run {cmd}"))?;

        if status.success() {
            Ok(ToolResult {
                output: format!("opened {} in default browser", parsed),
            })
        } else {
            Err(anyhow!(
                "browser_open failed with exit code {}",
                status.code().unwrap_or(-1)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn browser_open_rejects_empty_url() {
        let tool = BrowserOpenTool::default();
        let err = tool
            .execute(r#"{"url": ""}"#, &ToolContext::new(".".to_string()))
            .await
            .expect_err("empty url should fail");
        assert!(err.to_string().contains("url must not be empty"));
    }

    #[tokio::test]
    async fn browser_open_blocks_private_ip() {
        let tool = BrowserOpenTool::default();
        let err = tool
            .execute(
                r#"{"url": "http://10.0.0.1/internal"}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("private IP should be blocked");
        assert!(err.to_string().contains("URL access denied"));
    }

    #[tokio::test]
    async fn browser_open_blocks_blocklisted_domain() {
        let tool = BrowserOpenTool::default().with_url_policy(UrlAccessPolicy {
            domain_blocklist: vec!["evil.example".to_string()],
            ..Default::default()
        });
        let err = tool
            .execute(
                r#"{"url": "https://evil.example/phish"}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("blocklisted domain should be blocked");
        assert!(err.to_string().contains("URL access denied"));
    }

    #[test]
    fn open_command_returns_platform_binary() {
        let cmd = BrowserOpenTool::open_command();
        assert!(!cmd.is_empty());
    }
}
