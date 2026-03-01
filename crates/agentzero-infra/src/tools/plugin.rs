use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct PluginResponse {
    output: String,
}

pub struct ProcessPluginTool {
    name: &'static str,
    command: String,
    args: Vec<String>,
}

impl ProcessPluginTool {
    pub fn new(name: &'static str, command: String, args: Vec<String>) -> anyhow::Result<Self> {
        if name.trim().is_empty() {
            return Err(anyhow!("plugin tool name cannot be empty"));
        }
        if command.trim().is_empty() {
            return Err(anyhow!("plugin command cannot be empty"));
        }
        Ok(Self {
            name,
            command,
            args,
        })
    }
}

#[async_trait]
impl Tool for ProcessPluginTool {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let mut child = Command::new(&self.command)
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn plugin command: {} {}",
                    self.command,
                    self.args.join(" ")
                )
            })?;

        let payload = json!({
            "input": input,
            "workspace_root": ctx.workspace_root,
        })
        .to_string();

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(payload.as_bytes())
                .await
                .context("failed to write plugin stdin")?;
        }

        let output = child
            .wait_with_output()
            .await
            .context("failed waiting for plugin process")?;

        if !output.status.success() {
            return Err(anyhow!(
                "plugin exited with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8(output.stdout).context("plugin stdout was not UTF-8")?;
        if let Ok(parsed) = serde_json::from_str::<PluginResponse>(&stdout) {
            return Ok(ToolResult {
                output: parsed.output,
            });
        }

        Ok(ToolResult { output: stdout })
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessPluginTool;
    use agentzero_core::{Tool, ToolContext};

    #[tokio::test]
    async fn process_plugin_echoes_input_payload() {
        let tool = ProcessPluginTool::new("plugin_echo", "cat".to_string(), vec![])
            .expect("tool should be constructed");

        let result = tool
            .execute("hello", &ToolContext::new("/tmp".to_string()))
            .await
            .expect("plugin execution should succeed");

        assert!(result.output.contains("hello"));
    }

    #[test]
    fn process_plugin_rejects_empty_command() {
        let tool = ProcessPluginTool::new("plugin_echo", " ".to_string(), vec![]);
        assert!(tool.is_err());
    }
}
