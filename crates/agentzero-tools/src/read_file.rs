use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use std::path::{Component, Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncReadExt;

const DEFAULT_MAX_READ_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct ReadFilePolicy {
    pub allowed_root: PathBuf,
    pub max_read_bytes: u64,
    pub allow_binary: bool,
}

impl ReadFilePolicy {
    pub fn default_for_root(allowed_root: PathBuf) -> Self {
        Self {
            allowed_root,
            max_read_bytes: DEFAULT_MAX_READ_BYTES,
            allow_binary: false,
        }
    }
}

pub struct ReadFileTool {
    allowed_root: PathBuf,
    max_read_bytes: u64,
    allow_binary: bool,
}

impl ReadFileTool {
    pub fn new(policy: ReadFilePolicy) -> Self {
        Self {
            allowed_root: policy.allowed_root,
            max_read_bytes: policy.max_read_bytes,
            allow_binary: policy.allow_binary,
        }
    }

    fn resolve_safe(&self, input_path: &str, workspace_root: &str) -> anyhow::Result<PathBuf> {
        if input_path.trim().is_empty() {
            return Err(anyhow!("file path is required"));
        }

        let input = Path::new(input_path);
        if input.is_absolute() {
            return Err(anyhow!("absolute paths are not allowed"));
        }
        if input
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(anyhow!("path traversal is not allowed"));
        }

        let joined = Path::new(workspace_root).join(input_path);
        let normalized = joined
            .canonicalize()
            .with_context(|| format!("unable to resolve file path: {input_path}"))?;
        let canonical_allowed_root = self
            .allowed_root
            .canonicalize()
            .context("unable to resolve allowed root")?;
        if !normalized.starts_with(canonical_allowed_root) {
            return Err(anyhow!("path is outside allowed root"));
        }
        Ok(normalized)
    }

    fn validate_file_policy(&self, raw: &[u8]) -> anyhow::Result<()> {
        if raw.len() as u64 > self.max_read_bytes {
            return Err(anyhow!(
                "file is too large (max {} bytes)",
                self.max_read_bytes
            ));
        }

        if !self.allow_binary && Self::looks_binary(raw) {
            return Err(anyhow!("binary files are not allowed"));
        }

        Ok(())
    }

    fn looks_binary(raw: &[u8]) -> bool {
        if raw.is_empty() {
            return false;
        }
        if raw.contains(&0) {
            return true;
        }
        if std::str::from_utf8(raw).is_err() {
            return true;
        }

        // Fallback heuristic for control-character-heavy payloads.
        let control_count = raw
            .iter()
            .filter(|b| **b < 0x09 || (**b > 0x0D && **b < 0x20))
            .count();
        control_count * 10 > raw.len()
    }

    async fn read_limited(path: &Path, max_bytes: u64) -> anyhow::Result<Vec<u8>> {
        let mut file = fs::File::open(path).await.context("failed to open file")?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(max_bytes + 1)
            .read_to_end(&mut bytes)
            .await
            .context("failed to read file")?;
        Ok(bytes)
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let safe_path = self.resolve_safe(input, &ctx.workspace_root)?;

        // B7: Hard-link guard — refuse files with multiple hard links.
        agentzero_autonomy::AutonomyPolicy::check_hard_links(&safe_path.to_string_lossy())?;

        // B7: Sensitive file detection — block unless explicitly allowed.
        if !ctx.allow_sensitive_file_reads
            && agentzero_autonomy::is_sensitive_path(&safe_path.to_string_lossy())
        {
            return Err(anyhow!(
                "refusing to read sensitive file: {}",
                safe_path.display()
            ));
        }

        let metadata = fs::metadata(&safe_path)
            .await
            .context("failed to read file metadata")?;
        if metadata.len() > self.max_read_bytes {
            return Err(anyhow!(
                "file is too large (max {} bytes)",
                self.max_read_bytes
            ));
        }

        let raw = Self::read_limited(&safe_path, self.max_read_bytes).await?;
        self.validate_file_policy(&raw)?;
        let content = String::from_utf8(raw).context("only UTF-8 text files are supported")?;
        Ok(ToolResult { output: content })
    }
}

#[cfg(test)]
mod tests {
    use super::{ReadFilePolicy, ReadFileTool};
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
            "agentzero-tools-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn read_file_allows_text_within_root() {
        let dir = temp_dir();
        let file = dir.join("note.txt");
        fs::write(&file, "hello").expect("test file should be written");

