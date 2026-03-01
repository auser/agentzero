use crate::shell_parse::{self, AnnotatedChar, QuoteContext};
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

/// Context-aware shell command policy.
///
/// Replaces the flat `forbidden_chars` with structured classification that
/// respects quoting context.
#[derive(Debug, Clone)]
pub struct ShellCommandPolicy {
    /// Characters that are ALWAYS forbidden, even inside quotes.
    pub always_forbidden: Vec<char>,
    /// Characters forbidden only when they appear unquoted.
    pub forbidden_unquoted: Vec<char>,
}

impl Default for ShellCommandPolicy {
    fn default() -> Self {
        Self {
            always_forbidden: vec!['`', '\0'],
            forbidden_unquoted: vec![';', '&', '|', '>', '<', '$', '\n', '\r'],
        }
    }
}

impl ShellCommandPolicy {
    /// Build from the legacy flat `forbidden_chars` string.
    pub fn from_legacy_forbidden_chars(chars: &str) -> Self {
        let always: Vec<char> = chars.chars().filter(|c| *c == '`' || *c == '\0').collect();
        let unquoted: Vec<char> = chars.chars().filter(|c| *c != '`' && *c != '\0').collect();
        Self {
            always_forbidden: always,
            forbidden_unquoted: unquoted,
        }
    }

