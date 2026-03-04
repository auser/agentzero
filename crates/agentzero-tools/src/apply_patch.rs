use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
use tokio::fs;

const BEGIN_PATCH: &str = "*** Begin Patch";
const END_PATCH: &str = "*** End Patch";
const UPDATE_FILE: &str = "*** Update File: ";
const ADD_FILE: &str = "*** Add File: ";
const DELETE_FILE: &str = "*** Delete File: ";

#[derive(Debug, Clone)]
struct PatchFile {
    path: String,
    op: PatchOp,
}

#[derive(Debug, Clone)]
enum PatchOp {
    Update(Vec<Hunk>),
    Add(String),
    Delete,
}

#[derive(Debug, Clone)]
struct Hunk {
    context_before: Vec<String>,
    removals: Vec<String>,
    additions: Vec<String>,
    #[allow(dead_code)]
    context_after: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApplyPatchInput {
    patch: String,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ApplyPatchTool;

impl ApplyPatchTool {
    pub fn validate_patch(&self, patch: &str) -> anyhow::Result<()> {
        let trimmed = patch.trim();
        if trimmed.is_empty() {
            anyhow::bail!("patch must not be empty");
        }
        let first = trimmed
            .lines()
            .next()
            .context("patch must include a begin marker")?;
        if first != BEGIN_PATCH {
            anyhow::bail!("patch must start with `{BEGIN_PATCH}`");
        }
        if !trimmed.lines().any(|line| line == END_PATCH) {
            anyhow::bail!("patch must end with `{END_PATCH}`");
        }
        Ok(())
    }

    fn parse_patch(patch: &str) -> anyhow::Result<Vec<PatchFile>> {
        let trimmed = patch.trim();
        let lines: Vec<&str> = trimmed.lines().collect();
        if lines.is_empty() || lines[0] != BEGIN_PATCH {
            anyhow::bail!("patch must start with `{BEGIN_PATCH}`");
        }

        let mut files = Vec::new();
        let mut i = 1;

        while i < lines.len() {
            let line = lines[i];

            if line == END_PATCH {
                break;
            }

            if let Some(path) = line.strip_prefix(UPDATE_FILE) {
                let path = path.trim().to_string();
                i += 1;
                let mut hunks = Vec::new();

                while i < lines.len()
                    && lines[i] != END_PATCH
                    && !lines[i].starts_with(UPDATE_FILE)
                    && !lines[i].starts_with(ADD_FILE)
                    && !lines[i].starts_with(DELETE_FILE)
                {
                    if lines[i] == "@@" {
                        i += 1;
                        let mut context_before = Vec::new();
                        let mut removals = Vec::new();
                        let mut additions = Vec::new();
                        let mut context_after = Vec::new();
                        let mut seen_change = false;

                        while i < lines.len()
                            && lines[i] != "@@"
                            && lines[i] != END_PATCH
                            && !lines[i].starts_with(UPDATE_FILE)
                            && !lines[i].starts_with(ADD_FILE)
                            && !lines[i].starts_with(DELETE_FILE)
                        {
                            if let Some(removed) = lines[i].strip_prefix('-') {
                                seen_change = true;
                                removals.push(removed.to_string());
                            } else if let Some(added) = lines[i].strip_prefix('+') {
                                seen_change = true;
                                additions.push(added.to_string());
                            } else if lines[i].starts_with(' ') || lines[i].is_empty() {
                                let ctx_line = if lines[i].starts_with(' ') {
                                    lines[i][1..].to_string()
                                } else {
                                    String::new()
                                };
                                if seen_change {
                                    context_after.push(ctx_line);
                                } else {
                                    context_before.push(ctx_line);
                                }
                            }
                            i += 1;
                        }

                        hunks.push(Hunk {
                            context_before,
                            removals,
                            additions,
                            context_after,
                        });
                    } else {
                        i += 1;
                    }
                }

                files.push(PatchFile {
                    path,
                    op: PatchOp::Update(hunks),
                });
            } else if let Some(path) = line.strip_prefix(ADD_FILE) {
                let path = path.trim().to_string();
                i += 1;
                let mut content = Vec::new();

                while i < lines.len()
                    && lines[i] != END_PATCH
                    && !lines[i].starts_with(UPDATE_FILE)
                    && !lines[i].starts_with(ADD_FILE)
                    && !lines[i].starts_with(DELETE_FILE)
                {
                    if let Some(added) = lines[i].strip_prefix('+') {
                        content.push(added.to_string());
                    }
                    i += 1;
                }

                files.push(PatchFile {
                    path,
                    op: PatchOp::Add(content.join("\n") + "\n"),
                });
            } else if let Some(path) = line.strip_prefix(DELETE_FILE) {
                let path = path.trim().to_string();
                i += 1;
                // Skip any remaining lines in this section
                while i < lines.len()
                    && lines[i] != END_PATCH
                    && !lines[i].starts_with(UPDATE_FILE)
                    && !lines[i].starts_with(ADD_FILE)
                    && !lines[i].starts_with(DELETE_FILE)
                {
                    i += 1;
                }
                files.push(PatchFile {
                    path,
                    op: PatchOp::Delete,
                });
            } else {
                i += 1;
            }
        }

        if files.is_empty() {
            anyhow::bail!("patch contains no file operations");
        }

        Ok(files)
    }

    fn apply_hunks(content: &str, hunks: &[Hunk]) -> anyhow::Result<String> {
        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

        // Apply hunks in reverse order to preserve line numbers.
        for (hunk_idx, hunk) in hunks.iter().enumerate().rev() {
            let match_pos = Self::find_hunk_position(&lines, hunk).with_context(|| {
                format!(
                    "hunk {} could not be matched against the file content",
                    hunk_idx + 1
                )
            })?;

            let remove_start = match_pos + hunk.context_before.len();
            let remove_end = remove_start + hunk.removals.len();

            // Verify removal lines match.
            for (j, removal) in hunk.removals.iter().enumerate() {
                let line_idx = remove_start + j;
                if line_idx >= lines.len() || lines[line_idx] != *removal {
                    let actual = if line_idx < lines.len() {
                        &lines[line_idx]
                    } else {
                        "<past end of file>"
                    };
                    anyhow::bail!(
                        "hunk {} removal mismatch at line {}: expected {:?}, found {:?}",
                        hunk_idx + 1,
                        line_idx + 1,
                        removal,
                        actual,
                    );
                }
            }

            // Replace: remove old lines and insert new ones.
            lines.splice(remove_start..remove_end, hunk.additions.iter().cloned());
        }

        let mut result = lines.join("\n");
        // Preserve trailing newline if original had one.
        if content.ends_with('\n') && !result.ends_with('\n') {
            result.push('\n');
        }
        Ok(result)
    }

    fn find_hunk_position(lines: &[String], hunk: &Hunk) -> anyhow::Result<usize> {
        if hunk.context_before.is_empty() && hunk.removals.is_empty() {
            // No context or removals — insert at the end.
            return Ok(lines.len());
        }

        let match_lines: Vec<&str> = hunk
            .context_before
            .iter()
            .chain(hunk.removals.iter())
            .map(|s| s.as_str())
            .collect();

        if match_lines.is_empty() {
            return Ok(0);
        }

        for start in 0..=lines.len().saturating_sub(match_lines.len()) {
            let matched = match_lines
                .iter()
                .enumerate()
                .all(|(j, expected)| start + j < lines.len() && lines[start + j] == *expected);
            if matched {
                return Ok(start);
            }
        }

        anyhow::bail!("could not locate hunk context in file")
    }

    fn resolve_path(
        input_path: &str,
        workspace_root: &str,
        allowed_root: &Path,
    ) -> anyhow::Result<PathBuf> {
        if input_path.trim().is_empty() {
            return Err(anyhow!("file path is required"));
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
            .ok_or_else(|| anyhow!("path must target a file"))?
            .to_os_string();
        let parent = joined
            .parent()
            .ok_or_else(|| anyhow!("path must have a parent directory"))?;

        // For new files, the parent might not exist yet — create it.
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent directory for {input_path}"))?;
        }

        let canonical_parent = parent
            .canonicalize()
            .with_context(|| format!("unable to resolve path parent: {input_path}"))?;
        let canonical_allowed_root = allowed_root
            .canonicalize()
            .context("unable to resolve allowed root")?;
        if !canonical_parent.starts_with(&canonical_allowed_root) {
            return Err(anyhow!("path is outside allowed root"));
        }
        Ok(canonical_parent.join(file_name))
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &'static str {
        "apply_patch"
    }

    fn description(&self) -> &'static str {
        "Apply a unified patch to one or more files. Supports update, add, and delete operations with context-based matching and dry-run mode."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "The patch text in unified diff format"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, show what would change without modifying files"
                }
            },
            "required": ["patch"]
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let request: ApplyPatchInput = serde_json::from_str(input)
            .context("apply_patch expects JSON: {\"patch\": \"...\", \"dry_run\": false}")?;

