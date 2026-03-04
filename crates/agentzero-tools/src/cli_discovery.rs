use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::Context;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct Input {
    op: String,
    #[serde(default)]
    command: Option<String>,
}

/// Tool that discovers available CLI tools and capabilities at runtime.
///
/// Operations:
/// - `check_command`: Check if a shell command is available on PATH
/// - `runtime_info`: Return runtime environment information
#[derive(Debug, Default, Clone, Copy)]
pub struct CliDiscoveryTool;

#[async_trait]
impl Tool for CliDiscoveryTool {
    fn name(&self) -> &'static str {
        "cli_discovery"
    }

    fn description(&self) -> &'static str {
        "Discover available CLI tools and runtime environment: check command availability or get runtime info."
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input =
            serde_json::from_str(input).context("cli_discovery expects JSON: {\"op\", ...}")?;

        let output = match parsed.op.as_str() {
            "check_command" => {
                let cmd = parsed
                    .command
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("check_command requires a `command` field"))?;

                if cmd.trim().is_empty() {
                    return Err(anyhow::anyhow!("command must not be empty"));
                }

                let available = which_exists(cmd).await;
                json!({
                    "command": cmd,
                    "available": available,
                })
                .to_string()
            }
            "runtime_info" => json!({
                "workspace_root": ctx.workspace_root,
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "family": std::env::consts::FAMILY,
            })
            .to_string(),
            other => json!({ "error": format!("unknown op: {other}") }).to_string(),
        };

        Ok(ToolResult { output })
    }
}

async fn which_exists(command: &str) -> bool {
    tokio::process::Command::new("which")
        .arg(command)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;

    fn test_ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    #[tokio::test]
    async fn check_command_finds_sh() {
        let tool = CliDiscoveryTool;
        let result = tool
            .execute(r#"{"op": "check_command", "command": "sh"}"#, &test_ctx())
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["command"], "sh");
        assert_eq!(v["available"], true);
    }

    #[tokio::test]
    async fn check_command_missing_binary() {
        let tool = CliDiscoveryTool;
        let result = tool
            .execute(
                r#"{"op": "check_command", "command": "nonexistent_binary_xyz_123"}"#,
                &test_ctx(),
            )
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["available"], false);
    }

    #[tokio::test]
    async fn runtime_info_returns_os() {
        let tool = CliDiscoveryTool;
        let result = tool
            .execute(r#"{"op": "runtime_info"}"#, &test_ctx())
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert!(v["os"].as_str().is_some());
        assert!(v["arch"].as_str().is_some());
        assert_eq!(v["workspace_root"], "/tmp");
    }

    #[tokio::test]
    async fn unknown_op_returns_error() {
        let tool = CliDiscoveryTool;
        let result = tool
            .execute(r#"{"op": "bad_op"}"#, &test_ctx())
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert!(v["error"].as_str().unwrap().contains("unknown op"));
    }

    #[tokio::test]
    async fn check_command_empty_fails() {
        let tool = CliDiscoveryTool;
        let err = tool
            .execute(r#"{"op": "check_command", "command": ""}"#, &test_ctx())
            .await
            .expect_err("empty command should fail");
        assert!(err.to_string().contains("command must not be empty"));
    }
}
