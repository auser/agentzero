use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
use tokio::fs;

#[derive(Debug, Deserialize)]
struct FileEditInput {
    path: String,
    edits: Vec<Edit>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct Edit {
    old_text: String,
    new_text: String,
}

#[tool(
    name = "file_edit",
    description = "Apply surgical text edits to a file by replacing exact old_text matches with new_text. Supports multiple edits and dry-run mode."
)]
pub struct FileEditTool {
    allowed_root: PathBuf,
    max_file_bytes: u64,
}

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct FileEditSchema {
    /// Path to the file to edit
    path: String,
    /// Array of search-and-replace edits
    edits: Vec<serde_json::Value>,
    /// If true, show what would change without modifying the file
    #[serde(default)]
    dry_run: Option<bool>,
}

impl FileEditTool {
    pub fn new(allowed_root: PathBuf, max_file_bytes: u64) -> Self {
        Self {
            allowed_root,
            max_file_bytes,
        }
    }

    fn resolve_path(&self, input_path: &str, workspace_root: &str) -> anyhow::Result<PathBuf> {
        if input_path.trim().is_empty() {
            return Err(anyhow!("file_edit.path is required"));
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
        let canonical = joined
            .canonicalize()
            .with_context(|| format!("unable to resolve path: {input_path}"))?;
        let canonical_root = self
            .allowed_root
            .canonicalize()
            .context("unable to resolve allowed root")?;
        if !canonical.starts_with(&canonical_root) {
            return Err(anyhow!("path is outside allowed root"));
        }
        Ok(canonical)
    }
}

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(FileEditSchema::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let request: FileEditInput = serde_json::from_str(input).context(
            "file_edit expects JSON: {\"path\", \"edits\": [{\"old_text\", \"new_text\"}], \"dry_run\"}",
        )?;

        if request.edits.is_empty() {
            return Err(anyhow!("edits array must not be empty"));
        }

        let dest = self.resolve_path(&request.path, &ctx.workspace_root)?;

        // B7: Hard-link guard.
        crate::autonomy::AutonomyPolicy::check_hard_links(&dest.to_string_lossy())?;

        // B7: Sensitive file detection.
        if !ctx.allow_sensitive_file_writes
            && crate::autonomy::is_sensitive_path(&dest.to_string_lossy())
        {
            return Err(anyhow!(
                "refusing to edit sensitive file: {}",
                dest.display()
            ));
        }

        let content = fs::read_to_string(&dest)
            .await
            .with_context(|| format!("failed to read file: {}", request.path))?;

        if content.len() as u64 > self.max_file_bytes {
            return Err(anyhow!(
                "file is too large ({} bytes, max {})",
                content.len(),
                self.max_file_bytes
            ));
        }

        let mut result = content;
        for (i, edit) in request.edits.iter().enumerate() {
            if edit.old_text.is_empty() {
                return Err(anyhow!("edit {} has empty old_text", i + 1));
            }
            if edit.old_text == edit.new_text {
                return Err(anyhow!(
                    "edit {} has identical old_text and new_text",
                    i + 1
                ));
            }

            let count = result.matches(&edit.old_text).count();
            if count == 0 {
                return Err(anyhow!("edit {}: old_text not found in file", i + 1));
            }
            if count > 1 {
                return Err(anyhow!(
                    "edit {}: old_text matches {} locations (must be unique)",
                    i + 1,
                    count
                ));
            }

            result = result.replacen(&edit.old_text, &edit.new_text, 1);
        }

        if request.dry_run {
            return Ok(ToolResult {
                output: format!(
                    "dry_run=true path={} edits={}",
                    request.path,
                    request.edits.len()
                ),
            });
        }

        fs::write(&dest, &result)
            .await
            .with_context(|| format!("failed to write file: {}", request.path))?;

        Ok(ToolResult {
            output: format!(
                "path={} edits={} bytes={}",
                request.path,
                request.edits.len(),
                result.len()
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FileEditTool;
    use agentzero_core::{Tool, ToolContext};
    use std::fs;
    use std::path::{Path, PathBuf};
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
            "agentzero-file-edit-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn tool(dir: &Path) -> FileEditTool {
        FileEditTool::new(dir.to_path_buf(), 256 * 1024)
    }

    #[tokio::test]
    async fn file_edit_single_replacement() {
        let dir = temp_dir();
        fs::write(
            dir.join("test.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        let input = r#"{"path":"test.rs","edits":[{"old_text":"hello","new_text":"world"}]}"#;
        let result = tool(&dir)
            .execute(input, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect("edit should succeed");
        assert!(result.output.contains("edits=1"));
        let content = fs::read_to_string(dir.join("test.rs")).unwrap();
        assert!(content.contains("world"));
        assert!(!content.contains("hello"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn file_edit_multiple_edits() {
        let dir = temp_dir();
        fs::write(dir.join("test.txt"), "aaa\nbbb\nccc\n").unwrap();
        let input = r#"{"path":"test.txt","edits":[{"old_text":"aaa","new_text":"AAA"},{"old_text":"ccc","new_text":"CCC"}]}"#;
        let result = tool(&dir)
            .execute(input, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect("multi-edit should succeed");
        assert!(result.output.contains("edits=2"));
        let content = fs::read_to_string(dir.join("test.txt")).unwrap();
        assert!(content.contains("AAA") && content.contains("CCC"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn file_edit_dry_run_no_write() {
        let dir = temp_dir();
        fs::write(dir.join("test.txt"), "original").unwrap();
        let input = r#"{"path":"test.txt","edits":[{"old_text":"original","new_text":"modified"}],"dry_run":true}"#;
        let result = tool(&dir)
            .execute(input, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect("dry_run should succeed");
        assert!(result.output.contains("dry_run=true"));
        assert_eq!(
            fs::read_to_string(dir.join("test.txt")).unwrap(),
            "original"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn file_edit_rejects_not_found_negative_path() {
        let dir = temp_dir();
        fs::write(dir.join("test.txt"), "content").unwrap();
        let input = r#"{"path":"test.txt","edits":[{"old_text":"nonexistent","new_text":"x"}]}"#;
        let err = tool(&dir)
            .execute(input, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect_err("old_text not found should fail");
        assert!(err.to_string().contains("not found in file"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn file_edit_rejects_ambiguous_match_negative_path() {
        let dir = temp_dir();
        fs::write(dir.join("test.txt"), "aaa\naaa\n").unwrap();
        let input = r#"{"path":"test.txt","edits":[{"old_text":"aaa","new_text":"bbb"}]}"#;
        let err = tool(&dir)
            .execute(input, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect_err("ambiguous match should fail");
        assert!(err.to_string().contains("matches 2 locations"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn file_edit_rejects_path_traversal_negative_path() {
        let dir = temp_dir();
        let input = r#"{"path":"../escape.txt","edits":[{"old_text":"a","new_text":"b"}]}"#;
        let err = tool(&dir)
            .execute(input, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect_err("path traversal should be denied");
        assert!(err.to_string().contains("path traversal"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn file_edit_rejects_empty_edits_negative_path() {
        let dir = temp_dir();
        let input = r#"{"path":"test.txt","edits":[]}"#;
        let err = tool(&dir)
            .execute(input, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect_err("empty edits should fail");
        assert!(err.to_string().contains("edits array must not be empty"));
        fs::remove_dir_all(dir).ok();
    }
}
