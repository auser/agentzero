use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Mutex;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

const MAX_CONCURRENT: usize = 8;
const MAX_OUTPUT_BYTES: usize = 512 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "action")]
#[serde(rename_all = "snake_case")]
enum ProcessAction {
    Spawn { command: String },
    List,
    Output { id: usize },
    Kill { id: usize },
}

struct ProcessEntry {
    id: usize,
    command: String,
    handle: Option<tokio::task::JoinHandle<ProcessOutput>>,
    result: Option<ProcessOutput>,
}

struct ProcessOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

pub struct ProcessTool {
    entries: Mutex<Vec<ProcessEntry>>,
}

impl Default for ProcessTool {
    fn default() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }
}

impl ProcessTool {
    /// Extract finished handles from the entries (under lock) so we can await them
    /// outside the lock. Returns (index, handle) pairs for finished tasks.
    fn take_finished_handles(
        entries: &mut [ProcessEntry],
    ) -> Vec<(usize, tokio::task::JoinHandle<ProcessOutput>)> {
        let mut finished = Vec::new();
        for entry in entries.iter_mut() {
            if entry.result.is_some() {
                continue;
            }
            let is_finished = entry.handle.as_ref().is_some_and(|h| h.is_finished());
            if is_finished {
                if let Some(handle) = entry.handle.take() {
                    finished.push((entry.id, handle));
                }
            }
        }
        finished
    }

    /// Store results back into entries after awaiting handles.
    fn store_results(entries: &mut [ProcessEntry], results: Vec<(usize, ProcessOutput)>) {
        for (id, output) in results {
            if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                entry.result = Some(output);
            }
        }
    }

    /// Collect all finished process outputs. Must be called from async context.
    async fn collect_finished(&self) {
        let finished = {
            let mut entries = match self.entries.lock() {
                Ok(e) => e,
                Err(_) => return,
            };
            Self::take_finished_handles(&mut entries)
        };

        if finished.is_empty() {
            return;
        }

        let mut results = Vec::new();
        for (id, handle) in finished {
            let output = match handle.await {
                Ok(o) => o,
                Err(_) => ProcessOutput {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: "(task panicked)".to_string(),
                },
            };
            results.push((id, output));
        }

        if let Ok(mut entries) = self.entries.lock() {
            Self::store_results(&mut entries, results);
        }
    }
}

#[async_trait]
impl Tool for ProcessTool {
    fn name(&self) -> &'static str {
        "process"
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let action: ProcessAction =
            serde_json::from_str(input).context("process expects JSON with \"action\" field")?;

        match action {
            ProcessAction::Spawn { command } => {
                if command.trim().is_empty() {
                    return Err(anyhow!("command must not be empty"));
                }
                let mut entries = self.entries.lock().map_err(|_| anyhow!("lock poisoned"))?;
                let active = entries.iter().filter(|e| e.result.is_none()).count();
                if active >= MAX_CONCURRENT {
                    return Err(anyhow!(
                        "max concurrent processes reached ({MAX_CONCURRENT})"
                    ));
                }
                let id = entries.len();
                let workspace_root = ctx.workspace_root.clone();
                let cmd = command.clone();

                let handle = tokio::spawn(async move { run_process(&cmd, &workspace_root).await });

                entries.push(ProcessEntry {
                    id,
                    command: command.clone(),
                    handle: Some(handle),
                    result: None,
                });

                Ok(ToolResult {
                    output: format!("spawned process {id}: {command}"),
                })
            }

            ProcessAction::List => {
                self.collect_finished().await;
                let entries = self.entries.lock().map_err(|_| anyhow!("lock poisoned"))?;

                if entries.is_empty() {
                    return Ok(ToolResult {
                        output: "no processes".to_string(),
                    });
                }

                let lines: Vec<String> = entries
                    .iter()
                    .map(|e| {
                        let status = if e.result.is_some()
                            || e.handle.as_ref().is_some_and(|h| h.is_finished())
                        {
                            "finished"
                        } else {
                            "running"
                        };
                        format!("id={} status={} command={}", e.id, status, e.command)
                    })
                    .collect();

                Ok(ToolResult {
                    output: lines.join("\n"),
                })
            }

            ProcessAction::Output { id } => {
                self.collect_finished().await;
                let entries = self.entries.lock().map_err(|_| anyhow!("lock poisoned"))?;

                let entry = entries
                    .iter()
                    .find(|e| e.id == id)
                    .ok_or_else(|| anyhow!("process {id} not found"))?;

                if let Some(ref result) = entry.result {
                    let mut output = format!("exit={}\n", result.exit_code.unwrap_or(-1));
                    if !result.stdout.is_empty() {
                        output.push_str(&result.stdout);
                    }
                    if !result.stderr.is_empty() {
                        output.push_str("\nstderr:\n");
                        output.push_str(&result.stderr);
                    }
                    Ok(ToolResult { output })
                } else {
                    Ok(ToolResult {
                        output: format!("process {id} is still running"),
                    })
                }
            }

            ProcessAction::Kill { id } => {
                let mut entries = self.entries.lock().map_err(|_| anyhow!("lock poisoned"))?;
                let entry = entries
                    .iter_mut()
                    .find(|e| e.id == id)
                    .ok_or_else(|| anyhow!("process {id} not found"))?;

                if let Some(handle) = entry.handle.take() {
                    handle.abort();
                    entry.result = Some(ProcessOutput {
                        exit_code: None,
                        stdout: String::new(),
                        stderr: "(killed)".to_string(),
                    });
                }

                Ok(ToolResult {
                    output: format!("killed process {id}"),
                })
            }
        }
    }
}

