//! Sandboxed agent execution — pluggable isolation backends.
//!
//! The [`AgentSandbox`] trait abstracts how agents are isolated during workflow
//! execution. [`WorktreeSandbox`] is the default lightweight backend that
//! creates a git worktree per agent. Future backends include container (Docker)
//! and microVM (Firecracker) isolation.

use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Create a `git` command with all inherited git env vars removed.
fn git_cmd() -> Command {
    let mut cmd = Command::new("git");
    cmd.env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

// ── Types ────────────────────────────────────────────────────────────────────

/// Configuration for creating a sandbox.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// The workflow run this sandbox belongs to.
    pub workflow_id: String,
    /// The node this sandbox is created for.
    pub node_id: String,
    /// Root of the main workspace (for worktree base).
    pub workspace_root: PathBuf,
}

/// Handle to a created sandbox — holds paths needed for execution and cleanup.
#[derive(Debug, Clone)]
pub struct SandboxHandle {
    /// Filesystem path to the sandbox workspace.
    pub worktree_path: PathBuf,
    /// Git branch name for this sandbox.
    pub branch_name: String,
    /// Original workspace root (for reference).
    pub workspace_root: PathBuf,
}

/// A task to execute inside a sandbox.
#[derive(Debug, Clone)]
pub struct AgentTask {
    /// Agent node name.
    pub name: String,
    /// Input text for the agent.
    pub input: String,
    /// Metadata from the workflow node.
    pub metadata: serde_json::Value,
}

/// Output from a sandboxed agent execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    /// The agent's response text.
    pub response: String,
    /// Files modified relative to the worktree root.
    pub files_modified: Vec<String>,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Pluggable sandbox backend for isolated agent execution.
#[async_trait]
pub trait AgentSandbox: Send + Sync {
    /// Create a new isolated workspace for an agent.
    async fn create(&self, config: &SandboxConfig) -> anyhow::Result<SandboxHandle>;

    /// Destroy a sandbox, cleaning up all resources.
    async fn destroy(&self, handle: &SandboxHandle) -> anyhow::Result<()>;
}

// ── WorktreeSandbox ──────────────────────────────────────────────────────────

/// Lightweight sandbox using git worktrees for filesystem isolation.
///
/// Each agent gets its own worktree on a unique branch. The agent's tools
/// are scoped to the worktree root. After execution, diffs are collected
/// and the worktree is cleaned up.
#[derive(Debug, Clone)]
pub struct WorktreeSandbox {
    /// Base directory for worktrees (defaults to `{workspace}/.agentzero/worktrees`).
    pub worktree_base: PathBuf,
}

impl WorktreeSandbox {
    /// Create a new worktree sandbox with the given base directory.
    pub fn new(worktree_base: PathBuf) -> Self {
        Self { worktree_base }
    }

    /// Create a sandbox using `{workspace_root}/.agentzero/worktrees` as base.
    pub fn from_workspace(workspace_root: &Path) -> Self {
        Self {
            worktree_base: workspace_root.join(".agentzero").join("worktrees"),
        }
    }

    /// Build the branch name for a given workflow/node pair.
    fn branch_name(workflow_id: &str, node_id: &str) -> String {
        // Sanitize IDs for git branch names
        let wf = workflow_id.replace(['/', ' '], "-");
        let node = node_id.replace(['/', ' '], "-");
        format!("agentzero/wf/{wf}/{node}")
    }
}

#[async_trait]
impl AgentSandbox for WorktreeSandbox {
    async fn create(&self, config: &SandboxConfig) -> anyhow::Result<SandboxHandle> {
        let branch = Self::branch_name(&config.workflow_id, &config.node_id);
        let worktree_path = self.worktree_base.join(&config.node_id);

        // Ensure base directory exists.
        tokio::fs::create_dir_all(&self.worktree_base).await?;

        // Create worktree on a new branch from HEAD.
        let workspace = &config.workspace_root;
        let output = git_cmd()
            .args([
                "worktree",
                "add",
                "-b",
                &branch,
                worktree_path.to_string_lossy().as_ref(),
                "HEAD",
            ])
            .current_dir(workspace)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git worktree add failed: {stderr}");
        }

        tracing::info!(
            workflow_id = %config.workflow_id,
            node_id = %config.node_id,
            branch = %branch,
            path = %worktree_path.display(),
            "created sandbox worktree"
        );