        self.validate_patch(&request.patch)?;
        let patch_files = Self::parse_patch(&request.patch)?;

        let allowed_root = PathBuf::from(&ctx.workspace_root);
        let mut results = Vec::new();

        for pf in &patch_files {
            let dest = Self::resolve_path(&pf.path, &ctx.workspace_root, &allowed_root)?;

            // B7: Hard-link guard.
            if dest.exists() {
                agentzero_autonomy::AutonomyPolicy::check_hard_links(&dest.to_string_lossy())?;
            }

            // B7: Sensitive file detection.
            if !ctx.allow_sensitive_file_writes
                && agentzero_autonomy::is_sensitive_path(&dest.to_string_lossy())
            {
                return Err(anyhow!(
                    "refusing to patch sensitive file: {}",
                    dest.display()
                ));
            }

            match &pf.op {
                PatchOp::Update(hunks) => {
                    let content = fs::read_to_string(&dest)
                        .await
                        .with_context(|| format!("failed to read file: {}", pf.path))?;
                    let updated = Self::apply_hunks(&content, hunks)?;

                    if request.dry_run {
                        results.push(format!("update {} (dry_run)", pf.path));
                    } else {
                        fs::write(&dest, &updated)
                            .await
                            .with_context(|| format!("failed to write file: {}", pf.path))?;
                        results.push(format!("updated {}", pf.path));
                    }
                }
                PatchOp::Add(content) => {
                    if request.dry_run {
                        results.push(format!(
                            "add {} ({} bytes, dry_run)",
                            pf.path,
                            content.len()
                        ));
                    } else {
                        fs::write(&dest, content)
                            .await
                            .with_context(|| format!("failed to create file: {}", pf.path))?;
                        results.push(format!("added {}", pf.path));
                    }
                }
                PatchOp::Delete => {
                    if request.dry_run {
                        results.push(format!("delete {} (dry_run)", pf.path));
                    } else {
                        fs::remove_file(&dest)
                            .await
                            .with_context(|| format!("failed to delete file: {}", pf.path))?;
                        results.push(format!("deleted {}", pf.path));
                    }
                }
            }
        }

