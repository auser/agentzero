use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
use tokio::fs;

const DEFAULT_MAX_WRITE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct WriteFilePolicy {
    pub allowed_root: PathBuf,
    pub max_write_bytes: u64,
}

impl WriteFilePolicy {
    pub fn default_for_root(allowed_root: PathBuf) -> Self {
        Self {
            allowed_root,
            max_write_bytes: DEFAULT_MAX_WRITE_BYTES,
        }
    }
}

#[derive(Debug, Deserialize)]
struct WriteFileInput {
    path: String,
    content: String,
    #[serde(default)]
    overwrite: bool,
    #[serde(default)]
    dry_run: bool,
}

pub struct WriteFileTool {
    allowed_root: PathBuf,
    max_write_bytes: u64,
}

impl WriteFileTool {
    pub fn new(policy: WriteFilePolicy) -> Self {
        Self {
            allowed_root: policy.allowed_root,
            max_write_bytes: policy.max_write_bytes,
        }
    }

    fn parse_input(input: &str) -> anyhow::Result<WriteFileInput> {
        serde_json::from_str(input).context(
            "write_file expects JSON input: {\"path\",\"content\",\"overwrite\",\"dry_run\"}",
        )
    }

    fn resolve_destination(
        &self,
        input_path: &str,
        workspace_root: &str,
    ) -> anyhow::Result<PathBuf> {
        if input_path.trim().is_empty() {
            return Err(anyhow!("write_file.path is required"));
        }
        let relative = Path::new(input_path);
        if relative.is_absolute() {
            return Err(anyhow!("absolute paths are not allowed"));
        }
        if relative
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(anyhow!("path traversal is not allowed"));
        }

        let joined = Path::new(workspace_root).join(relative);
        let file_name = joined
            .file_name()
            .ok_or_else(|| anyhow!("write_file.path must target a file"))?
            .to_os_string();
        let parent = joined
            .parent()
            .ok_or_else(|| anyhow!("write_file.path must have a parent directory"))?;
        let canonical_parent = parent
            .canonicalize()
            .with_context(|| format!("unable to resolve write target parent: {input_path}"))?;
        let canonical_allowed_root = self
            .allowed_root
            .canonicalize()
            .context("unable to resolve allowed root")?;
        if !canonical_parent.starts_with(&canonical_allowed_root) {
            return Err(anyhow!("path is outside allowed root"));
        }
        Ok(canonical_parent.join(file_name))
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let request = Self::parse_input(input)?;
        let destination = self.resolve_destination(&request.path, &ctx.workspace_root)?;

        let bytes = request.content.as_bytes();
        if bytes.len() as u64 > self.max_write_bytes {
            return Err(anyhow!(
                "content is too large (max {} bytes)",
                self.max_write_bytes
            ));
        }

        let exists = fs::try_exists(&destination)
            .await
            .context("failed to check destination path")?;
        if exists && !request.overwrite {
            return Err(anyhow!(
                "destination already exists; set overwrite=true to replace"
            ));
        }

        if request.dry_run {
            return Ok(ToolResult {
                output: format!(
                    "dry_run=true path={} bytes={} overwrite={}",
                    destination.display(),
                    bytes.len(),
                    request.overwrite
                ),
            });
        }

        fs::write(&destination, bytes)
            .await
            .with_context(|| format!("failed to write file: {}", destination.display()))?;

        Ok(ToolResult {
            output: format!(
                "dry_run=false path={} bytes={} overwrite={}",
                destination.display(),
                bytes.len(),
                request.overwrite
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{WriteFilePolicy, WriteFileTool};
    use agentzero_core::{Tool, ToolContext};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-write-file-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn write_file_writes_inside_allowed_root() {
        let dir = temp_dir();
        let tool = WriteFileTool::new(WriteFilePolicy::default_for_root(dir.clone()));
        let result = tool
            .execute(
                r#"{"path":"note.txt","content":"hello","overwrite":false,"dry_run":false}"#,
                &ToolContext {
                    workspace_root: dir.to_string_lossy().to_string(),
                },
            )
            .await
            .expect("write_file should succeed");

        assert!(result.output.contains("dry_run=false"));
        let content = fs::read_to_string(dir.join("note.txt")).expect("written file should exist");
        assert_eq!(content, "hello");
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn write_file_dry_run_does_not_write() {
        let dir = temp_dir();
        let tool = WriteFileTool::new(WriteFilePolicy::default_for_root(dir.clone()));
        let result = tool
            .execute(
                r#"{"path":"note.txt","content":"hello","overwrite":false,"dry_run":true}"#,
                &ToolContext {
                    workspace_root: dir.to_string_lossy().to_string(),
                },
            )
            .await
            .expect("dry run should succeed");

        assert!(result.output.contains("dry_run=true"));
        assert!(!dir.join("note.txt").exists());
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn write_file_rejects_existing_file_when_overwrite_false() {
        let dir = temp_dir();
        let target = dir.join("note.txt");
        fs::write(&target, "old").expect("seed file should be written");
        let tool = WriteFileTool::new(WriteFilePolicy::default_for_root(dir.clone()));
        let result = tool
            .execute(
                r#"{"path":"note.txt","content":"new","overwrite":false,"dry_run":false}"#,
                &ToolContext {
                    workspace_root: dir.to_string_lossy().to_string(),
                },
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("overwrite=false should fail when file exists")
            .to_string()
            .contains("overwrite=true"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn write_file_allows_overwrite_when_enabled() {
        let dir = temp_dir();
        let target = dir.join("note.txt");
        fs::write(&target, "old").expect("seed file should be written");
        let tool = WriteFileTool::new(WriteFilePolicy::default_for_root(dir.clone()));
        tool.execute(
            r#"{"path":"note.txt","content":"new","overwrite":true,"dry_run":false}"#,
            &ToolContext {
                workspace_root: dir.to_string_lossy().to_string(),
            },
        )
        .await
        .expect("overwrite=true should succeed");

        let content = fs::read_to_string(&target).expect("target should be readable");
        assert_eq!(content, "new");
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn write_file_rejects_path_outside_allowed_root() {
        let dir = temp_dir();
        let tool = WriteFileTool::new(WriteFilePolicy::default_for_root(dir.clone()));
        let result = tool
            .execute(
                r#"{"path":"../escape.txt","content":"x","overwrite":false,"dry_run":false}"#,
                &ToolContext {
                    workspace_root: dir.to_string_lossy().to_string(),
                },
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("traversal should be denied")
            .to_string()
            .contains("path traversal is not allowed"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn write_file_rejects_content_larger_than_policy_limit() {
        let dir = temp_dir();
        let tool = WriteFileTool::new(WriteFilePolicy {
            allowed_root: dir.clone(),
            max_write_bytes: 4,
        });
        let result = tool
            .execute(
                r#"{"path":"note.txt","content":"12345","overwrite":false,"dry_run":false}"#,
                &ToolContext {
                    workspace_root: dir.to_string_lossy().to_string(),
                },
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("oversized content should fail")
            .to_string()
            .contains("content is too large"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
