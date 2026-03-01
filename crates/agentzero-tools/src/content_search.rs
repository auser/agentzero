use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

const DEFAULT_LIMIT: usize = 50;
const MAX_LINE_DISPLAY: usize = 200;

#[derive(Debug, Deserialize)]
struct ContentSearchInput {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    case_insensitive: bool,
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ContentSearchTool;

impl ContentSearchTool {
    fn resolve_base(input_path: Option<&str>, workspace_root: &str) -> anyhow::Result<PathBuf> {
        let base = match input_path {
            Some(p) if !p.trim().is_empty() => {
                let rel = Path::new(p);
                if rel.is_absolute() {
                    return Err(anyhow!("absolute paths are not allowed"));
                }
                if rel.components().any(|c| matches!(c, Component::ParentDir)) {
                    return Err(anyhow!("path traversal is not allowed"));
                }
                Path::new(workspace_root).join(rel)
            }
            _ => PathBuf::from(workspace_root),
        };

        let canonical = base
            .canonicalize()
            .with_context(|| format!("unable to resolve search base: {}", base.display()))?;
        let canonical_root = Path::new(workspace_root)
            .canonicalize()
            .context("unable to resolve workspace root")?;
        if !canonical.starts_with(&canonical_root) {
            return Err(anyhow!("search path is outside workspace root"));
        }
        Ok(canonical)
    }

    fn looks_binary(bytes: &[u8]) -> bool {
        let check_len = bytes.len().min(8192);
        bytes[..check_len].contains(&0)
    }

    fn walk_files(
        base: &Path,
        glob_pattern: Option<&str>,
        workspace_root: &Path,
    ) -> anyhow::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        Self::walk_recursive(base, glob_pattern, workspace_root, &mut files)?;
        files.sort();
        Ok(files)
    }

