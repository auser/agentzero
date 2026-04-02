//! Checkpoint/file recovery — automatic snapshots before file mutations.
//!
//! Implemented as a `ToolMiddleware` that intercepts `write_file` and
//! `apply_patch` tool calls, snapshotting the target file before mutation.
//! Snapshots are plain file copies stored under `.agentzero/checkpoints/`.

use agentzero_core::tool_middleware::ToolMiddleware;
use agentzero_core::ToolContext;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Maximum number of checkpoints to retain per session.
const MAX_CHECKPOINTS: usize = 50;

/// Middleware that snapshots files before write_file and apply_patch tools.
pub struct CheckpointMiddleware {
    session_id: String,
}

impl CheckpointMiddleware {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
        }
    }

    fn checkpoint_dir(&self, workspace: &str) -> PathBuf {
        Path::new(workspace)
            .join(".agentzero")
            .join("checkpoints")
            .join(&self.session_id)
    }

    /// Extract the file path from tool input JSON.
    fn extract_file_path(input: &str) -> Option<String> {
        serde_json::from_str::<serde_json::Value>(input)
            .ok()
            .and_then(|v| {
                v.get("path")
                    .or_else(|| v.get("file_path"))
                    .or_else(|| v.get("file"))
                    .and_then(|p| p.as_str())
                    .map(String::from)
            })
    }

    /// Snapshot a file before mutation. Returns the checkpoint path if successful.
    fn snapshot_file(&self, file_path: &str, workspace: &str) -> Option<PathBuf> {
        let source = Path::new(file_path);
        if !source.exists() {
            return None; // New file, nothing to snapshot.
        }

        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);

        // Preserve relative path structure under checkpoints.
        let relative = source.strip_prefix(workspace).unwrap_or(source);
        let checkpoint_dir = self.checkpoint_dir(workspace);
        let dest = checkpoint_dir.join(format!("{ts}-{seq}")).join(relative);

        if let Some(parent) = dest.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                warn!(path = %dest.display(), "failed to create checkpoint directory");
                return None;
            }
        }

        match std::fs::copy(source, &dest) {
            Ok(_) => {
                debug!(
                    source = %source.display(),
                    checkpoint = %dest.display(),
                    "file checkpointed"
                );
                Some(dest)
            }
            Err(e) => {
                warn!(
                    source = %source.display(),
                    error = %e,
                    "failed to checkpoint file"
                );
                None
            }
        }
    }

    /// List all checkpoints for this session.
    pub fn list_checkpoints(&self, workspace: &str) -> Vec<CheckpointEntry> {
        let dir = self.checkpoint_dir(workspace);
        let mut entries = Vec::new();

        if let Ok(timestamps) = std::fs::read_dir(&dir) {
            for ts_entry in timestamps.flatten() {
                let ts_name = ts_entry.file_name().to_string_lossy().to_string();
                if let Ok(files) = walk_files(&ts_entry.path()) {
                    for file in files {
                        let relative = file
                            .strip_prefix(ts_entry.path())
                            .unwrap_or(&file)
                            .to_path_buf();
                        entries.push(CheckpointEntry {
                            timestamp: ts_name.clone(),
                            relative_path: relative,
                            checkpoint_path: file,
                        });
                    }
                }
            }
        }

        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        entries.truncate(MAX_CHECKPOINTS);
        entries
    }

    /// Restore a file from a checkpoint.
    pub fn restore(
        &self,
        checkpoint_path: &Path,
        target_path: &Path,
        workspace: &str,
    ) -> anyhow::Result<()> {
        // Pre-rollback snapshot: save current version before restoring.
        if target_path.exists() {
            self.snapshot_file(&target_path.to_string_lossy(), workspace);
        }
        std::fs::copy(checkpoint_path, target_path)?;
        Ok(())
    }
}

/// A single checkpoint entry.
#[derive(Debug, Clone)]
pub struct CheckpointEntry {
    pub timestamp: String,
    pub relative_path: PathBuf,
    pub checkpoint_path: PathBuf,
}

