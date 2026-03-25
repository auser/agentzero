use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

const DEFAULT_LIMIT: usize = 100;

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct GlobSearchInput {
    /// Glob pattern to match (e.g. "**/*.rs", "src/*.ts")
    pattern: String,
    /// Subdirectory to search within (optional, defaults to workspace root)
    #[serde(default)]
    path: Option<String>,
    /// Maximum number of results to return (default: 100)
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[tool(
    name = "glob_search",
    description = "Search for files matching a glob pattern within the workspace. Returns a list of matching file paths."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct GlobSearchTool;

impl GlobSearchTool {
    fn resolve_base(
        input_path: Option<&str>,
        workspace_root: &str,
        allowed_root: &Path,
    ) -> anyhow::Result<PathBuf> {
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
        let canonical_root = allowed_root
            .canonicalize()
            .context("unable to resolve allowed root")?;
        if !canonical.starts_with(&canonical_root) {
            return Err(anyhow!("search path is outside allowed root"));
        }
        Ok(canonical)
    }
}

#[async_trait]
impl Tool for GlobSearchTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(GlobSearchInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let request: GlobSearchInput = serde_json::from_str(input)
            .context("glob_search expects JSON: {\"pattern\", \"path\"?, \"limit\"?}")?;

        if request.pattern.trim().is_empty() {
            return Err(anyhow!("pattern must not be empty"));
        }

        let workspace_root = PathBuf::from(&ctx.workspace_root);
        let base = Self::resolve_base(
            request.path.as_deref(),
            &ctx.workspace_root,
            &workspace_root,
        )?;

        let full_pattern = base.join(&request.pattern);
        let pattern_str = full_pattern.to_string_lossy().to_string();

        let entries = glob::glob(&pattern_str)
            .with_context(|| format!("invalid glob pattern: {}", request.pattern))?;

        let canonical_root = workspace_root
            .canonicalize()
            .context("unable to resolve workspace root")?;

        let limit = if request.limit == 0 {
            DEFAULT_LIMIT
        } else {
            request.limit
        };

        let mut results = Vec::new();
        for entry in entries {
            if results.len() >= limit {
                break;
            }
            match entry {
                Ok(path) => {
                    // Only include files within workspace.
                    if let Ok(canonical) = path.canonicalize() {
                        if canonical.starts_with(&canonical_root) {
                            let relative = canonical
                                .strip_prefix(&canonical_root)
                                .unwrap_or(&canonical);
                            results.push(relative.to_string_lossy().to_string());
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        results.sort();

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
    use super::GlobSearchTool;
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
            "agentzero-glob-search-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn glob_search_finds_matching_files() {
        let dir = temp_dir();
        fs::write(dir.join("foo.rs"), "").unwrap();
        fs::write(dir.join("bar.rs"), "").unwrap();
        fs::write(dir.join("baz.txt"), "").unwrap();

        let tool = GlobSearchTool;
        let result = tool
            .execute(
                r#"{"pattern": "*.rs"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("glob should succeed");
        assert!(result.output.contains("foo.rs"));
        assert!(result.output.contains("bar.rs"));
        assert!(!result.output.contains("baz.txt"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn glob_search_respects_limit() {
        let dir = temp_dir();
        for i in 0..10 {
            fs::write(dir.join(format!("file{i}.txt")), "").unwrap();
        }

        let tool = GlobSearchTool;
        let result = tool
            .execute(
                r#"{"pattern": "*.txt", "limit": 3}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("glob should succeed");
        assert!(result.output.contains("truncated at 3"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn glob_search_no_matches() {
        let dir = temp_dir();

        let tool = GlobSearchTool;
        let result = tool
            .execute(
                r#"{"pattern": "*.nonexistent"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("no matches should succeed");
        assert!(result.output.contains("no matches"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn glob_search_rejects_empty_pattern_negative_path() {
        let dir = temp_dir();

        let tool = GlobSearchTool;
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
    async fn glob_search_rejects_path_traversal_negative_path() {
        let dir = temp_dir();

        let tool = GlobSearchTool;
        let err = tool
            .execute(
                r#"{"pattern": "*.txt", "path": "../"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("path traversal should be denied");
        assert!(err.to_string().contains("path traversal"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn glob_search_recursive_pattern() {
        let dir = temp_dir();
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.join("top.rs"), "").unwrap();
        fs::write(sub.join("nested.rs"), "").unwrap();

        let tool = GlobSearchTool;
        let result = tool
            .execute(
                r#"{"pattern": "**/*.rs"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("recursive glob should succeed");
        assert!(result.output.contains("top.rs"));
        assert!(result.output.contains("nested.rs"));
        fs::remove_dir_all(dir).ok();
    }
}
