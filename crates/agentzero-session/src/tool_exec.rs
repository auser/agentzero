use std::path::Path;

use agentzero_core::{
    Capability, DataClassification, ExecutionId, PolicyDecision, RuntimeTier, ToolId,
};
use agentzero_policy::{PolicyEngine, PolicyRequest};
use agentzero_tracing::{debug, info, warn};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolExecutorError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("tool execution denied: {0}")]
    Denied(String),
    #[error("tool execution failed: {0}")]
    Failed(String),
    #[error("invalid path: {0}")]
    InvalidPath(String),
}

/// Result of a tool execution.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_id: ToolId,
    pub execution_id: ExecutionId,
    pub success: bool,
    pub output: String,
}

/// Supervised tool executor that enforces policy before every invocation.
pub struct ToolExecutor {
    policy: PolicyEngine,
    project_root: Option<String>,
}

impl ToolExecutor {
    /// Create a new tool executor with the given policy engine.
    pub fn new(policy: PolicyEngine) -> Self {
        Self {
            policy,
            project_root: None,
        }
    }

    /// Set the project root for path validation.
    pub fn with_project_root(mut self, root: impl Into<String>) -> Self {
        self.project_root = Some(root.into());
        self
    }

    /// Execute the `read` tool — read file contents.
    pub fn read_file(&self, path: &str) -> Result<ToolResult, ToolExecutorError> {
        let execution_id = ExecutionId::new();
        let tool_id = ToolId::from_string("read");

        debug!(tool = "read", path = path, "checking policy for file read");

        let decision = self.check_policy(Capability::FileRead, DataClassification::Private);
        if !decision.is_allowed() {
            warn!(tool = "read", path = path, "file read denied by policy");
            return Err(ToolExecutorError::Denied(format!(
                "file read denied: {decision:?}"
            )));
        }

        self.validate_path(path)?;

        info!(tool = "read", path = path, "reading file");
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(ToolResult {
                tool_id,
                execution_id,
                success: true,
                output: content,
            }),
            Err(e) => Err(ToolExecutorError::Failed(format!(
                "failed to read {path}: {e}"
            ))),
        }
    }

    /// Execute the `list` tool — list directory contents.
    pub fn list_dir(&self, path: &str) -> Result<ToolResult, ToolExecutorError> {
        let execution_id = ExecutionId::new();
        let tool_id = ToolId::from_string("list");

        debug!(tool = "list", path = path, "checking policy for dir list");

        let decision = self.check_policy(Capability::FileRead, DataClassification::Private);
        if !decision.is_allowed() {
            warn!(tool = "list", path = path, "dir list denied by policy");
            return Err(ToolExecutorError::Denied(format!(
                "dir list denied: {decision:?}"
            )));
        }

        self.validate_path(path)?;

        info!(tool = "list", path = path, "listing directory");
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut lines = Vec::new();
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let file_type = if entry.path().is_dir() { "dir" } else { "file" };
                    lines.push(format!("{file_type}\t{name}"));
                }
                lines.sort();
                Ok(ToolResult {
                    tool_id,
                    execution_id,
                    success: true,
                    output: lines.join("\n"),
                })
            }
            Err(e) => Err(ToolExecutorError::Failed(format!(
                "failed to list {path}: {e}"
            ))),
        }
    }

    /// Execute the `search` tool — search file contents with a pattern.
    pub fn search_files(&self, path: &str, pattern: &str) -> Result<ToolResult, ToolExecutorError> {
        let execution_id = ExecutionId::new();
        let tool_id = ToolId::from_string("search");

        debug!(
            tool = "search",
            path = path,
            pattern = pattern,
            "checking policy for search"
        );

        let decision = self.check_policy(Capability::FileRead, DataClassification::Private);
        if !decision.is_allowed() {
            warn!(tool = "search", path = path, "search denied by policy");
            return Err(ToolExecutorError::Denied(format!(
                "search denied: {decision:?}"
            )));
        }

        self.validate_path(path)?;

        info!(
            tool = "search",
            path = path,
            pattern = pattern,
            "searching files"
        );
        let mut results = Vec::new();
        self.search_recursive(Path::new(path), pattern, &mut results)?;
        results.sort();

        Ok(ToolResult {
            tool_id,
            execution_id,
            success: true,
            output: results.join("\n"),
        })
    }

    /// Propose an edit without writing — returns the proposed diff as output.
    pub fn propose_edit(
        &self,
        path: &str,
        description: &str,
    ) -> Result<ToolResult, ToolExecutorError> {
        let execution_id = ExecutionId::new();
        let tool_id = ToolId::from_string("propose_edit");

        self.validate_path(path)?;

        info!(tool = "propose_edit", path = path, "proposing edit");
        Ok(ToolResult {
            tool_id,
            execution_id,
            success: true,
            output: format!("PROPOSED EDIT for {path}:\n{description}\n\n(edit not applied — requires approval)"),
        })
    }

    /// Write content to a file (requires policy approval).
    pub fn write_file(&self, path: &str, content: &str) -> Result<ToolResult, ToolExecutorError> {
        let execution_id = ExecutionId::new();
        let tool_id = ToolId::from_string("write");

        debug!(
            tool = "write",
            path = path,
            "checking policy for file write"
        );

        let decision = self.check_policy(Capability::FileWrite, DataClassification::Private);
        if !decision.is_allowed() {
            return match decision {
                PolicyDecision::RequiresApproval { reason } => Err(ToolExecutorError::Denied(
                    format!("file write requires approval: {reason}"),
                )),
                _ => Err(ToolExecutorError::Denied(format!(
                    "file write denied: {decision:?}"
                ))),
            };
        }

        self.validate_path(path)?;

        info!(
            tool = "write",
            path = path,
            bytes = content.len(),
            "writing file"
        );
        match std::fs::write(path, content) {
            Ok(()) => Ok(ToolResult {
                tool_id,
                execution_id,
                success: true,
                output: format!("wrote {} bytes to {path}", content.len()),
            }),
            Err(e) => Err(ToolExecutorError::Failed(format!(
                "failed to write {path}: {e}"
            ))),
        }
    }

    /// Execute a shell command (requires policy approval).
    pub fn shell_command(&self, command: &str) -> Result<ToolResult, ToolExecutorError> {
        let execution_id = ExecutionId::new();
        let tool_id = ToolId::from_string("shell");

        debug!(
            tool = "shell",
            command = command,
            "checking policy for shell command"
        );

        let decision = self.check_policy(Capability::ShellCommand, DataClassification::Private);
        if !decision.is_allowed() {
            return match decision {
                PolicyDecision::RequiresApproval { reason } => Err(ToolExecutorError::Denied(
                    format!("shell command requires approval: {reason}"),
                )),
                _ => Err(ToolExecutorError::Denied(format!(
                    "shell command denied: {decision:?}"
                ))),
            };
        }

        info!(tool = "shell", command = command, "executing shell command");
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .map_err(|e| ToolExecutorError::Failed(format!("failed to execute: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let combined = if stderr.is_empty() {
            stdout
        } else {
            format!("{stdout}\nSTDERR:\n{stderr}")
        };

        Ok(ToolResult {
            tool_id,
            execution_id,
            success: output.status.success(),
            output: combined,
        })
    }

    fn check_policy(
        &self,
        capability: Capability,
        classification: DataClassification,
    ) -> PolicyDecision {
        let request = PolicyRequest {
            capability,
            classification,
            runtime: RuntimeTier::HostReadonly,
            context: String::new(),
        };
        self.policy.evaluate(&request)
    }

    fn validate_path(&self, path: &str) -> Result<(), ToolExecutorError> {
        let canonical = std::fs::canonicalize(path)
            .map_err(|e| ToolExecutorError::InvalidPath(format!("{path}: {e}")))?;

        // Block path traversal outside project root if one is set
        if let Some(ref root) = self.project_root {
            let root_canonical = std::fs::canonicalize(root)
                .map_err(|e| ToolExecutorError::InvalidPath(format!("{root}: {e}")))?;
            if !canonical.starts_with(&root_canonical) {
                return Err(ToolExecutorError::Denied(format!(
                    "path {path} is outside project root"
                )));
            }
        }

        // Block obvious sensitive paths
        let path_str = canonical.to_string_lossy();
        let sensitive = [".ssh", ".gnupg", ".aws/credentials", ".env"];
        for s in &sensitive {
            if path_str.contains(s) {
                return Err(ToolExecutorError::Denied(format!(
                    "access to sensitive path denied: {s}"
                )));
            }
        }

        Ok(())
    }

    fn search_recursive(
        &self,
        dir: &Path,
        pattern: &str,
        results: &mut Vec<String>,
    ) -> Result<(), ToolExecutorError> {
        if !dir.is_dir() {
            return Ok(());
        }

        let entries = std::fs::read_dir(dir).map_err(|e| {
            ToolExecutorError::Failed(format!("failed to read {}: {e}", dir.display()))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();

            // Skip hidden directories and target/
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
            }

            if path.is_dir() {
                self.search_recursive(&path, pattern, results)?;
            } else if path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for (line_num, line) in content.lines().enumerate() {
                        if line.contains(pattern) {
                            results.push(format!(
                                "{}:{}:{}",
                                path.display(),
                                line_num + 1,
                                line.trim()
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_policy::PolicyRule;

    fn executor_with_read_allowed() -> ToolExecutor {
        let policy = PolicyEngine::with_rules(vec![PolicyRule::allow(
            Capability::FileRead,
            DataClassification::Private,
        )]);
        ToolExecutor::new(policy)
    }

    fn executor_deny_all() -> ToolExecutor {
        ToolExecutor::new(PolicyEngine::deny_by_default())
    }

    #[test]
    fn read_file_with_allowed_policy() {
        let executor = executor_with_read_allowed();
        let result = executor.read_file("Cargo.toml");
        assert!(result.is_ok());
        let result = result.expect("should succeed");
        assert!(result.success);
        assert!(result.output.contains("[package]"));
    }

    #[test]
    fn read_file_denied_by_policy() {
        let executor = executor_deny_all();
        let result = executor.read_file("Cargo.toml");
        assert!(result.is_err());
        match result {
            Err(ToolExecutorError::Denied(_)) => {}
            other => panic!("expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn read_nonexistent_file_fails() {
        let executor = executor_with_read_allowed();
        let result = executor.read_file("nonexistent-file-abc123.txt");
        assert!(result.is_err());
    }

    #[test]
    fn list_dir_with_allowed_policy() {
        let executor = executor_with_read_allowed();
        let result = executor.list_dir(".");
        assert!(result.is_ok());
        let result = result.expect("should succeed");
        assert!(result.output.contains("Cargo.toml"));
    }

    #[test]
    fn list_dir_denied_by_policy() {
        let executor = executor_deny_all();
        let result = executor.list_dir(".");
        assert!(result.is_err());
    }

    #[test]
    fn search_files_with_allowed_policy() {
        let executor = executor_with_read_allowed();
        let result = executor.search_files("src", "agentzero");
        assert!(result.is_ok());
        let result = result.expect("should succeed");
        assert!(result.output.contains("agentzero"));
    }

    #[test]
    fn propose_edit_returns_description() {
        let executor = executor_with_read_allowed();
        let result = executor
            .propose_edit("src/lib.rs", "Add a new module")
            .expect("should succeed");
        assert!(result.output.contains("PROPOSED EDIT"));
        assert!(result.output.contains("requires approval"));
    }

    #[test]
    fn path_traversal_blocked_with_project_root() {
        let executor = executor_with_read_allowed().with_project_root("crates/agentzero-core");
        // Trying to read outside the project root
        let result = executor.read_file("Cargo.toml");
        assert!(result.is_err());
    }
}