async fn run_process(command: &str, workspace_root: &str) -> ProcessOutput {
    let result = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(workspace_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let mut child = match result {
        Ok(c) => c,
        Err(e) => {
            return ProcessOutput {
                exit_code: None,
                stdout: String::new(),
                stderr: format!("failed to spawn: {e}"),
            };
        }
    };

    let stdout_handle = child.stdout.take().unwrap();
    let stderr_handle = child.stderr.take().unwrap();

    let stdout_task = tokio::spawn(read_limited(stdout_handle));
    let stderr_task = tokio::spawn(read_limited(stderr_handle));

    let status = child.wait().await;
    let stdout = stdout_task
        .await
        .unwrap_or_else(|_| Ok(String::new()))
        .unwrap_or_default();
    let stderr = stderr_task
        .await
        .unwrap_or_else(|_| Ok(String::new()))
        .unwrap_or_default();

    ProcessOutput {
        exit_code: status.ok().and_then(|s| s.code()),
        stdout,
        stderr,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn process_spawn_list_output() {
        let tool = ProcessTool::default();
        let ctx = ToolContext::new(".".to_string());

        let result = tool
            .execute(r#"{"action": "spawn", "command": "echo hello"}"#, &ctx)
            .await
            .expect("spawn should succeed");
        assert!(result.output.contains("spawned process 0"));

        // Give the process time to finish
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let result = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("echo hello"));

        let result = tool
            .execute(r#"{"action": "output", "id": 0}"#, &ctx)
            .await
            .expect("output should succeed");
        assert!(result.output.contains("hello") || result.output.contains("still running"));
    }

    #[tokio::test]
    async fn process_rejects_empty_command() {
        let tool = ProcessTool::default();
        let err = tool
            .execute(
                r#"{"action": "spawn", "command": ""}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("empty command should fail");
        assert!(err.to_string().contains("command must not be empty"));
    }

    #[tokio::test]
    async fn process_kill_running() {
        let tool = ProcessTool::default();
        let ctx = ToolContext::new(".".to_string());

        tool.execute(r#"{"action": "spawn", "command": "sleep 60"}"#, &ctx)
            .await
            .expect("spawn should succeed");

        let result = tool
            .execute(r#"{"action": "kill", "id": 0}"#, &ctx)
            .await
            .expect("kill should succeed");
        assert!(result.output.contains("killed process 0"));
    }

    #[tokio::test]
    async fn process_nonexistent_id_fails() {
        let tool = ProcessTool::default();
        let err = tool
            .execute(
                r#"{"action": "output", "id": 99}"#,
                &ToolContext::new(".".to_string()),
            )
            .await
            .expect_err("nonexistent should fail");
        assert!(err.to_string().contains("not found"));
    }
}