        Ok(SandboxHandle {
            worktree_path,
            branch_name: branch,
            workspace_root: config.workspace_root.clone(),
        })
    }

    async fn destroy(&self, handle: &SandboxHandle) -> anyhow::Result<()> {
        // Remove the worktree.
        let output = git_cmd()
            .args([
                "worktree",
                "remove",
                "--force",
                &handle.worktree_path.to_string_lossy(),
            ])
            .current_dir(&handle.workspace_root)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                branch = %handle.branch_name,
                error = %stderr,
                "git worktree remove failed (may already be cleaned up)"
            );
        }

        // Delete the branch.
        let output = git_cmd()
            .args(["branch", "-D", &handle.branch_name])
            .current_dir(&handle.workspace_root)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                branch = %handle.branch_name,
                error = %stderr,
                "git branch delete failed (may already be deleted)"
            );
        }

        tracing::info!(
            branch = %handle.branch_name,
            path = %handle.worktree_path.display(),
            "destroyed sandbox worktree"
        );

        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a temporary git repo for testing worktrees.
    ///
    /// Removes inherited git env vars to prevent interference from the
    /// outer project repo when tests run under nextest.
    fn create_test_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path();

        git_cmd()
            .args(["init"])
            .current_dir(path)
            .output()
            .expect("git init");
        git_cmd()
            .args(["config", "user.email", "test@test.com"])
            .current_dir(path)
            .output()
            .expect("git config email");
        git_cmd()
            .args(["config", "user.name", "Test"])
            .current_dir(path)
            .output()
            .expect("git config name");

        // Need at least one commit for worktrees to work.
        std::fs::write(path.join("README.md"), "# Test\n").expect("write readme");
        git_cmd()
            .args(["add", "."])
            .current_dir(path)
            .output()
            .expect("git add");
        git_cmd()
            .args(["commit", "-m", "initial"])
            .current_dir(path)
            .output()
            .expect("git commit");

        dir
    }

    #[tokio::test]
    async fn worktree_create_and_destroy_lifecycle() {
        let repo = create_test_repo();
        let workspace = repo.path().to_path_buf();
        let sandbox = WorktreeSandbox::from_workspace(&workspace);

        let config = SandboxConfig {
            workflow_id: "wf-test-1".to_string(),
            node_id: "node-alpha".to_string(),
            workspace_root: workspace.clone(),
        };

        // Create sandbox.
        let handle = sandbox.create(&config).await.expect("create sandbox");
        assert!(
            handle.worktree_path.exists(),
            "worktree directory should exist"
        );
        assert!(
            handle.worktree_path.join("README.md").exists(),
            "worktree should contain repo files"
        );
        assert_eq!(handle.branch_name, "agentzero/wf/wf-test-1/node-alpha");

        // Destroy sandbox.
        sandbox.destroy(&handle).await.expect("destroy sandbox");
        assert!(
            !handle.worktree_path.exists(),
            "worktree directory should be removed"
        );

        // Branch should be deleted.
        let output = git_cmd()
            .args(["branch", "--list", &handle.branch_name])
            .current_dir(&workspace)
            .output()
            .expect("git branch list");
        let branches = String::from_utf8_lossy(&output.stdout);
        assert!(
            !branches.contains(&handle.branch_name),
            "branch should be deleted"
        );
    }

    #[tokio::test]
    async fn worktree_isolation_independent_changes() {
        let repo = create_test_repo();
        let workspace = repo.path().to_path_buf();
        let sandbox = WorktreeSandbox::from_workspace(&workspace);

        // Create two sandboxes.
        let config_a = SandboxConfig {
            workflow_id: "wf-iso".to_string(),
            node_id: "agent-a".to_string(),
            workspace_root: workspace.clone(),
        };
        let config_b = SandboxConfig {
            workflow_id: "wf-iso".to_string(),
            node_id: "agent-b".to_string(),
            workspace_root: workspace.clone(),
        };

        let handle_a = sandbox.create(&config_a).await.expect("create A");
        let handle_b = sandbox.create(&config_b).await.expect("create B");

        // Write different files in each worktree.
        std::fs::write(handle_a.worktree_path.join("file_a.txt"), "from agent A\n")
            .expect("write file_a");
        std::fs::write(handle_b.worktree_path.join("file_b.txt"), "from agent B\n")
            .expect("write file_b");

        // Each worktree should only see its own file.
        assert!(handle_a.worktree_path.join("file_a.txt").exists());
        assert!(!handle_a.worktree_path.join("file_b.txt").exists());
        assert!(handle_b.worktree_path.join("file_b.txt").exists());
        assert!(!handle_b.worktree_path.join("file_a.txt").exists());

        // Cleanup.
        sandbox.destroy(&handle_a).await.expect("destroy A");
        sandbox.destroy(&handle_b).await.expect("destroy B");
    }

    #[test]
    fn branch_name_sanitizes_ids() {
        assert_eq!(
            WorktreeSandbox::branch_name("wf/123", "node 1"),
            "agentzero/wf/wf-123/node-1"
        );
        assert_eq!(
            WorktreeSandbox::branch_name("simple", "alpha"),
            "agentzero/wf/simple/alpha"
        );
    }
}
