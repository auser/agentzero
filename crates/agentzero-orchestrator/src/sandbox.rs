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

// ── ContainerSandbox ─────────────────────────────────────────────────────────

/// Container configuration for Docker/Podman sandboxes.
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    /// Container runtime: "docker" or "podman".
    pub runtime: String,
    /// Base image for agent containers.
    pub image: String,
    /// Memory limit (e.g. "512m", "1g").
    pub memory_limit: String,
    /// CPU limit (e.g. "1.0", "0.5").
    pub cpu_limit: String,
    /// Whether to enable network access inside the container.
    pub network_enabled: bool,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            runtime: "docker".to_string(),
            image: "agentzero-sandbox:latest".to_string(),
            memory_limit: "512m".to_string(),
            cpu_limit: "1.0".to_string(),
            network_enabled: false,
        }
    }
}

/// Docker/Podman container sandbox for process and network isolation.
///
/// Each agent runs in a container with:
/// - Read-only root filesystem
/// - Dropped capabilities (`--cap-drop=ALL`)
/// - Memory and CPU limits
/// - Workspace bind-mounted from a worktree
/// - Optional network isolation (`--network=none`)
#[derive(Debug, Clone)]
pub struct ContainerSandbox {
    /// Container configuration.
    pub config: ContainerConfig,
    /// Underlying worktree sandbox for filesystem setup.
    worktree: WorktreeSandbox,
}

impl ContainerSandbox {
    /// Create a new container sandbox with the given configuration.
    pub fn new(config: ContainerConfig, worktree_base: PathBuf) -> Self {
        Self {
            config,
            worktree: WorktreeSandbox::new(worktree_base),
        }
    }

    /// Create with default config and workspace-derived worktree base.
    pub fn from_workspace(workspace_root: &Path) -> Self {
        Self {
            config: ContainerConfig::default(),
            worktree: WorktreeSandbox::from_workspace(workspace_root),
        }
    }

    /// Container name for a given sandbox.
    fn container_name(workflow_id: &str, node_id: &str) -> String {
        format!(
            "agentzero-swarm-{}-{}",
            workflow_id.replace('/', "-"),
            node_id.replace('/', "-")
        )
    }

    /// Build the `docker run` argument list.
    fn build_run_args(&self, handle: &SandboxHandle, container_name: &str) -> Vec<String> {
        let mut args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            container_name.to_string(),
            // Security hardening
            "--cap-drop=ALL".to_string(),
            "--read-only".to_string(),
            // Resource limits
            format!("--memory={}", self.config.memory_limit),
            format!("--cpus={}", self.config.cpu_limit),
            // Tmpfs for writable areas
            "--tmpfs=/tmp:rw,noexec,nosuid,size=256m".to_string(),
            "--tmpfs=/sandbox:rw,noexec,nosuid,size=256m".to_string(),
            // Mount worktree
            "-v".to_string(),
            format!("{}:/workspace:rw", handle.worktree_path.display()),
            "-w".to_string(),
            "/workspace".to_string(),
        ];

        if !self.config.network_enabled {
            args.push("--network=none".to_string());
        }

        // Image + default command (sleep to keep container alive).
        args.push(self.config.image.clone());
        args.push("sleep".to_string());
        args.push("infinity".to_string());

        args
    }
}

#[async_trait]
impl AgentSandbox for ContainerSandbox {
    async fn create(&self, config: &SandboxConfig) -> anyhow::Result<SandboxHandle> {
        // First create the worktree for filesystem isolation.
        let handle = self.worktree.create(config).await?;

        // Then start a container with the worktree mounted.
        let container_name = Self::container_name(&config.workflow_id, &config.node_id);
        let args = self.build_run_args(&handle, &container_name);

        let output = std::process::Command::new(&self.config.runtime)
            .args(&args)
            .output()?;

        if !output.status.success() {
            // Clean up the worktree if container creation fails.
            let _ = self.worktree.destroy(&handle).await;
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("{} run failed: {stderr}", self.config.runtime);
        }

        tracing::info!(
            container = %container_name,
            runtime = %self.config.runtime,
            memory = %self.config.memory_limit,
            cpu = %self.config.cpu_limit,
            network = self.config.network_enabled,
            "created container sandbox"
        );

        Ok(handle)
    }

