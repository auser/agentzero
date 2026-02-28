use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

const DEFAULT_MAX_SHELL_ARGS: usize = 8;
const DEFAULT_MAX_ARG_LENGTH: usize = 128;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 8192;
const DEFAULT_FORBIDDEN_CHARS: &str = ";&|><$`\n\r";

#[derive(Debug, Clone)]
pub struct ShellPolicy {
    pub allowed_commands: Vec<String>,
    pub max_args: usize,
    pub max_arg_length: usize,
    pub max_output_bytes: usize,
    pub forbidden_chars: String,
}

impl ShellPolicy {
    pub fn default_with_commands(allowed_commands: Vec<String>) -> Self {
        Self {
            allowed_commands,
            max_args: DEFAULT_MAX_SHELL_ARGS,
            max_arg_length: DEFAULT_MAX_ARG_LENGTH,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            forbidden_chars: DEFAULT_FORBIDDEN_CHARS.to_string(),
        }
    }
}

pub struct ShellTool {
    policy: ShellPolicy,
}

impl ShellTool {
    pub fn new(policy: ShellPolicy) -> Self {
        Self { policy }
    }

    fn parse_invocation(input: &str) -> anyhow::Result<(String, Vec<String>)> {
        let mut parts = input.split_whitespace();
        let command_name = parts
            .next()
            .ok_or_else(|| anyhow!("command is required"))?
            .to_string();
        let args = parts.map(ToString::to_string).collect::<Vec<_>>();

        Ok((command_name, args))
    }

    fn validate_argument(policy: &ShellPolicy, arg: &str) -> anyhow::Result<()> {
        if arg.is_empty() {
            return Err(anyhow!("empty shell argument is not allowed"));
        }

        if arg.len() > policy.max_arg_length {
            return Err(anyhow!("shell argument exceeds max length"));
        }

        if arg.chars().any(|c| policy.forbidden_chars.contains(c)) {
            return Err(anyhow!(
                "shell argument contains forbidden shell metacharacters"
            ));
        }

        Ok(())
    }

    async fn read_limited<R>(mut reader: R, max_bytes: usize) -> anyhow::Result<(Vec<u8>, bool)>
    where
        R: AsyncRead + Unpin,
    {
        let mut bytes = Vec::new();
        let mut limited = (&mut reader).take((max_bytes + 1) as u64);
        limited
            .read_to_end(&mut bytes)
            .await
            .context("failed to capture command output")?;

        let truncated = bytes.len() > max_bytes;
        if truncated {
            bytes.truncate(max_bytes);
        }

        Ok((bytes, truncated))
    }

    fn render_stream(name: &str, bytes: &[u8], truncated: bool, max_bytes: usize) -> String {
        let mut out = format!("{name}:\n{}", String::from_utf8_lossy(bytes));
        if truncated {
            out.push_str(&format!("\n<truncated at {max_bytes} bytes>"));
        }
        out
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &'static str {
        "shell"
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // Deny by default: only explicit allowlisted commands can run.
        let (command_name, args) = Self::parse_invocation(input)?;
        if !self
            .policy
            .allowed_commands
            .iter()
            .any(|c| c == &command_name)
        {
            return Err(anyhow!("command is not in allowlist"));
        }
        if args.len() > self.policy.max_args {
            return Err(anyhow!("too many shell arguments"));
        }
        for arg in &args {
            Self::validate_argument(&self.policy, arg)?;
        }

        let mut child = Command::new(&command_name)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("shell command failed to execute")?;

        let stdout_reader = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("shell command did not provide stdout pipe"))?;
        let stderr_reader = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("shell command did not provide stderr pipe"))?;

        let stdout_task = tokio::spawn(Self::read_limited(
            stdout_reader,
            self.policy.max_output_bytes,
        ));
        let stderr_task = tokio::spawn(Self::read_limited(
            stderr_reader,
            self.policy.max_output_bytes,
        ));

        let status = child.wait().await.context("shell command failed to run")?;
        let (stdout, stdout_truncated) = stdout_task
            .await
            .context("failed joining stdout capture task")??;
        let (stderr, stderr_truncated) = stderr_task
            .await
            .context("failed joining stderr capture task")??;

        Ok(ToolResult {
            output: format!(
                "status={}\n{}\n{}",
                status,
                Self::render_stream(
                    "stdout",
                    &stdout,
                    stdout_truncated,
                    self.policy.max_output_bytes
                ),
                Self::render_stream(
                    "stderr",
                    &stderr,
                    stderr_truncated,
                    self.policy.max_output_bytes
                )
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ShellPolicy, ShellTool};
    use agentzero_core::{Tool, ToolContext};

    #[tokio::test]
    async fn shell_allows_allowlisted_command() {
        let tool = ShellTool::new(ShellPolicy::default_with_commands(vec!["echo".to_string()]));
        let result = tool
            .execute(
                "echo hello",
                &ToolContext {
                    workspace_root: ".".to_string(),
                },
            )
            .await
            .expect("shell should succeed");

        assert!(result.output.contains("stdout:\nhello"));
    }

    #[tokio::test]
    async fn shell_rejects_forbidden_argument_chars() {
        let tool = ShellTool::new(ShellPolicy::default_with_commands(vec!["echo".to_string()]));
        let result = tool
            .execute(
                "echo hello;uname",
                &ToolContext {
                    workspace_root: ".".to_string(),
                },
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("metacharacters should be rejected")
            .to_string()
            .contains("forbidden shell metacharacters"));
    }

    #[tokio::test]
    async fn shell_rejects_non_allowlisted_command() {
        let tool = ShellTool::new(ShellPolicy::default_with_commands(vec!["echo".to_string()]));
        let result = tool
            .execute(
                "pwd",
                &ToolContext {
                    workspace_root: ".".to_string(),
                },
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("non-allowlisted command should be denied")
            .to_string()
            .contains("command is not in allowlist"));
    }

    #[tokio::test]
    async fn shell_truncates_stdout_to_policy_limit() {
        let mut policy = ShellPolicy::default_with_commands(vec!["echo".to_string()]);
        policy.max_output_bytes = 8;
        let tool = ShellTool::new(policy);

        let result = tool
            .execute(
                "echo 1234567890",
                &ToolContext {
                    workspace_root: ".".to_string(),
                },
            )
            .await
            .expect("shell should succeed");

        assert!(result.output.contains("stdout:\n12345678"));
        assert!(result.output.contains("<truncated at 8 bytes>"));
    }
}
