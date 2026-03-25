//! OpenCode CLI delegation tool — invokes the `opencode` CLI as a subprocess
//! for two-tier agent delegation.
//!
//! AgentZero can delegate complex coding tasks to OpenCode CLI, which runs
//! as an independent agent with its own tool set and context.

use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 65_536; // 64 KiB

/// Environment variables to strip before spawning CLI subprocesses.
const BLOCKED_ENV_PREFIXES: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "AWS_SECRET_ACCESS_KEY",
    "AZURE_OPENAI_API_KEY",
    "MISTRAL_API_KEY",
    "COHERE_API_KEY",
    "GROQ_API_KEY",
    "DEEPSEEK_API_KEY",
    "TOGETHER_API_KEY",
    "FIREWORKS_API_KEY",
];

fn sanitized_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(k, _)| !BLOCKED_ENV_PREFIXES.iter().any(|prefix| k.contains(prefix)))
        .collect()
}

/// Configuration for the OpenCode CLI delegation tool.
#[derive(Debug, Clone)]
pub struct OpenCodeCliConfig {
    /// Maximum time to wait for the `opencode` process (default: 300s).
    pub timeout: Duration,
    /// Maximum output bytes to capture (default: 64 KiB).
    pub max_output_bytes: usize,
    /// If set, constrains `opencode` to this working directory.
    pub workspace_root: Option<String>,
}

impl Default for OpenCodeCliConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            workspace_root: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Input {
    /// The task/prompt to send to OpenCode CLI.
    task: String,
    /// Optional timeout override in seconds.
    #[serde(default)]
    timeout_secs: Option<u64>,
    /// Optional max output bytes override.
    #[serde(default)]
    max_output_bytes: Option<usize>,
}

/// Tool that delegates tasks to the `opencode` CLI.
///
/// Runs `opencode "{task}"` as a subprocess, captures output,
/// and returns it to the calling agent.
#[tool(
    name = "opencode_cli",
    description = "Delegate a task to OpenCode CLI. Runs as an independent agent with filesystem and shell access."
)]
pub struct OpenCodeCliTool {
    config: OpenCodeCliConfig,
}

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct OpenCodeCliSchema {
    /// The task or prompt to delegate to OpenCode CLI
    task: String,
    /// Optional timeout in seconds (default: 300)
    #[serde(default)]
    timeout_secs: Option<i64>,
    /// Optional maximum output bytes (default: 65536)
    #[serde(default)]
    max_output_bytes: Option<i64>,
}

impl OpenCodeCliTool {
    pub fn new(config: OpenCodeCliConfig) -> Self {
        Self { config }
    }
}

impl Default for OpenCodeCliTool {
    fn default() -> Self {
        Self::new(OpenCodeCliConfig::default())
    }
}

#[async_trait]
impl Tool for OpenCodeCliTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(OpenCodeCliSchema::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input = serde_json::from_str(input).map_err(|e| {
            anyhow::anyhow!("opencode_cli expects JSON: {{\"task\": \"...\"}}: {e}")
        })?;

        if parsed.task.trim().is_empty() {
            return Err(anyhow::anyhow!("task must not be empty"));
        }

        // Check that `opencode` is available.
        let opencode_path = which_opencode().await?;

        let mut cmd = Command::new(&opencode_path);
        cmd.arg(&parsed.task)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Sanitize environment.
        cmd.env_clear().envs(sanitized_env());

        // Set working directory.
        let cwd = self
            .config
            .workspace_root
            .as_deref()
            .unwrap_or(&ctx.workspace_root);
        cmd.current_dir(cwd);

        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn opencode CLI: {e}"))?;

        // Read output with timeout.
        let timeout_duration = parsed
            .timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(self.config.timeout);
        let max_output = parsed
            .max_output_bytes
            .unwrap_or(self.config.max_output_bytes);

        match tokio::time::timeout(timeout_duration, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                let stdout_truncated = truncate_output(&stdout, max_output);
                let stderr_str = stderr.trim();

                let mut response = format!("status={}\n", output.status);
                if !stdout_truncated.is_empty() {
                    response.push_str(&stdout_truncated);
                }
                if !stderr_str.is_empty() {
                    response.push_str(&format!("\n[stderr] {stderr_str}"));
                }

                Ok(ToolResult { output: response })
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("opencode CLI execution failed: {e}")),
            Err(_) => {
                // Timeout — the child process is dropped and killed automatically.
                Err(anyhow::anyhow!(
                    "opencode CLI timed out after {} seconds",
                    timeout_duration.as_secs()
                ))
            }
        }
    }
}

/// Find the `opencode` binary in PATH.
async fn which_opencode() -> anyhow::Result<String> {
    for name in &["opencode"] {
        let output = Command::new("which").arg(name).output().await;
        if let Ok(out) = output {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(path);
                }
            }
        }
    }
    Err(anyhow::anyhow!(
        "opencode CLI not found in PATH. Install it from https://github.com/opencode-ai/opencode"
    ))
}

fn truncate_output(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let truncated = &s[..max_bytes];
        format!("{truncated}\n[output truncated at {max_bytes} bytes]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;

    fn ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    #[tokio::test]
    async fn input_schema_is_valid_json() {
        let tool = OpenCodeCliTool::default();
        let schema = tool.input_schema().expect("should have schema");
        assert_eq!(schema["required"][0], "task");
        assert!(schema["properties"]["task"].is_object());
        assert!(schema["properties"]["timeout_secs"].is_object());
        assert!(schema["properties"]["max_output_bytes"].is_object());
    }

    #[tokio::test]
    async fn empty_task_returns_error() {
        let tool = OpenCodeCliTool::default();
        let err = tool
            .execute(r#"{"task": ""}"#, &ctx())
            .await
            .expect_err("empty task should fail");
        assert!(err.to_string().contains("task must not be empty"));
    }

    #[tokio::test]
    async fn invalid_json_returns_error() {
        let tool = OpenCodeCliTool::default();
        let err = tool
            .execute("not json", &ctx())
            .await
            .expect_err("invalid JSON should fail");
        assert!(err.to_string().contains("opencode_cli expects JSON"));
    }

    #[test]
    fn truncate_output_short_unchanged() {
        let s = "hello world";
        assert_eq!(truncate_output(s, 100), "hello world");
    }

    #[test]
    fn truncate_output_long_truncates() {
        let s = "a".repeat(200);
        let result = truncate_output(&s, 50);
        assert!(result.contains("[output truncated at 50 bytes]"));
        assert!(result.len() < 200);
    }

    #[test]
    fn default_config_values() {
        let config = OpenCodeCliConfig::default();
        assert_eq!(config.timeout.as_secs(), 300);
        assert_eq!(config.max_output_bytes, 65_536);
        assert!(config.workspace_root.is_none());
    }
}