    fn walk_recursive(
        dir: &Path,
        glob_pattern: Option<&str>,
        workspace_root: &Path,
        files: &mut Vec<PathBuf>,
    ) -> anyhow::Result<()> {
        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("unable to read directory: {}", dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden directories and common noise.
            if file_name.starts_with('.')
                || file_name == "node_modules"
                || file_name == "target"
                || file_name == "__pycache__"
            {
                continue;
            }

            if path.is_dir() {
                Self::walk_recursive(&path, glob_pattern, workspace_root, files)?;
            } else if path.is_file() {
                if let Some(pattern) = glob_pattern {
                    let glob = glob::Pattern::new(pattern)
                        .with_context(|| format!("invalid glob pattern: {pattern}"))?;
                    if !glob.matches(&file_name) {
                        continue;
                    }
                }
                // Verify still within workspace.
                if let Ok(canonical) = path.canonicalize() {
                    if canonical.starts_with(workspace_root) {
                        files.push(canonical);
                    }
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Tool for ContentSearchTool {
    fn name(&self) -> &'static str {
        "content_search"
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let request: ContentSearchInput = serde_json::from_str(input).context(
            "content_search expects JSON: {\"pattern\", \"path\"?, \"glob\"?, \"limit\"?, \"case_insensitive\"?}",
        )?;

        if request.pattern.is_empty() {
            return Err(anyhow!("pattern must not be empty"));
        }

        let regex = if request.case_insensitive {
            regex::RegexBuilder::new(&request.pattern)
                .case_insensitive(true)
                .build()
        } else {
            regex::Regex::new(&request.pattern)
        }
        .with_context(|| format!("invalid regex pattern: {}", request.pattern))?;

        let workspace_root = PathBuf::from(&ctx.workspace_root);
        let base = Self::resolve_base(request.path.as_deref(), &ctx.workspace_root)?;
        let canonical_root = workspace_root
            .canonicalize()
            .context("unable to resolve workspace root")?;

        let files = Self::walk_files(&base, request.glob.as_deref(), &canonical_root)?;

        let limit = if request.limit == 0 {
            DEFAULT_LIMIT
        } else {
            request.limit
        };

        let mut results = Vec::new();
        'outer: for file_path in &files {
            let bytes = match std::fs::read(file_path) {
                Ok(b) => b,
                Err(_) => continue,
            };

            if Self::looks_binary(&bytes) {
                continue;
            }

            let content = match std::str::from_utf8(&bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let relative = file_path.strip_prefix(&canonical_root).unwrap_or(file_path);

            for (line_num, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    let display_line = if line.len() > MAX_LINE_DISPLAY {
                        format!("{}...", &line[..MAX_LINE_DISPLAY])
                    } else {
                        line.to_string()
                    };
                    results.push(format!(
                        "{}:{}:{}",
                        relative.display(),
                        line_num + 1,
                        display_line
                    ));
                    if results.len() >= limit {
                        break 'outer;
                    }
                }
            }
        }

        if results.is_empty() {
            return Ok(ToolResult {
                output: "no matches found".to_string(),
            });
        }

        let truncated = results.len() >= limit;
        let mut output = results.join("\n");
        if truncated {
            output.push_str(&format!("\n<truncated at {} results>", limit));
        }

        Ok(ToolResult { output })
    }
}

#[cfg(test)]
mod tests {
    use super::ContentSearchTool;
    use agentzero_core::{Tool, ToolContext};
    use std::fs;
    use std::path::PathBuf;
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
            "agentzero-content-search-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn content_search_finds_matches() {
        let dir = temp_dir();
        fs::write(
            dir.join("main.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        fs::write(dir.join("lib.rs"), "pub fn helper() {}\n").unwrap();

        let tool = ContentSearchTool;
        let result = tool
            .execute(
                r#"{"pattern": "fn \\w+"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("search should succeed");
        assert!(result.output.contains("main.rs:1:fn main()"));
        assert!(result.output.contains("lib.rs:1:pub fn helper()"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn content_search_case_insensitive() {
        let dir = temp_dir();
        fs::write(dir.join("test.txt"), "Hello World\nhello world\n").unwrap();

        let tool = ContentSearchTool;
        let result = tool
            .execute(
                r#"{"pattern": "HELLO", "case_insensitive": true}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("case insensitive search should succeed");
        assert!(result.output.contains(":1:"));
        assert!(result.output.contains(":2:"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn content_search_with_glob_filter() {
        let dir = temp_dir();
        fs::write(dir.join("match.rs"), "fn test() {}\n").unwrap();
        fs::write(dir.join("skip.txt"), "fn test() {}\n").unwrap();

        let tool = ContentSearchTool;
        let result = tool
            .execute(
                r#"{"pattern": "fn test", "glob": "*.rs"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("filtered search should succeed");
        assert!(result.output.contains("match.rs"));
        assert!(!result.output.contains("skip.txt"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn content_search_no_matches() {
        let dir = temp_dir();
        fs::write(dir.join("test.txt"), "nothing relevant here\n").unwrap();

        let tool = ContentSearchTool;
        let result = tool
            .execute(
                r#"{"pattern": "nonexistent_pattern_xyz"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("no matches should succeed");
        assert!(result.output.contains("no matches"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn content_search_rejects_invalid_regex_negative_path() {
        let dir = temp_dir();

        let tool = ContentSearchTool;
        let err = tool
            .execute(
                r#"{"pattern": "[invalid"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("invalid regex should fail");
        assert!(err.to_string().contains("invalid regex"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn content_search_rejects_empty_pattern_negative_path() {
        let dir = temp_dir();

        let tool = ContentSearchTool;
        let err = tool
            .execute(
                r#"{"pattern": ""}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("empty pattern should fail");
        assert!(err.to_string().contains("pattern must not be empty"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn content_search_skips_binary_files() {
        let dir = temp_dir();
        fs::write(dir.join("text.txt"), "searchable content\n").unwrap();
        fs::write(dir.join("binary.bin"), [0u8, 1, 2, 3, 0, 5, 6]).unwrap();

        let tool = ContentSearchTool;
        let result = tool
            .execute(
                r#"{"pattern": "."}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("search should succeed");
        assert!(result.output.contains("text.txt"));
        assert!(!result.output.contains("binary.bin"));
        fs::remove_dir_all(dir).ok();
    }
}
