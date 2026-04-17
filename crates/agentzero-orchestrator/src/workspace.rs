//! Workspace lifecycle management for sandboxed agent execution.
//!
//! Handles collecting diffs from agent worktrees, detecting conflicts between
//! parallel agents, and merging results back to the main branch.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::sandbox::SandboxHandle;

/// Create a `git` command with all inherited git env vars removed.
///
/// This prevents interference from the outer repo when running inside
/// git hooks (which set `GIT_INDEX_FILE`) or worktrees (which set
/// `GIT_COMMON_DIR`).
fn git_cmd() -> Command {
    let mut cmd = Command::new("git");
    cmd.env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

// ── Types ────────────────────────────────────────────────────────────────────

/// A file modification produced by an agent in its sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// Path relative to the worktree root.
    pub path: String,
    /// The type of change (added, modified, deleted, renamed).
    pub change_type: ChangeType,
    /// Unified diff text for the file (empty for binary files).
    pub diff_text: String,
}

/// Classification of a file change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// Severity of a merge conflict between two agents' changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSeverity {
    /// Same directory modified by multiple agents.
    Low,
    /// Same file modified by multiple agents (different lines).
    Medium,
    /// Same lines in the same file modified by multiple agents.
    High,
}

/// A detected conflict between two agents' changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// The file path where the conflict occurs.
    pub path: String,
    /// Severity classification.
    pub severity: ConflictSeverity,
    /// IDs of the agents involved.
    pub agents: Vec<String>,
    /// Description of the conflict.
    pub description: String,
}

/// Result of merging multiple agents' worktrees.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    /// Whether the merge was clean (no conflicts).
    pub clean: bool,
    /// Detected conflicts, sorted by severity (high first).
    pub conflicts: Vec<Conflict>,
    /// Files successfully merged.
    pub merged_files: Vec<String>,
}

// ── Functions ────────────────────────────────────────────────────────────────

/// Collect the diff (staged + unstaged) from a sandbox worktree.
///
/// Returns a list of file modifications the agent made relative to the
/// worktree's base commit (HEAD when the worktree was created).
pub fn collect_diff(handle: &SandboxHandle) -> anyhow::Result<Vec<FileDiff>> {
    let worktree = &handle.worktree_path;

    // Get list of changed files (status --porcelain for machine-readable output).
    let output = git_cmd()
        .args(["status", "--porcelain"])
        .current_dir(worktree)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git status failed in worktree: {stderr}");
    }

    let status_text = String::from_utf8_lossy(&output.stdout);
    let mut diffs = Vec::new();

    for line in status_text.lines() {
        if line.len() < 4 {
            continue;
        }
        let status_code = &line[..2];
        let file_path = line[3..].trim().to_string();

        let change_type = match status_code.trim() {
            "??" | "A" | "AM" => ChangeType::Added,
            "M" | "MM" | " M" => ChangeType::Modified,
            "D" | " D" => ChangeType::Deleted,
            "R" | "RM" => ChangeType::Renamed,
            _ => ChangeType::Modified,
        };

        // Get the diff text for this file.
        let diff_output = git_cmd()
            .args(["diff", "HEAD", "--", &file_path])
            .current_dir(worktree)
            .output()?;

        let diff_text = if diff_output.status.success() {
            String::from_utf8_lossy(&diff_output.stdout).to_string()
        } else {
            String::new()
        };

        diffs.push(FileDiff {
            path: file_path,
            change_type,
            diff_text,
        });
    }

    Ok(diffs)
}

