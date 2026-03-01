use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct ScreenshotInput {
    #[serde(default = "default_filename")]
    filename: String,
}

fn default_filename() -> String {
    "screenshot.png".to_string()
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ScreenshotTool;

impl ScreenshotTool {
    fn screenshot_command() -> &'static str {
        if cfg!(target_os = "macos") {
            "screencapture"
        } else {
            "import"
        }
    }

    fn screenshot_args(output_path: &str) -> Vec<String> {
        if cfg!(target_os = "macos") {
            vec!["-x".to_string(), output_path.to_string()]
        } else {
            // ImageMagick's import tool
            vec![
                "-window".to_string(),
                "root".to_string(),
                output_path.to_string(),
            ]
        }
    }
}

#[async_trait]
impl Tool for ScreenshotTool {
    fn name(&self) -> &'static str {
        "screenshot"
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: ScreenshotInput = serde_json::from_str(input)
            .context("screenshot expects JSON: {\"filename\": \"...\"}")?;

        if req.filename.contains("..") || req.filename.starts_with('/') {
            return Err(anyhow!("invalid filename"));
        }

        let output_path = PathBuf::from(&ctx.workspace_root).join(&req.filename);
        let output_str = output_path.to_string_lossy().to_string();

        let cmd = Self::screenshot_command();
        let args = Self::screenshot_args(&output_str);

        let status = Command::new(cmd)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .with_context(|| format!("failed to run {cmd}"))?;

        if status.success() {
            Ok(ToolResult {
                output: format!("screenshot saved to {}", req.filename),
            })
        } else {
            Err(anyhow!(
                "screenshot failed with exit code {}",
                status.code().unwrap_or(-1)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn screenshot_rejects_path_traversal() {
        let tool = ScreenshotTool;
        let err = tool
            .execute(
                r#"{"filename": "../escape.png"}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("path traversal should fail");
        assert!(err.to_string().contains("invalid filename"));
    }

    #[tokio::test]
    async fn screenshot_rejects_absolute_path() {
        let tool = ScreenshotTool;
        let err = tool
            .execute(
                r#"{"filename": "/tmp/screenshot.png"}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("absolute path should fail");
        assert!(err.to_string().contains("invalid filename"));
    }

    #[test]
    fn screenshot_command_returns_platform_binary() {
        let cmd = ScreenshotTool::screenshot_command();
        assert!(!cmd.is_empty());
    }
}
