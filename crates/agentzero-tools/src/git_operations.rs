use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Component, Path};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

const DEFAULT_LOG_LIMIT: usize = 20;
const MAX_OUTPUT_BYTES: usize = 65536;

#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
#[serde(rename_all = "snake_case")]
enum GitOp {
    Status,
    Diff {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        staged: bool,
    },
    Log {
        #[serde(default = "default_log_limit")]
        limit: usize,
        #[serde(default)]
        path: Option<String>,
    },
    Branch {
        #[serde(default)]
        name: Option<String>,
    },
    Checkout {
        branch: String,
    },
    Add {
        paths: Vec<String>,
    },
    Commit {
        message: String,
    },
    Push {
        #[serde(default)]
        remote: Option<String>,
        #[serde(default)]
        branch: Option<String>,
    },
    Pull {
        #[serde(default)]
        remote: Option<String>,
        #[serde(default)]
        branch: Option<String>,
    },
    Show {
        #[serde(default)]
        rev: Option<String>,
    },
}

fn default_log_limit() -> usize {
    DEFAULT_LOG_LIMIT
}

pub struct GitOperationsTool;

impl Default for GitOperationsTool {
    fn default() -> Self {
        Self
    }
}

impl GitOperationsTool {
    pub fn new() -> Self {
        Self
    }

