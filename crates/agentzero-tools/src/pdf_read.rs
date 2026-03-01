use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

const MAX_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Deserialize)]
struct PdfReadInput {
    path: String,
    #[serde(default)]
    page_start: Option<usize>,
    #[serde(default)]
    page_end: Option<usize>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PdfReadTool;

impl PdfReadTool {
    fn resolve_path(input_path: &str, workspace_root: &str) -> anyhow::Result<PathBuf> {
        if input_path.trim().is_empty() {
            return Err(anyhow!("path is required"));
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
        let canonical_root = Path::new(workspace_root)
            .canonicalize()
            .context("unable to resolve workspace root")?;
        let canonical = joined
            .canonicalize()
            .with_context(|| format!("file not found: {input_path}"))?;
        if !canonical.starts_with(&canonical_root) {
            return Err(anyhow!("path is outside workspace"));
        }
        Ok(canonical)
    }
}

#[async_trait]
impl Tool for PdfReadTool {
    fn name(&self) -> &'static str {
        "pdf_read"
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: PdfReadInput =
            serde_json::from_str(input).context("pdf_read expects JSON: {\"path\": \"...\"}")?;

        let file_path = Self::resolve_path(&req.path, &ctx.workspace_root)?;

        // Use pdftotext (from poppler-utils) which is commonly available
        let mut args = vec![file_path.to_string_lossy().to_string()];

        if let Some(start) = req.page_start {
            args.push("-f".to_string());
            args.push(start.to_string());
        }
        if let Some(end) = req.page_end {
            args.push("-l".to_string());
            args.push(end.to_string());
        }
        args.push("-".to_string()); // output to stdout

        let mut child = Command::new("pdftotext")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn pdftotext — is poppler-utils installed?")?;

        let stdout_handle = child.stdout.take().unwrap();
        let stderr_handle = child.stderr.take().unwrap();

        let stdout_task = tokio::spawn(read_limited(stdout_handle));
        let stderr_task = tokio::spawn(read_limited(stderr_handle));

        let status = child.wait().await.context("pdftotext command failed")?;
        let stdout = stdout_task.await.context("stdout join")??;
        let stderr = stderr_task.await.context("stderr join")??;

        if !status.success() {
            let mut msg = format!("pdftotext exited with code {}", status.code().unwrap_or(-1));
            if !stderr.is_empty() {
                msg.push_str(": ");
                msg.push_str(&stderr);
            }
            return Err(anyhow!(msg));
        }

        if stdout.is_empty() {
            Ok(ToolResult {
                output: "(no text content extracted)".to_string(),
            })
        } else {
            Ok(ToolResult { output: stdout })
        }
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
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-pdf-read-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn pdf_read_rejects_path_traversal() {
        let dir = temp_dir();
        let tool = PdfReadTool;
        let err = tool
            .execute(
                r#"{"path": "../escape.pdf"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("path traversal should fail");
        assert!(err.to_string().contains("path traversal"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn pdf_read_rejects_empty_path() {
        let dir = temp_dir();
        let tool = PdfReadTool;
        let err = tool
            .execute(
                r#"{"path": ""}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("empty path should fail");
        assert!(err.to_string().contains("path is required"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn pdf_read_rejects_nonexistent_file() {
        let dir = temp_dir();
        let tool = PdfReadTool;
        let err = tool
            .execute(
                r#"{"path": "nonexistent.pdf"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("nonexistent file should fail");
        assert!(err.to_string().contains("not found"));
        fs::remove_dir_all(dir).ok();
    }
}