/// Detect conflicts between multiple agents' diffs.
///
/// Compares file modifications from each agent and classifies overlaps by
/// severity. Agents are identified by their sandbox node IDs.
pub fn detect_conflicts(agent_diffs: &[(String, Vec<FileDiff>)]) -> Vec<Conflict> {
    // Build a map: file_path → list of (agent_id, diff)
    let mut file_agents: HashMap<String, Vec<(String, &FileDiff)>> = HashMap::new();
    // Also track directories.
    let mut dir_agents: HashMap<String, HashSet<String>> = HashMap::new();

    for (agent_id, diffs) in agent_diffs {
        for diff in diffs {
            file_agents
                .entry(diff.path.clone())
                .or_default()
                .push((agent_id.clone(), diff));

            // Track parent directories (skip root).
            if let Some(parent) = Path::new(&diff.path).parent() {
                let parent_str = parent.to_string_lossy().to_string();
                if !parent_str.is_empty() {
                    dir_agents
                        .entry(parent_str)
                        .or_default()
                        .insert(agent_id.clone());
                }
            }
        }
    }

    let mut conflicts = Vec::new();

    // Check file-level conflicts.
    for (path, agents) in &file_agents {
        if agents.len() < 2 {
            continue;
        }

        let agent_ids: Vec<String> = agents.iter().map(|(id, _)| id.clone()).collect();

        // Determine severity: check if diffs touch the same lines.
        let severity = if has_line_overlap(agents) {
            ConflictSeverity::High
        } else {
            ConflictSeverity::Medium
        };

        conflicts.push(Conflict {
            path: path.clone(),
            severity,
            agents: agent_ids.clone(),
            description: format!(
                "{} agents modified '{}' ({})",
                agent_ids.len(),
                path,
                match severity {
                    ConflictSeverity::High => "same lines",
                    ConflictSeverity::Medium => "different lines",
                    ConflictSeverity::Low => "same directory",
                }
            ),
        });
    }

    // Check directory-level conflicts (only if no file-level conflict already).
    let conflicted_files: HashSet<String> = conflicts.iter().map(|c| c.path.clone()).collect();
    for (dir, agents) in &dir_agents {
        if agents.len() < 2 {
            continue;
        }
        // Skip if we already have file-level conflicts for files in this dir.
        let has_file_conflict = file_agents.keys().any(|f| {
            Path::new(f)
                .parent()
                .map(|p| p.to_string_lossy() == *dir)
                .unwrap_or(false)
                && conflicted_files.contains(f)
        });
        if has_file_conflict {
            continue;
        }

        let agent_ids: Vec<String> = agents.iter().cloned().collect();
        conflicts.push(Conflict {
            path: dir.clone(),
            severity: ConflictSeverity::Low,
            agents: agent_ids.clone(),
            description: format!(
                "{} agents modified files in directory '{}'",
                agent_ids.len(),
                dir
            ),
        });
    }

    // Sort by severity (high first).
    conflicts.sort_by_key(|b| std::cmp::Reverse(b.severity));
    conflicts
}