    fn validate_path(path: &str) -> anyhow::Result<()> {
        if path.trim().is_empty() {
            return Err(anyhow!("path must not be empty"));
        }
        let p = Path::new(path);
        if p.is_absolute() {
            return Err(anyhow!("absolute paths are not allowed"));
        }
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(anyhow!("path traversal is not allowed"));
        }
        Ok(())
    }

    fn validate_ref(s: &str) -> anyhow::Result<()> {
        if s.trim().is_empty() {
            return Err(anyhow!("ref must not be empty"));
        }
        // Block shell metacharacters in refs.
        if s.chars()
            .any(|c| matches!(c, ';' | '&' | '|' | '`' | '$' | '>' | '<' | '\n' | '\r'))
        {
            return Err(anyhow!("ref contains forbidden characters"));
        }
        Ok(())
    }

    async fn run_git(workspace_root: &str, args: &[&str]) -> anyhow::Result<ToolResult> {
        let mut child = Command::new("git")
            .args(args)
            .current_dir(workspace_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn git")?;

        let stdout_handle = child.stdout.take().unwrap();
        let stderr_handle = child.stderr.take().unwrap();

        let stdout_task = tokio::spawn(Self::read_limited(stdout_handle));
        let stderr_task = tokio::spawn(Self::read_limited(stderr_handle));

        let status = child.wait().await.context("git command failed")?;
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

        Ok(ToolResult { output })
    }

    async fn read_limited<R: tokio::io::AsyncRead + Unpin>(
        mut reader: R,
    ) -> anyhow::Result<String> {
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
}

#[async_trait]
impl Tool for GitOperationsTool {
    fn name(&self) -> &'static str {
        "git_operations"
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let op: GitOp =
            serde_json::from_str(input).context("git_operations expects JSON with \"op\" field")?;

        match op {
            GitOp::Status => Self::run_git(&ctx.workspace_root, &["status", "--short"]).await,

            GitOp::Diff { path, staged } => {
                let mut args = vec!["diff"];
                if staged {
                    args.push("--cached");
                }
                if let Some(ref p) = path {
                    Self::validate_path(p)?;
                    args.push("--");
                    args.push(p);
                }
                Self::run_git(&ctx.workspace_root, &args).await
            }

            GitOp::Log { limit, path } => {
                let limit_str = format!("-{}", limit.min(100));
                let mut args = vec!["log", &limit_str, "--oneline"];
                if let Some(ref p) = path {
                    Self::validate_path(p)?;
                    args.push("--");
                    args.push(p);
                }
                Self::run_git(&ctx.workspace_root, &args).await
            }

            GitOp::Branch { name } => {
                if let Some(ref n) = name {
                    Self::validate_ref(n)?;
                    Self::run_git(&ctx.workspace_root, &["branch", n]).await
                } else {
                    Self::run_git(&ctx.workspace_root, &["branch", "--list"]).await
                }
            }

            GitOp::Checkout { ref branch } => {
                Self::validate_ref(branch)?;
                Self::run_git(&ctx.workspace_root, &["checkout", branch]).await
            }

            GitOp::Add { ref paths } => {
                if paths.is_empty() {
                    return Err(anyhow!("add requires at least one path"));
                }
                for p in paths {
                    Self::validate_path(p)?;
                }
                let mut args = vec!["add"];
                let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
                args.extend(path_refs);
                Self::run_git(&ctx.workspace_root, &args).await
            }

            GitOp::Commit { ref message } => {
                if message.trim().is_empty() {
                    return Err(anyhow!("commit message must not be empty"));
                }
                Self::run_git(&ctx.workspace_root, &["commit", "-m", message]).await
            }

            GitOp::Push {
                ref remote,
                ref branch,
            } => {
                let mut args = vec!["push"];
                if let Some(ref r) = remote {
                    Self::validate_ref(r)?;
                    args.push(r);
                }
                if let Some(ref b) = branch {
                    Self::validate_ref(b)?;
                    args.push(b);
                }
                Self::run_git(&ctx.workspace_root, &args).await
            }

            GitOp::Pull {
                ref remote,
                ref branch,
            } => {
                let mut args = vec!["pull"];
                if let Some(ref r) = remote {
                    Self::validate_ref(r)?;
                    args.push(r);
                }
                if let Some(ref b) = branch {
                    Self::validate_ref(b)?;
                    args.push(b);
                }
                Self::run_git(&ctx.workspace_root, &args).await
            }

            GitOp::Show { ref rev } => {
                let mut args = vec!["show", "--stat"];
                if let Some(ref r) = rev {
                    Self::validate_ref(r)?;
                    args.push(r);
                }
                Self::run_git(&ctx.workspace_root, &args).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GitOperationsTool;
    use agentzero_core::{Tool, ToolContext};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_git_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-git-ops-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        // Initialize a git repo.
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output()
            .expect("git init should succeed");
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&dir)
            .output()
            .ok();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&dir)
            .output()
            .ok();
        // Point hooksPath to an empty directory so inherited global/system
        // hooks (e.g. core.hooksPath = .githooks) never fire in test repos.
        let empty_hooks = dir.join(".no-hooks");
        fs::create_dir_all(&empty_hooks).ok();
        std::process::Command::new("git")
            .args(["config", "core.hooksPath", &empty_hooks.to_string_lossy()])
            .current_dir(&dir)
            .output()
            .ok();
        // Disable GPG signing that may be inherited from global config.
        std::process::Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
            .current_dir(&dir)
            .output()
            .ok();
        dir
    }

    #[tokio::test]
    async fn git_status_in_repo() {
        let dir = temp_git_dir();
        let tool = GitOperationsTool::new();
        let result = tool
            .execute(
                r#"{"op": "status"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("git status should succeed");
        assert!(result.output.contains("exit=0"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn git_log_in_repo_with_commits() {
        let dir = temp_git_dir();
        fs::write(dir.join("test.txt"), "hello").unwrap();
        std::process::Command::new("git")
            .args(["add", "test.txt"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let commit_out = std::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            commit_out.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&commit_out.stderr)
        );

        let tool = GitOperationsTool::new();
        let result = tool
            .execute(
                r#"{"op": "log", "limit": 5}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("git log should succeed");
        assert!(result.output.contains("initial"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn git_rejects_path_traversal_in_add_negative_path() {
        let dir = temp_git_dir();
        let tool = GitOperationsTool::new();
        let err = tool
            .execute(
                r#"{"op": "add", "paths": ["../escape.txt"]}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("path traversal should be denied");
        assert!(err.to_string().contains("path traversal"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn git_rejects_metacharacters_in_ref_negative_path() {
        let dir = temp_git_dir();
        let tool = GitOperationsTool::new();
        let err = tool
            .execute(
                r#"{"op": "checkout", "branch": "main;rm -rf /"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("metacharacters should be rejected");
        assert!(err.to_string().contains("forbidden characters"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn git_rejects_empty_commit_message_negative_path() {
        let dir = temp_git_dir();
        let tool = GitOperationsTool::new();
        let err = tool
            .execute(
                r#"{"op": "commit", "message": ""}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("empty message should fail");
        assert!(err.to_string().contains("commit message must not be empty"));
        fs::remove_dir_all(dir).ok();
    }
}