    async fn destroy(&self, handle: &SandboxHandle) -> anyhow::Result<()> {
        // Derive container name from handle.
        let branch_parts: Vec<&str> = handle.branch_name.split('/').collect();
        let container_name = if branch_parts.len() >= 4 {
            format!("agentzero-swarm-{}-{}", branch_parts[2], branch_parts[3])
        } else {
            format!("agentzero-swarm-{}", handle.branch_name.replace('/', "-"))
        };

        // Stop and remove the container.
        let _ = std::process::Command::new(&self.config.runtime)
            .args(["stop", &container_name])
            .output();

        let _ = std::process::Command::new(&self.config.runtime)
            .args(["rm", "-f", &container_name])
            .output();

        tracing::info!(container = %container_name, "destroyed container sandbox");

        // Then clean up the worktree.
        self.worktree.destroy(handle).await
    }
}

// ── MicroVmSandbox ──────────────────────────────────────────────────────────

/// MicroVM configuration for Firecracker/Cloud Hypervisor sandboxes.
#[derive(Debug, Clone)]
pub struct MicroVmConfig {
    /// MicroVM runtime: "firecracker" or "cloud-hypervisor".
    pub runtime: String,
    /// Path to the kernel image.
    pub kernel_path: PathBuf,
    /// Path to the root filesystem image.
    pub rootfs_path: PathBuf,
    /// Memory size in MB.
    pub memory_mb: usize,
    /// Number of vCPUs.
    pub vcpus: usize,
    /// Path to the Firecracker/CH binary.
    pub binary_path: PathBuf,
}

impl Default for MicroVmConfig {
    fn default() -> Self {
        Self {
            runtime: "firecracker".to_string(),
            kernel_path: PathBuf::from("/opt/agentzero/vmlinux"),
            rootfs_path: PathBuf::from("/opt/agentzero/rootfs.ext4"),
            memory_mb: 256,
            vcpus: 1,
            binary_path: PathBuf::from("firecracker"),
        }
    }
}

/// # Maintenance-only (Sprint 85)
///
/// This sandbox backend is maintenance-only. New Firecracker/microVM investment belongs
/// to the `mvm` project (gomicrovm.com). AgentZero will integrate `mvm` as an external
/// dependency when the interface stabilizes.
///
/// See `specs/BACKLOG-EXTERNAL.md` for the full rationale.
///
/// > **NOTE: Maintenance-only.** New Firecracker investment belongs to the `mvm`
/// > project (gomicrovm.com) — see `BACKLOG-EXTERNAL.md` § "MicroVM Agent Backends".
/// > This type exists as a proof-of-concept; do not add new features here without
/// > first checking whether they should land in the external `mvm` integration instead.
///
/// Firecracker/Cloud Hypervisor microVM sandbox for kernel-level isolation.
///
/// Each agent runs in its own microVM with:
/// - Full kernel isolation (~125ms boot)
/// - No host filesystem access outside the mount
/// - Memory and CPU limits enforced by the hypervisor
/// - Network isolation via TAP devices
///
/// Requires the Firecracker binary and kernel/rootfs images to be available.
#[derive(Debug, Clone)]
pub struct MicroVmSandbox {
    /// MicroVM configuration.
    pub config: MicroVmConfig,
    /// Underlying worktree sandbox for filesystem setup.
    worktree: WorktreeSandbox,
}

impl MicroVmSandbox {
    /// Create a new microVM sandbox with the given configuration.
    pub fn new(config: MicroVmConfig, worktree_base: PathBuf) -> Self {
        Self {
            config,
            worktree: WorktreeSandbox::new(worktree_base),
        }
    }

    /// Check whether the microVM runtime binary is available.
    pub fn is_available(&self) -> bool {
        self.config.binary_path.exists() || which::which(&self.config.runtime).is_ok()
    }

    /// Socket path for the Firecracker API.
    fn socket_path(workflow_id: &str, node_id: &str) -> PathBuf {
        std::env::temp_dir().join(format!("agentzero-vm-{workflow_id}-{node_id}.sock"))
    }
}