/// Check if two or more agents' diffs for the same file touch overlapping lines.
fn has_line_overlap(agents: &[(String, &FileDiff)]) -> bool {
    // Parse diff hunks to extract line ranges.
    let mut agent_ranges: Vec<Vec<(usize, usize)>> = Vec::new();

    for (_, diff) in agents {
        let ranges = parse_diff_line_ranges(&diff.diff_text);
        agent_ranges.push(ranges);
    }

    // Check pairwise overlap.
    for i in 0..agent_ranges.len() {
        for j in (i + 1)..agent_ranges.len() {
            for (a_start, a_end) in &agent_ranges[i] {
                for (b_start, b_end) in &agent_ranges[j] {
                    if a_start <= b_end && b_start <= a_end {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Parse unified diff text to extract modified line ranges.
///
/// Returns `(start_line, end_line)` pairs from `@@ -X,Y +A,B @@` headers.
fn parse_diff_line_ranges(diff_text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();

    for line in diff_text.lines() {
        if !line.starts_with("@@") {
            continue;
        }
        // Parse "@@ -X,Y +A,B @@" — we care about the +A,B (new file lines).
        if let Some(plus_part) = line.split('+').nth(1) {
            let nums: Vec<&str> = plus_part
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .collect();

            if let Some(start_str) = nums.first() {
                if let Ok(start) = start_str.parse::<usize>() {
                    let count = nums
                        .get(1)
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(1);
                    let end = start + count.saturating_sub(1);
                    ranges.push((start, end));
                }
            }
        }
    }

    ranges
}

/// Merge an agent's worktree changes back to the target branch.
///
/// Stages and commits all changes in the worktree, then cherry-picks
/// the commit onto the target branch.
pub fn merge_worktree(handle: &SandboxHandle, agent_name: &str) -> anyhow::Result<bool> {
    let worktree = &handle.worktree_path;

    // Stage all changes.
    let output = git_cmd()
        .args(["add", "-A"])
        .current_dir(worktree)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git add failed: {stderr}");
    }

    // Check if there's anything to commit.
    let status = git_cmd()
        .args(["status", "--porcelain"])
        .current_dir(worktree)
        .output()?;

    let status_text = String::from_utf8_lossy(&status.stdout);
    if status_text.trim().is_empty() {
        // Nothing to merge.
        return Ok(true);
    }

    // Commit in the worktree.
    let commit_msg = format!("swarm: {agent_name} agent output");
    let output = git_cmd()
        .args(["commit", "-m", &commit_msg])
        .current_dir(worktree)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git commit in worktree failed: {stderr}");
    }

    // Get the commit hash.
    let hash_output = git_cmd()
        .args(["rev-parse", "HEAD"])
        .current_dir(worktree)
        .output()?;

    let commit_hash = String::from_utf8_lossy(&hash_output.stdout)
        .trim()
        .to_string();

    // Cherry-pick onto the main workspace.
    let pick_output = git_cmd()
        .args(["cherry-pick", &commit_hash])
        .current_dir(&handle.workspace_root)
        .output()?;

    if !pick_output.status.success() {
        let stderr = String::from_utf8_lossy(&pick_output.stderr);
        tracing::warn!(
            agent = %agent_name,
            commit = %commit_hash,
            error = %stderr,
            "cherry-pick had conflicts"
        );
        // Abort the failed cherry-pick to keep workspace clean.
        let _ = git_cmd()
            .args(["cherry-pick", "--abort"])
            .current_dir(&handle.workspace_root)
            .output();
        return Ok(false);
    }

    Ok(true)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::{AgentSandbox, SandboxConfig, WorktreeSandbox};

    /// Helper: create a temporary git repo for testing.
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

        std::fs::write(path.join("README.md"), "# Test\n").expect("write readme");
        std::fs::create_dir_all(path.join("src")).expect("create src dir");
        std::fs::write(path.join("src/main.rs"), "fn main() {}\n").expect("write main");

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
    async fn collect_diff_detects_new_file() {
        let repo = create_test_repo();
        let workspace = repo.path().to_path_buf();
        let sandbox = WorktreeSandbox::from_workspace(&workspace);

        let config = SandboxConfig {
            workflow_id: "wf-diff".to_string(),
            node_id: "diff-node".to_string(),
            workspace_root: workspace.clone(),
        };

        let handle = sandbox.create(&config).await.expect("create");

        // Write a new file in the worktree.
        std::fs::write(handle.worktree_path.join("output.txt"), "agent output\n")
            .expect("write output");

        let diffs = collect_diff(&handle).expect("collect diff");
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "output.txt");
        assert_eq!(diffs[0].change_type, ChangeType::Added);

        sandbox.destroy(&handle).await.expect("destroy");
    }

    #[tokio::test]
    async fn collect_diff_detects_modification() {
        let repo = create_test_repo();
        let workspace = repo.path().to_path_buf();
        let sandbox = WorktreeSandbox::from_workspace(&workspace);

        let config = SandboxConfig {
            workflow_id: "wf-mod".to_string(),
            node_id: "mod-node".to_string(),
            workspace_root: workspace.clone(),
        };

        let handle = sandbox.create(&config).await.expect("create");

        // Modify an existing file.
        std::fs::write(
            handle.worktree_path.join("README.md"),
            "# Modified by agent\n",
        )
        .expect("modify readme");

        let diffs = collect_diff(&handle).expect("collect diff");
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "README.md");
        assert_eq!(diffs[0].change_type, ChangeType::Modified);
        assert!(!diffs[0].diff_text.is_empty());

        sandbox.destroy(&handle).await.expect("destroy");
    }

    #[test]
    fn detect_conflicts_no_overlap() {
        let diffs_a = vec![FileDiff {
            path: "file_a.txt".to_string(),
            change_type: ChangeType::Added,
            diff_text: String::new(),
        }];
        let diffs_b = vec![FileDiff {
            path: "file_b.txt".to_string(),
            change_type: ChangeType::Added,
            diff_text: String::new(),
        }];

        let conflicts = detect_conflicts(&[
            ("agent-a".to_string(), diffs_a),
            ("agent-b".to_string(), diffs_b),
        ]);

        assert!(conflicts.is_empty(), "no conflicts expected");
    }

    #[test]
    fn detect_conflicts_same_file_different_lines() {
        let diffs_a = vec![FileDiff {
            path: "shared.rs".to_string(),
            change_type: ChangeType::Modified,
            diff_text: "@@ -1,3 +1,4 @@\n+// added by A\n fn main() {}\n".to_string(),
        }];
        let diffs_b = vec![FileDiff {
            path: "shared.rs".to_string(),
            change_type: ChangeType::Modified,
            diff_text: "@@ -10,3 +10,4 @@\n+// added by B\n fn helper() {}\n".to_string(),
        }];

        let conflicts = detect_conflicts(&[
            ("agent-a".to_string(), diffs_a),
            ("agent-b".to_string(), diffs_b),
        ]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].severity, ConflictSeverity::Medium);
        assert_eq!(conflicts[0].path, "shared.rs");
    }

    #[test]
    fn detect_conflicts_same_lines() {
        let diffs_a = vec![FileDiff {
            path: "shared.rs".to_string(),
            change_type: ChangeType::Modified,
            diff_text: "@@ -1,3 +1,4 @@\n+// added by A\n fn main() {}\n".to_string(),
        }];
        let diffs_b = vec![FileDiff {
            path: "shared.rs".to_string(),
            change_type: ChangeType::Modified,
            diff_text: "@@ -1,3 +1,5 @@\n+// added by B\n+// more B\n fn main() {}\n".to_string(),
        }];

        let conflicts = detect_conflicts(&[
            ("agent-a".to_string(), diffs_a),
            ("agent-b".to_string(), diffs_b),
        ]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].severity, ConflictSeverity::High);
    }

    #[test]
    fn detect_conflicts_same_directory() {
        let diffs_a = vec![FileDiff {
            path: "src/file_a.rs".to_string(),
            change_type: ChangeType::Added,
            diff_text: String::new(),
        }];
        let diffs_b = vec![FileDiff {
            path: "src/file_b.rs".to_string(),
            change_type: ChangeType::Added,
            diff_text: String::new(),
        }];

        let conflicts = detect_conflicts(&[
            ("agent-a".to_string(), diffs_a),
            ("agent-b".to_string(), diffs_b),
        ]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].severity, ConflictSeverity::Low);
        assert_eq!(conflicts[0].path, "src");
    }

    #[test]
    fn parse_diff_line_ranges_extracts_correctly() {
        let diff = "@@ -1,5 +1,7 @@\n context\n+added\n@@ -20,3 +22,4 @@\n more\n";
        let ranges = parse_diff_line_ranges(diff);
        assert_eq!(ranges, vec![(1, 7), (22, 25)]);
    }

    #[tokio::test]
    async fn merge_worktree_clean_merge() {
        let repo = create_test_repo();
        let workspace = repo.path().to_path_buf();
        let sandbox = WorktreeSandbox::from_workspace(&workspace);

        let config = SandboxConfig {
            workflow_id: "wf-merge".to_string(),
            node_id: "merge-node".to_string(),
            workspace_root: workspace.clone(),
        };

        let handle = sandbox.create(&config).await.expect("create");

        // Write a new file in the worktree.
        std::fs::write(handle.worktree_path.join("agent_output.txt"), "result\n").expect("write");

        let clean = merge_worktree(&handle, "merge-agent").expect("merge");
        assert!(clean, "merge should be clean");

        // Verify the file exists in the main workspace now.
        assert!(
            workspace.join("agent_output.txt").exists(),
            "merged file should exist in main workspace"
        );

        sandbox.destroy(&handle).await.expect("destroy");
    }
}