        Ok(ToolResult {
            output: results.join("\n"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ApplyPatchTool;
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
            "agentzero-apply-patch-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn validate_patch_accepts_basic_envelope_success_path() {
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Update File: test.txt\n@@\n-old\n+new\n*** End Patch\n";
        tool.validate_patch(patch)
            .expect("well-formed patch should validate");
    }

    #[test]
    fn validate_patch_rejects_missing_begin_marker_negative_path() {
        let tool = ApplyPatchTool;
        let err = tool
            .validate_patch("*** Update File: test.txt\n*** End Patch\n")
            .expect_err("missing begin marker should fail");
        assert!(err.to_string().contains("patch must start with"));
    }

    #[tokio::test]
    async fn apply_patch_single_file_single_hunk() {
        let dir = temp_dir();
        fs::write(dir.join("hello.txt"), "line1\nline2\nline3\n").unwrap();

        let patch = r#"{"patch": "*** Begin Patch\n*** Update File: hello.txt\n@@\n line1\n-line2\n+line2_modified\n line3\n*** End Patch"}"#;
        let tool = ApplyPatchTool;
        let result = tool
            .execute(patch, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect("patch should apply");
        assert!(result.output.contains("updated hello.txt"));

        let content = fs::read_to_string(dir.join("hello.txt")).unwrap();
        assert!(content.contains("line2_modified"));
        assert!(!content.contains("\nline2\n"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn apply_patch_dry_run_does_not_modify() {
        let dir = temp_dir();
        fs::write(dir.join("hello.txt"), "line1\nline2\n").unwrap();

        let patch = r#"{"patch": "*** Begin Patch\n*** Update File: hello.txt\n@@\n-line2\n+changed\n*** End Patch", "dry_run": true}"#;
        let tool = ApplyPatchTool;
        let result = tool
            .execute(patch, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect("dry_run should succeed");
        assert!(result.output.contains("dry_run"));

        let content = fs::read_to_string(dir.join("hello.txt")).unwrap();
        assert!(content.contains("line2"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn apply_patch_add_file() {
        let dir = temp_dir();

        let patch = r#"{"patch": "*** Begin Patch\n*** Add File: new.txt\n+hello world\n+second line\n*** End Patch"}"#;
        let tool = ApplyPatchTool;
        let result = tool
            .execute(patch, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect("add file should succeed");
        assert!(result.output.contains("added new.txt"));

        let content = fs::read_to_string(dir.join("new.txt")).unwrap();
        assert!(content.contains("hello world"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn apply_patch_delete_file() {
        let dir = temp_dir();
        fs::write(dir.join("doomed.txt"), "goodbye").unwrap();

        let patch = r#"{"patch": "*** Begin Patch\n*** Delete File: doomed.txt\n*** End Patch"}"#;
        let tool = ApplyPatchTool;
        let result = tool
            .execute(patch, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect("delete should succeed");
        assert!(result.output.contains("deleted doomed.txt"));
        assert!(!dir.join("doomed.txt").exists());
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn apply_patch_rejects_path_traversal_negative_path() {
        let dir = temp_dir();

        let patch =
            r#"{"patch": "*** Begin Patch\n*** Add File: ../escape.txt\n+evil\n*** End Patch"}"#;
        let tool = ApplyPatchTool;
        let err = tool
            .execute(patch, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect_err("path traversal should be denied");
        assert!(err.to_string().contains("path traversal"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn apply_patch_rejects_missing_context_negative_path() {
        let dir = temp_dir();
        fs::write(dir.join("hello.txt"), "aaa\nbbb\nccc\n").unwrap();

        let patch = r#"{"patch": "*** Begin Patch\n*** Update File: hello.txt\n@@\n nonexistent_context\n-bbb\n+replaced\n*** End Patch"}"#;
        let tool = ApplyPatchTool;
        let err = tool
            .execute(patch, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect_err("missing context should fail");
        assert!(err.to_string().contains("could not be matched"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn apply_patch_rejects_sensitive_file_negative_path() {
        let dir = temp_dir();

        let patch = r#"{"patch": "*** Begin Patch\n*** Add File: .env\n+SECRET=x\n*** End Patch"}"#;
        let tool = ApplyPatchTool;
        let err = tool
            .execute(patch, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect_err("sensitive file should be blocked");
        assert!(err.to_string().contains("refusing to patch sensitive file"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn apply_patch_multi_file() {
        let dir = temp_dir();
        fs::write(dir.join("a.txt"), "alpha\nbeta\n").unwrap();
        fs::write(dir.join("b.txt"), "one\ntwo\n").unwrap();

        let patch = r#"{"patch": "*** Begin Patch\n*** Update File: a.txt\n@@\n-beta\n+BETA\n*** Update File: b.txt\n@@\n-two\n+TWO\n*** End Patch"}"#;
        let tool = ApplyPatchTool;
        let result = tool
            .execute(patch, &ToolContext::new(dir.to_string_lossy().to_string()))
            .await
            .expect("multi-file patch should apply");
        assert!(result.output.contains("updated a.txt"));
        assert!(result.output.contains("updated b.txt"));

        assert!(fs::read_to_string(dir.join("a.txt"))
            .unwrap()
            .contains("BETA"));
        assert!(fs::read_to_string(dir.join("b.txt"))
            .unwrap()
            .contains("TWO"));
        fs::remove_dir_all(dir).ok();
    }
}