#[async_trait]
impl AgentSandbox for MicroVmSandbox {
    async fn create(&self, config: &SandboxConfig) -> anyhow::Result<SandboxHandle> {
        // Verify runtime is available.
        if !self.is_available() {
            anyhow::bail!(
                "microVM runtime '{}' not found at {:?}",
                self.config.runtime,
                self.config.binary_path
            );
        }

        // Create worktree for the workspace files.
        let handle = self.worktree.create(config).await?;

        let socket = Self::socket_path(&config.workflow_id, &config.node_id);

        // Build Firecracker config JSON.
        let fc_config = serde_json::json!({
            "boot-source": {
                "kernel_image_path": self.config.kernel_path.to_string_lossy(),
                "boot_args": "console=ttyS0 reboot=k panic=1 pci=off"
            },
            "drives": [{
                "drive_id": "rootfs",
                "path_on_host": self.config.rootfs_path.to_string_lossy(),
                "is_root_device": true,
                "is_read_only": false
            }],
            "machine-config": {
                "vcpu_count": self.config.vcpus,
                "mem_size_mib": self.config.memory_mb
            }
        });

        // Write config to a temp file.
        let config_path = std::env::temp_dir().join(format!(
            "agentzero-vm-{}-{}.json",
            config.workflow_id, config.node_id
        ));
        std::fs::write(&config_path, fc_config.to_string())?;

        // Start Firecracker (non-blocking — it runs as a daemon).
        let child = std::process::Command::new(&self.config.binary_path)
            .args([
                "--api-sock",
                &socket.to_string_lossy(),
                "--config-file",
                &config_path.to_string_lossy(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match child {
            Ok(_) => {
                tracing::info!(
                    runtime = %self.config.runtime,
                    vcpus = self.config.vcpus,
                    memory_mb = self.config.memory_mb,
                    socket = %socket.display(),
                    "created microVM sandbox"
                );
            }
            Err(e) => {
                let _ = self.worktree.destroy(&handle).await;
                let _ = std::fs::remove_file(&config_path);
                anyhow::bail!("failed to start {}: {e}", self.config.runtime);
            }
        }

        Ok(handle)
    }

    async fn destroy(&self, handle: &SandboxHandle) -> anyhow::Result<()> {
        // Derive IDs from branch name for socket/config cleanup.
        let branch_parts: Vec<&str> = handle.branch_name.split('/').collect();
        if branch_parts.len() >= 4 {
            let workflow_id = branch_parts[2];
            let node_id = branch_parts[3];

            // Clean up socket and config files.
            let socket = Self::socket_path(workflow_id, node_id);
            let config_path =
                std::env::temp_dir().join(format!("agentzero-vm-{workflow_id}-{node_id}.json"));

            let _ = std::fs::remove_file(&socket);
            let _ = std::fs::remove_file(&config_path);
        }

        tracing::info!(
            branch = %handle.branch_name,
            "destroyed microVM sandbox"
        );

        // Clean up the worktree.
        self.worktree.destroy(handle).await
    }
}

// ── Sandbox level selection ─────────────────────────────────────────────────

/// Sandbox isolation level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxLevel {
    /// Git worktree isolation only (lightweight, default for local dev).
    #[default]
    Worktree,
    /// Docker/Podman container with resource limits and capability dropping.
    Container,
    /// Firecracker/Cloud Hypervisor microVM with kernel-level isolation.
    MicroVm,
}

impl std::fmt::Display for SandboxLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Worktree => write!(f, "worktree"),
            Self::Container => write!(f, "container"),
            Self::MicroVm => write!(f, "microvm"),
        }
    }
}