        let tool = ReadFileTool::new(ReadFilePolicy::default_for_root(dir.clone()));
        let result = tool
            .execute(
                "note.txt",
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("read_file should succeed");

        assert_eq!(result.output, "hello");
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn read_file_rejects_binary_content() {
        let dir = temp_dir();
        let file = dir.join("blob.bin");
        fs::write(&file, vec![0_u8, 159, 146, 150]).expect("binary test file should be written");

        let tool = ReadFileTool::new(ReadFilePolicy::default_for_root(dir.clone()));
        let result = tool
            .execute(
                "blob.bin",
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("binary should be rejected")
            .to_string()
            .contains("binary files are not allowed"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn read_file_rejects_path_traversal_outside_allowlist() {
        let dir = temp_dir();
        let sibling = temp_dir();
        let outside_file = sibling.join("outside.txt");
        fs::write(&outside_file, "nope").expect("outside test file should be written");

        let tool = ReadFileTool::new(ReadFilePolicy::default_for_root(dir.clone()));
        let result = tool
            .execute(
                "../outside.txt",
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("traversal should be denied")
            .to_string()
            .contains("path traversal is not allowed"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
        fs::remove_dir_all(sibling).expect("sibling temp dir should be removed");
    }

    #[tokio::test]
    async fn read_file_rejects_oversized_file() {
        let dir = temp_dir();
        let file = dir.join("large.txt");
        fs::write(&file, "x".repeat(32)).expect("large file should be written");

        let tool = ReadFileTool::new(ReadFilePolicy {
            allowed_root: dir.clone(),
            max_read_bytes: 8,
            allow_binary: false,
        });
        let result = tool
            .execute(
                "large.txt",
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("oversized file should be rejected")
            .to_string()
            .contains("file is too large"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn read_file_rejects_invalid_utf8_as_binary_fallback() {
        let dir = temp_dir();
        let file = dir.join("non_utf8.bin");
        fs::write(&file, vec![0xFF, 0xFE, 0xFD, 0xFC]).expect("binary bytes should be written");

        let tool = ReadFileTool::new(ReadFilePolicy::default_for_root(dir.clone()));
        let result = tool
            .execute(
                "non_utf8.bin",
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("invalid utf8 should be treated as binary")
            .to_string()
            .contains("binary files are not allowed"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    // B7: Hard-link guard tests

    #[cfg(unix)]
    #[tokio::test]
    async fn read_file_rejects_hard_linked_file() {
        let dir = temp_dir();
        let original = dir.join("original.txt");
        fs::write(&original, "secret").expect("original file should be written");
        let link = dir.join("hardlink.txt");
        fs::hard_link(&original, &link).expect("hard link should be created");

        let tool = ReadFileTool::new(ReadFilePolicy::default_for_root(dir.clone()));
        let result = tool
            .execute(
                "hardlink.txt",
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("hard-linked file should be rejected")
            .to_string()
            .contains("hard link"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    // B7: Sensitive file detection tests

    #[tokio::test]
    async fn read_file_blocks_sensitive_path() {
        let dir = temp_dir();
        let sensitive = dir.join(".env");
        fs::write(&sensitive, "SECRET_KEY=abc123").expect("sensitive file should be written");

        let tool = ReadFileTool::new(ReadFilePolicy::default_for_root(dir.clone()));
        let result = tool
            .execute(".env", &ToolContext::new(dir.to_string_lossy().to_string()))
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("sensitive file should be blocked")
            .to_string()
            .contains("refusing to read sensitive file"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn read_file_allows_sensitive_path_when_configured() {
        let dir = temp_dir();
        let sensitive = dir.join(".env");
        fs::write(&sensitive, "SECRET_KEY=abc123").expect("sensitive file should be written");

        let tool = ReadFileTool::new(ReadFilePolicy::default_for_root(dir.clone()));
        let mut ctx = ToolContext::new(dir.to_string_lossy().to_string());
        ctx.allow_sensitive_file_reads = true;
        let result = tool
            .execute(".env", &ctx)
            .await
            .expect("sensitive file should be allowed when configured");

        assert!(result.output.contains("SECRET_KEY=abc123"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn read_file_rejects_symlink_pointing_outside_allowed_root() {
        use std::os::unix::fs as unix_fs;

        let allowed_root = temp_dir();
        let outside_root = temp_dir();
        let outside_file = outside_root.join("outside.txt");
        fs::write(&outside_file, "outside").expect("outside file should be written");

        let link_path = allowed_root.join("link.txt");
        unix_fs::symlink(&outside_file, &link_path).expect("symlink should be created");

        let tool = ReadFileTool::new(ReadFilePolicy::default_for_root(allowed_root.clone()));
        let result = tool
            .execute(
                "link.txt",
                &ToolContext::new(allowed_root.to_string_lossy().to_string()),
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .expect_err("outside symlink should be denied")
            .to_string()
            .contains("path is outside allowed root"));

        fs::remove_dir_all(allowed_root).expect("allowed root temp dir should be removed");
        fs::remove_dir_all(outside_root).expect("outside root temp dir should be removed");
    }
}
