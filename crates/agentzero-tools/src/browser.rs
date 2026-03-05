use agentzero_core::common::url_policy::UrlAccessPolicy;
use agentzero_core::common::util::parse_http_url_with_policy;
use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

const MAX_OUTPUT_BYTES: usize = 65536;
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Deserialize)]
#[serde(tag = "action")]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
enum BrowserAction {
    Navigate {
        url: String,
    },
    Snapshot,
    Click {
        selector: String,
    },
    Fill {
        selector: String,
        value: String,
    },
    Type {
        selector: String,
        text: String,
    },
    GetText {
        selector: String,
    },
    GetTitle,
    GetUrl,
    Screenshot {
        #[serde(default)]
        path: Option<String>,
    },
    Wait {
        #[serde(default)]
        selector: Option<String>,
        #[serde(default)]
        ms: Option<u64>,
    },
    Press {
        key: String,
    },
    Hover {
        selector: String,
    },
    Scroll {
        direction: String,
    },
    Close,
}

#[derive(Debug, Clone)]
pub struct BrowserConfig {
    pub agent_browser_command: String,
    pub agent_browser_extra_args: Vec<String>,
    pub timeout_ms: u64,
    pub allowed_domains: Vec<String>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            agent_browser_command: "agent-browser".to_string(),
            agent_browser_extra_args: Vec::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            allowed_domains: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct BrowserTool {
    config: BrowserConfig,
    url_policy: UrlAccessPolicy,
}

impl BrowserTool {
    pub fn new(config: BrowserConfig) -> Self {
        Self {
            config,
            url_policy: UrlAccessPolicy::default(),
        }
    }

    pub fn with_url_policy(mut self, policy: UrlAccessPolicy) -> Self {
        self.url_policy = policy;
        self
    }

    fn validate_selector(selector: &str) -> anyhow::Result<()> {
        if selector.trim().is_empty() {
            return Err(anyhow!("selector must not be empty"));
        }
        Ok(())
    }

    fn validate_domain(&self, url: &str) -> anyhow::Result<()> {
        if self.config.allowed_domains.is_empty() {
            return Ok(());
        }
        let parsed = url::Url::parse(url).context("invalid URL")?;
        let host = parsed.host_str().unwrap_or("");
        if !self
            .config
            .allowed_domains
            .iter()
            .any(|d| host == d || host.ends_with(&format!(".{d}")))
        {
            return Err(anyhow!(
                "domain {} is not in the allowed domains list",
                host
            ));
        }
        Ok(())
    }

    async fn send_to_agent_browser(&self, action_json: &str) -> anyhow::Result<String> {
        let mut cmd = Command::new(&self.config.agent_browser_command);
        cmd.args(&self.config.agent_browser_extra_args);
        cmd.arg("--action").arg(action_json);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "failed to spawn agent-browser command: {}",
                self.config.agent_browser_command
            )
        })?;

        let stdout_handle = child
            .stdout
            .take()
            .context("stdout not piped on spawned child")?;
        let stderr_handle = child
            .stderr
            .take()
            .context("stderr not piped on spawned child")?;

        let stdout_task = tokio::spawn(read_limited(stdout_handle));
        let stderr_task = tokio::spawn(read_limited(stderr_handle));

        let timeout = tokio::time::Duration::from_millis(self.config.timeout_ms);
        let status = tokio::time::timeout(timeout, child.wait())
            .await
            .context("agent-browser timed out")?
            .context("agent-browser command failed")?;

        let stdout = stdout_task.await.context("stdout join")??;
        let stderr = stderr_task.await.context("stderr join")??;

        let mut output = format!("exit={}\n", status.code().unwrap_or(-1));
        if !stdout.is_empty() {
            output.push_str(&stdout);
        }
        if !stderr.is_empty() {
            output.push_str("\nstderr:\n");
            output.push_str(&stderr);
        }
        Ok(output)
    }
}

async fn read_limited<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> anyhow::Result<String> {
    let mut buf = Vec::new();
    let mut limited = (&mut reader).take((MAX_OUTPUT_BYTES + 1) as u64);
    limited.read_to_end(&mut buf).await?;
    let truncated = buf.len() > MAX_OUTPUT_BYTES;
    if truncated {
        buf.truncate(MAX_OUTPUT_BYTES);
    }
    let mut s = String::from_utf8_lossy(&buf).to_string();
    if truncated {
        s.push_str(&format!("\n<truncated at {} bytes>", MAX_OUTPUT_BYTES));
    }
    Ok(s)
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &'static str {
        "browser"
    }

    fn description(&self) -> &'static str {
        "Control a headless browser: navigate to URLs, execute JavaScript, take screenshots, and extract page content."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Browser action: navigate, execute_js, screenshot, content, close",
                    "enum": ["navigate", "execute_js", "screenshot", "content", "close"]
                },
                "url": { "type": "string", "description": "URL to navigate to (for navigate action)" },
                "script": { "type": "string", "description": "JavaScript to execute (for execute_js action)" }
            },
            "required": ["action"]
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let action: BrowserAction =
            serde_json::from_str(input).context("browser expects JSON with \"action\" field")?;

        match &action {
            BrowserAction::Navigate { url } => {
                parse_http_url_with_policy(url, &self.url_policy)?;
                self.validate_domain(url)?;
            }
            BrowserAction::Click { selector }
            | BrowserAction::Fill { selector, .. }
            | BrowserAction::Type { selector, .. }
            | BrowserAction::GetText { selector }
            | BrowserAction::Hover { selector } => {
                Self::validate_selector(selector)?;
            }
            _ => {}
        }

        let output = self.send_to_agent_browser(input).await?;
        Ok(ToolResult { output })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn browser_rejects_invalid_json() {
        let tool = BrowserTool::default();
        let err = tool
            .execute("not json", &ToolContext::new(".".to_string()))
            .await
            .expect_err("invalid JSON should fail");
        assert!(err.to_string().contains("browser expects JSON"));
    }

    #[tokio::test]
    async fn browser_navigate_blocks_private_ip() {
        let tool = BrowserTool::default();
        let err = tool
            .execute(
                r#"{"action": "navigate", "url": "http://10.0.0.1/internal"}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("private IP should be blocked");
        assert!(err.to_string().contains("URL access denied"));
    }

    #[tokio::test]
    async fn browser_navigate_blocks_unapproved_domain() {
        let tool = BrowserTool::new(BrowserConfig {
            allowed_domains: vec!["example.com".to_string()],
            ..Default::default()
        });
        let err = tool
            .execute(
                r#"{"action": "navigate", "url": "https://evil.example.org/page"}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("non-allowed domain should be blocked");
        assert!(err.to_string().contains("not in the allowed domains"));
    }

    #[tokio::test]
    async fn browser_click_rejects_empty_selector() {
        let tool = BrowserTool::default();
        let err = tool
            .execute(
                r#"{"action": "click", "selector": ""}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("empty selector should fail");
        assert!(err.to_string().contains("selector must not be empty"));
    }

    #[test]
    fn validate_domain_allows_any_when_empty() {
        let tool = BrowserTool::default();
        assert!(tool.validate_domain("https://anything.com").is_ok());
    }

    #[test]
    fn validate_domain_allows_subdomain() {
        let tool = BrowserTool::new(BrowserConfig {
            allowed_domains: vec!["example.com".to_string()],
            ..Default::default()
        });
        assert!(tool.validate_domain("https://sub.example.com/page").is_ok());
        assert!(tool.validate_domain("https://example.com").is_ok());
    }
}