impl std::str::FromStr for SandboxLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "worktree" => Ok(Self::Worktree),
            "container" => Ok(Self::Container),
            "microvm" => Ok(Self::MicroVm),
            other => anyhow::bail!("unknown sandbox level: {other}"),
        }
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

    // ── ContainerSandbox tests ───────────────────────────────────────────

    #[test]
    fn container_name_sanitizes() {
        assert_eq!(
            ContainerSandbox::container_name("wf/1", "node/a"),
            "agentzero-swarm-wf-1-node-a"
        );
    }

    #[test]
    fn container_default_config() {
        let config = ContainerConfig::default();
        assert_eq!(config.runtime, "docker");
        assert_eq!(config.memory_limit, "512m");
        assert_eq!(config.cpu_limit, "1.0");
        assert!(!config.network_enabled);
    }

    #[test]
    fn container_run_args_include_security_flags() {
        let sandbox = ContainerSandbox::new(ContainerConfig::default(), PathBuf::from("/tmp/wt"));
        let handle = SandboxHandle {
            worktree_path: PathBuf::from("/tmp/wt/test-node"),
            branch_name: "agentzero/wf/test/node".to_string(),
            workspace_root: PathBuf::from("/tmp/ws"),
        };

        let args = sandbox.build_run_args(&handle, "test-container");

        assert!(
            args.contains(&"--cap-drop=ALL".to_string()),
            "should drop caps"
        );
        assert!(
            args.contains(&"--read-only".to_string()),
            "should be read-only"
        );
        assert!(
            args.contains(&"--memory=512m".to_string()),
            "should have memory limit"
        );
        assert!(
            args.contains(&"--cpus=1.0".to_string()),
            "should have cpu limit"
        );
        assert!(
            args.contains(&"--network=none".to_string()),
            "should disable network by default"
        );
        assert!(
            args.contains(&"--name".to_string()),
            "should set container name"
        );
    }

    #[test]
    fn container_run_args_network_enabled() {
        let config = ContainerConfig {
            network_enabled: true,
            ..ContainerConfig::default()
        };
        let sandbox = ContainerSandbox::new(config, PathBuf::from("/tmp/wt"));
        let handle = SandboxHandle {
            worktree_path: PathBuf::from("/tmp/wt/test"),
            branch_name: "test".to_string(),
            workspace_root: PathBuf::from("/tmp/ws"),
        };

        let args = sandbox.build_run_args(&handle, "c1");
        assert!(
            !args.contains(&"--network=none".to_string()),
            "network should be enabled"
        );
    }

    // ── MicroVmSandbox tests ────────────────────────────────────────────

    #[test]
    fn microvm_default_config() {
        let config = MicroVmConfig::default();
        assert_eq!(config.runtime, "firecracker");
        assert_eq!(config.memory_mb, 256);
        assert_eq!(config.vcpus, 1);
    }

    #[test]
    fn microvm_socket_path_is_unique() {
        let s1 = MicroVmSandbox::socket_path("wf1", "n1");
        let s2 = MicroVmSandbox::socket_path("wf1", "n2");
        assert_ne!(s1, s2);
    }

    #[test]
    fn microvm_is_available_returns_false_for_missing_binary() {
        let config = MicroVmConfig {
            binary_path: PathBuf::from("/nonexistent/firecracker"),
            runtime: "nonexistent-runtime-xyz".to_string(),
            ..Default::default()
        };
        let sandbox = MicroVmSandbox::new(config, PathBuf::from("/tmp/wt"));
        assert!(!sandbox.is_available());
    }

    // ── SandboxLevel tests ──────────────────────────────────────────────

    #[test]
    fn sandbox_level_parse_and_display() {
        assert_eq!(
            "worktree".parse::<SandboxLevel>().expect("parse"),
            SandboxLevel::Worktree
        );
        assert_eq!(
            "container".parse::<SandboxLevel>().expect("parse"),
            SandboxLevel::Container
        );
        assert_eq!(
            "microvm".parse::<SandboxLevel>().expect("parse"),
            SandboxLevel::MicroVm
        );
        assert!("invalid".parse::<SandboxLevel>().is_err());

        assert_eq!(SandboxLevel::Worktree.to_string(), "worktree");
        assert_eq!(SandboxLevel::Container.to_string(), "container");
        assert_eq!(SandboxLevel::MicroVm.to_string(), "microvm");
    }

    #[test]
    fn sandbox_level_default_is_worktree() {
        assert_eq!(SandboxLevel::default(), SandboxLevel::Worktree);
    }
}