/// Recursively walk files in a directory.
fn walk_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if dir.is_file() {
        files.push(dir.to_path_buf());
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk_files(&path)?);
        } else {
            files.push(path);
        }
    }
    Ok(files)
}

/// Tools that trigger checkpointing.
const CHECKPOINT_TOOLS: &[&str] = &["write_file", "apply_patch", "file_edit"];

#[async_trait]
impl ToolMiddleware for CheckpointMiddleware {
    async fn before(&self, tool_name: &str, input: &str, ctx: &ToolContext) -> anyhow::Result<()> {
        if CHECKPOINT_TOOLS.contains(&tool_name) {
            if let Some(file_path) = Self::extract_file_path(input) {
                self.snapshot_file(&file_path, &ctx.workspace_root);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_file_path_from_json() {
        let input = r#"{"path": "/tmp/test.txt", "content": "hello"}"#;
        assert_eq!(
            CheckpointMiddleware::extract_file_path(input),
            Some("/tmp/test.txt".to_string())
        );
    }

    #[test]
    fn extract_file_path_alternate_keys() {
        let input = r#"{"file_path": "/tmp/alt.txt"}"#;
        assert_eq!(
            CheckpointMiddleware::extract_file_path(input),
            Some("/tmp/alt.txt".to_string())
        );
    }

    #[test]
    fn extract_file_path_missing() {
        let input = r#"{"content": "no path here"}"#;
        assert!(CheckpointMiddleware::extract_file_path(input).is_none());
    }

    #[test]
    fn snapshot_creates_checkpoint() {
        let dir = tempfile::tempdir().expect("temp dir");
        let workspace = dir.path().to_string_lossy().to_string();

        // Create a source file.
        let source = dir.path().join("test.txt");
        std::fs::write(&source, "original content").expect("write");

        let mw = CheckpointMiddleware::new("test-session");
        let result = mw.snapshot_file(&source.to_string_lossy(), &workspace);
        assert!(result.is_some(), "should create checkpoint");

        // Verify checkpoint content.
        let checkpoint = result.expect("checkpoint path");
        let content = std::fs::read_to_string(checkpoint).expect("read checkpoint");
        assert_eq!(content, "original content");
    }

    #[test]
    fn snapshot_nonexistent_returns_none() {
        let mw = CheckpointMiddleware::new("s");
        assert!(mw.snapshot_file("/nonexistent/file.txt", "/tmp").is_none());
    }

    #[test]
    fn list_checkpoints_finds_entries() {
        let dir = tempfile::tempdir().expect("temp dir");
        let workspace = dir.path().to_string_lossy().to_string();

        let source = dir.path().join("data.txt");
        std::fs::write(&source, "v1").expect("write");

        let mw = CheckpointMiddleware::new("list-session");
        mw.snapshot_file(&source.to_string_lossy(), &workspace);

        let entries = mw.list_checkpoints(&workspace);
        assert!(!entries.is_empty(), "should find at least one checkpoint");
    }

    #[test]
    fn restore_copies_and_snapshots_current() {
        let dir = tempfile::tempdir().expect("temp dir");
        let workspace = dir.path().to_string_lossy().to_string();

        let source = dir.path().join("restore.txt");
        std::fs::write(&source, "original").expect("write");

        let mw = CheckpointMiddleware::new("restore-session");
        let checkpoint = mw
            .snapshot_file(&source.to_string_lossy(), &workspace)
            .expect("checkpoint");

        // Modify the file.
        std::fs::write(&source, "modified").expect("modify");

        // Restore from checkpoint.
        mw.restore(&checkpoint, &source, &workspace)
            .expect("restore");

        let content = std::fs::read_to_string(&source).expect("read");
        assert_eq!(content, "original", "should restore original content");

        // Pre-rollback snapshot should exist (the "modified" version).
        let entries = mw.list_checkpoints(&workspace);
        assert!(
            entries.len() >= 2,
            "should have original checkpoint + pre-rollback snapshot"
        );
    }
}
