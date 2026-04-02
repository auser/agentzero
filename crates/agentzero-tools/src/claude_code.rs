//! Claude Code delegation tool — invokes the `claude` CLI as a subprocess.

use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 131_072;

#[derive(Debug, Clone)]
pub struct ClaudeCodeConfig {
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub workspace_root: Option<String>,
}

impl Default for ClaudeCodeConfig {
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
    task: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    max_turns: Option<u32>,
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
}

#[tool(
    name = "claude_code",
    description = "Delegate a task to Claude Code (the `claude` CLI). Runs as an independent agent with access to the filesystem, shell, and other tools."
)]
pub struct ClaudeCodeTool {
    config: ClaudeCodeConfig,
}

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct ClaudeCodeSchema {
    /// The task or prompt to delegate to Claude Code
    task: String,
    /// Optional model override
    #[serde(default)]
    model: Option<String>,
    /// Maximum number of agentic turns
    #[serde(default)]
    max_turns: Option<i64>,
    /// Tools to allow
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
}

impl ClaudeCodeTool {
    pub fn new(config: ClaudeCodeConfig) -> Self {
        Self { config }
    }
}

impl Default for ClaudeCodeTool {
    fn default() -> Self {
        Self::new(ClaudeCodeConfig::default())
    }
}

#[async_trait]
impl Tool for ClaudeCodeTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(ClaudeCodeSchema::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input = serde_json::from_str(input)
            .map_err(|e| anyhow::anyhow!("claude_code expects JSON: {{\"task\": \"...\"}}: {e}"))?;

        if parsed.task.trim().is_empty() {
            return Err(anyhow::anyhow!("task must not be empty"));
        }

        let claude_path = which_claude().await?;

        let mut cmd = Command::new(&claude_path);
        cmd.arg("--print")
            .arg("--output-format")
            .arg("text")
            .arg("--prompt")
            .arg(&parsed.task)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let cwd = self
            .config
            .workspace_root
            .as_deref()
            .unwrap_or(&ctx.workspace_root);
        cmd.current_dir(cwd);

        if let Some(ref model) = parsed.model {
            cmd.arg("--model").arg(model);
        }
        if let Some(max_turns) = parsed.max_turns {
            cmd.arg("--max-turns").arg(max_turns.to_string());
        }
        if let Some(ref tools) = parsed.allowed_tools {
            for tool_name in tools {
                cmd.arg("--allowedTools").arg(tool_name);
            }
        }

        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn claude CLI: {e}"))?;

        let timeout_duration = self.config.timeout;
        let max_output = self.config.max_output_bytes;

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
            Ok(Err(e)) => Err(anyhow::anyhow!("claude CLI execution failed: {e}")),
            Err(_) => Err(anyhow::anyhow!(
                "claude CLI timed out after {} seconds",
                timeout_duration.as_secs()
            )),
        }
    }
}

async fn which_claude() -> anyhow::Result<String> {
    for name in &["claude"] {
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
    Err(anyhow::anyhow!("claude CLI not found in PATH"))
}

fn truncate_output(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let truncated = &s[..max_bytes];
        format!("{truncated}\n[output truncated at {max_bytes} bytes]")
    }
}