    /// Validate annotated characters from a single token.
    pub fn validate_token(&self, chars: &[AnnotatedChar]) -> anyhow::Result<()> {
        for ac in chars {
            if self.always_forbidden.contains(&ac.ch) {
                anyhow::bail!(
                    "shell argument contains always-forbidden character: {:?}",
                    ac.ch
                );
            }
            if ac.context == QuoteContext::Unquoted && self.forbidden_unquoted.contains(&ac.ch) {
                anyhow::bail!(
                    "shell argument contains unquoted forbidden metacharacter: {:?}",
                    ac.ch
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ShellPolicy {
    pub allowed_commands: Vec<String>,
    pub max_args: usize,
    pub max_arg_length: usize,
    pub max_output_bytes: usize,
    pub forbidden_chars: String,
    /// Context-aware policy. When `Some`, uses quote-aware validation.
    /// When `None`, falls back to legacy flat `forbidden_chars` check.
    pub command_policy: Option<ShellCommandPolicy>,
}

impl ShellPolicy {
    pub fn default_with_commands(allowed_commands: Vec<String>) -> Self {
        Self {
            allowed_commands,
            max_args: DEFAULT_MAX_SHELL_ARGS,
            max_arg_length: DEFAULT_MAX_ARG_LENGTH,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            forbidden_chars: DEFAULT_FORBIDDEN_CHARS.to_string(),
            command_policy: Some(ShellCommandPolicy::default()),
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

    /// Parse and validate a shell command input using context-aware or legacy mode.
    fn parse_and_validate(
        policy: &ShellPolicy,
        input: &str,
    ) -> anyhow::Result<(String, Vec<String>)> {
        if policy.command_policy.is_some() {
            Self::parse_context_aware(policy, input)
        } else {
            Self::parse_legacy(policy, input)
        }
    }

    /// Context-aware parsing: uses quote-aware tokenizer and structured policy.
    fn parse_context_aware(
        policy: &ShellPolicy,
        input: &str,
    ) -> anyhow::Result<(String, Vec<String>)> {
        let tokens = shell_parse::tokenize(input)?;
        let annotated = shell_parse::tokenize_annotated(input)?;

        if tokens.is_empty() {
            return Err(anyhow!("command is required"));
        }

        let command_name = tokens[0].text.clone();
        let args: Vec<String> = tokens[1..].iter().map(|t| t.text.clone()).collect();

        if args.len() > policy.max_args {
            return Err(anyhow!("too many shell arguments"));
        }

        let cmd_policy = policy.command_policy.as_ref().unwrap();
        for (i, token) in tokens.iter().enumerate().skip(1) {
            if token.text.is_empty() {
                return Err(anyhow!("empty shell argument is not allowed"));
            }
            if token.text.len() > policy.max_arg_length {
                return Err(anyhow!("shell argument exceeds max length"));
            }
            cmd_policy.validate_token(&annotated[i])?;
        }

        Ok((command_name, args))
    }

    /// Legacy parsing: flat whitespace split and flat forbidden_chars check.
    fn parse_legacy(policy: &ShellPolicy, input: &str) -> anyhow::Result<(String, Vec<String>)> {
        let mut parts = input.split_whitespace();
        let command_name = parts
            .next()
            .ok_or_else(|| anyhow!("command is required"))?
            .to_string();
        let args: Vec<String> = parts.map(ToString::to_string).collect();

        if args.len() > policy.max_args {
            return Err(anyhow!("too many shell arguments"));
        }
        for arg in &args {
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
        }

        Ok((command_name, args))
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
        let (command_name, args) = Self::parse_and_validate(&self.policy, input)?;
        if !self
            .policy
            .allowed_commands
            .iter()
            .any(|c| c == &command_name)
        {
            return Err(anyhow!("command is not in allowlist"));
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
    use super::*;
    use agentzero_core::{Tool, ToolContext};

    fn echo_tool() -> ShellTool {
        ShellTool::new(ShellPolicy::default_with_commands(vec!["echo".to_string()]))
    }

    fn ctx() -> ToolContext {
        ToolContext::new(".".to_string())
    }

    #[tokio::test]
    async fn shell_allows_allowlisted_command() {
        let result = echo_tool()
            .execute("echo hello", &ctx())
            .await
            .expect("shell should succeed");
        assert!(result.output.contains("stdout:\nhello"));
    }

    #[tokio::test]
    async fn shell_rejects_unquoted_metacharacters() {
        let result = echo_tool().execute("echo hello;uname", &ctx()).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("unquoted forbidden metacharacter"));
    }

    #[tokio::test]
    async fn shell_rejects_non_allowlisted_command() {
        let result = echo_tool().execute("pwd", &ctx()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("command is not in allowlist"));
    }

    #[tokio::test]
    async fn shell_truncates_stdout_to_policy_limit() {
        let mut policy = ShellPolicy::default_with_commands(vec!["echo".to_string()]);
        policy.max_output_bytes = 8;
        let tool = ShellTool::new(policy);
        let result = tool
            .execute("echo 1234567890", &ctx())
            .await
            .expect("shell should succeed");
        assert!(result.output.contains("stdout:\n12345678"));
        assert!(result.output.contains("<truncated at 8 bytes>"));
    }

    // B7: Context-aware policy tests

    #[tokio::test]
    async fn policy_allows_single_quoted_semicolon() {
        let result = echo_tool()
            .execute("echo 'hello;world'", &ctx())
            .await
            .expect("quoted semicolon should be allowed");
        assert!(result.output.contains("hello;world"));
    }

    #[tokio::test]
    async fn policy_allows_double_quoted_semicolon() {
        let result = echo_tool()
            .execute(r#"echo "hello;world""#, &ctx())
            .await
            .expect("quoted semicolon should be allowed");
        assert!(result.output.contains("hello;world"));
    }

    #[tokio::test]
    async fn policy_blocks_backtick_always() {
        let result = echo_tool().execute("echo '`uname`'", &ctx()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("always-forbidden"));
    }

    #[tokio::test]
    async fn policy_blocks_unquoted_dollar() {
        let result = echo_tool().execute("echo $HOME", &ctx()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unquoted forbidden metacharacter"));
    }

    #[tokio::test]
    async fn policy_allows_dollar_in_single_quotes() {
        let result = echo_tool()
            .execute("echo '$HOME'", &ctx())
            .await
            .expect("dollar in single quotes should be allowed");
        assert!(result.output.contains("$HOME"));
    }

    #[tokio::test]
    async fn legacy_mode_flat_check() {
        let mut policy = ShellPolicy::default_with_commands(vec!["echo".to_string()]);
        policy.command_policy = None; // disable context-aware
        let tool = ShellTool::new(policy);
        let result = tool.execute("echo hello;uname", &ctx()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("forbidden shell metacharacters"));
    }

    #[tokio::test]
    async fn shell_quoted_argument_with_spaces() {
        let result = echo_tool()
            .execute("echo 'hello world'", &ctx())
            .await
            .expect("quoted spaces should work");
        assert!(result.output.contains("hello world"));
    }
}
