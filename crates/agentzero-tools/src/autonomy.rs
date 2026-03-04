use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use tracing::warn;

/// Autonomy levels ordered by increasing permissiveness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyLevel {
    ReadOnly,
    Supervised,
    Full,
}

impl AutonomyLevel {
    pub fn from_str_loose(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "read_only" | "readonly" | "read-only" => Self::ReadOnly,
            "full" | "autonomous" => Self::Full,
            _ => Self::Supervised,
        }
    }

    pub fn allows_writes(&self) -> bool {
        !matches!(self, Self::ReadOnly)
    }

    pub fn requires_approval(&self) -> bool {
        matches!(self, Self::Supervised)
    }
}

/// Runtime autonomy policy built from config values.
#[derive(Debug, Clone)]
pub struct AutonomyPolicy {
    pub level: AutonomyLevel,
    pub workspace_only: bool,
    pub forbidden_paths: Vec<String>,
    pub allowed_roots: Vec<String>,
    pub auto_approve: HashSet<String>,
    pub always_ask: HashSet<String>,
    pub allow_sensitive_file_reads: bool,
    pub allow_sensitive_file_writes: bool,
}

impl Default for AutonomyPolicy {
    fn default() -> Self {
        Self {
            level: AutonomyLevel::Supervised,
            workspace_only: true,
            forbidden_paths: vec![
                "/etc".into(),
                "/root".into(),
                "/proc".into(),
                "/sys".into(),
                "~/.ssh".into(),
                "~/.gnupg".into(),
                "~/.aws".into(),
            ],
            allowed_roots: Vec::new(),
            auto_approve: HashSet::new(),
            always_ask: HashSet::new(),
            allow_sensitive_file_reads: false,
            allow_sensitive_file_writes: false,
        }
    }
}

/// Outcome of a tool approval check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Tool is auto-approved.
    Approved,
    /// Tool requires interactive approval.
    NeedsApproval { reason: String },
    /// Tool is blocked unconditionally.
    Blocked { reason: String },
}

/// Sensitive file patterns for detection.
const SENSITIVE_FILE_PATTERNS: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".aws/credentials",
    ".ssh/id_rsa",
    ".ssh/id_ed25519",
    ".gnupg/",
    "credentials.json",
    "service-account.json",
    ".npmrc",
    ".pypirc",
];

impl AutonomyPolicy {
    /// Evaluate whether a tool invocation should proceed.
    pub fn check_tool(&self, tool_name: &str) -> ApprovalDecision {
        // Read-only mode blocks all write tools.
        if !self.level.allows_writes() {
            let write_tools = [
                "file_write",
                "shell",
                "apply_patch",
                "browser",
                "http_request",
            ];
            if write_tools.contains(&tool_name) {
                return ApprovalDecision::Blocked {
                    reason: format!("tool `{tool_name}` blocked: autonomy level is read_only"),
                };
            }
        }

        // Always-ask list overrides auto-approve.
        if self.always_ask.contains(tool_name) {
            return ApprovalDecision::NeedsApproval {
                reason: format!("tool `{tool_name}` is in always_ask list"),
            };
        }

        // Auto-approve list.
        if self.auto_approve.contains(tool_name) {
            return ApprovalDecision::Approved;
        }

        // Full autonomy approves everything not explicitly blocked.
        if matches!(self.level, AutonomyLevel::Full) {
            return ApprovalDecision::Approved;
        }

        // Supervised mode: requires approval for non-read tools.
        if self.level.requires_approval() {
            let read_tools = ["file_read", "glob_search", "content_search", "memory_read"];
            if read_tools.contains(&tool_name) {
                return ApprovalDecision::Approved;
            }
            return ApprovalDecision::NeedsApproval {
                reason: format!("tool `{tool_name}` requires approval in supervised mode"),
            };
        }

        ApprovalDecision::Approved
    }

    /// Check whether a file path is allowed for reading.
    pub fn check_file_read(&self, path: &str) -> ApprovalDecision {
        if self.is_forbidden_path(path) {
            return ApprovalDecision::Blocked {
                reason: format!("path `{path}` is in forbidden_paths"),
            };
        }
        if !self.allow_sensitive_file_reads && is_sensitive_path(path) {
            return ApprovalDecision::Blocked {
                reason: format!(
                    "path `{path}` is a sensitive file (allow_sensitive_file_reads = false)"
                ),
            };
        }
        if self.workspace_only && !self.is_within_allowed_roots(path) {
            return ApprovalDecision::Blocked {
                reason: format!("path `{path}` is outside allowed workspace roots"),
            };
        }
        ApprovalDecision::Approved
    }

    /// Check whether a file path is allowed for writing.
    pub fn check_file_write(&self, path: &str) -> ApprovalDecision {
        if !self.level.allows_writes() {
            return ApprovalDecision::Blocked {
                reason: "writes blocked: autonomy level is read_only".into(),
            };
        }
        if self.is_forbidden_path(path) {
            return ApprovalDecision::Blocked {
                reason: format!("path `{path}` is in forbidden_paths"),
            };
        }
        if !self.allow_sensitive_file_writes && is_sensitive_path(path) {
            return ApprovalDecision::Blocked {
                reason: format!(
                    "path `{path}` is a sensitive file (allow_sensitive_file_writes = false)"
                ),
            };
        }
        if self.workspace_only && !self.is_within_allowed_roots(path) {
            return ApprovalDecision::Blocked {
                reason: format!("path `{path}` is outside allowed workspace roots"),
            };
        }
        ApprovalDecision::Approved
    }

    /// Check whether a file has multiple hard links (potential symlink attack).
    pub fn check_hard_links(path: &str) -> anyhow::Result<()> {
        let metadata = std::fs::metadata(path);
        match metadata {
            Ok(meta) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if meta.nlink() > 1 {
                        anyhow::bail!(
                            "refusing to operate on `{path}`: file has {} hard links",
                            meta.nlink()
                        );
                    }
                }
                let _ = meta;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => {
                warn!("hard-link check failed for {path}: {e}");
                Ok(())
            }
        }
    }

    fn is_forbidden_path(&self, path: &str) -> bool {
        let expanded = expand_tilde(path);
        self.forbidden_paths.iter().any(|forbidden| {
            let forbidden_expanded = expand_tilde(forbidden);
            expanded.starts_with(&forbidden_expanded)
        })
    }

    fn is_within_allowed_roots(&self, path: &str) -> bool {
        if self.allowed_roots.is_empty() {
            return true;
        }
        let expanded = expand_tilde(path);
        self.allowed_roots.iter().any(|root| {
            let root_expanded = expand_tilde(root);
            expanded.starts_with(&root_expanded)
        })
    }
}

/// Detect sensitive files by path suffix/pattern.
pub fn is_sensitive_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    SENSITIVE_FILE_PATTERNS.iter().any(|pattern| {
        normalized.ends_with(pattern)
            || normalized.contains(&format!("/{pattern}"))
            || Path::new(&normalized)
                .file_name()
                .is_some_and(|f| f.to_string_lossy() == *pattern)
    })
}

fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}{}", &path[1..]);
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> AutonomyPolicy {
        AutonomyPolicy::default()
    }

    #[test]
    fn autonomy_level_from_str_loose() {
        assert_eq!(
            AutonomyLevel::from_str_loose("read_only"),
            AutonomyLevel::ReadOnly
        );
        assert_eq!(
            AutonomyLevel::from_str_loose("readonly"),
            AutonomyLevel::ReadOnly
        );
        assert_eq!(AutonomyLevel::from_str_loose("full"), AutonomyLevel::Full);
        assert_eq!(
            AutonomyLevel::from_str_loose("supervised"),
            AutonomyLevel::Supervised
        );
        assert_eq!(
            AutonomyLevel::from_str_loose("anything"),
            AutonomyLevel::Supervised
        );
    }

    #[test]
    fn read_only_blocks_write_tools() {
        let mut p = policy();
        p.level = AutonomyLevel::ReadOnly;
        assert_eq!(
            p.check_tool("shell"),
            ApprovalDecision::Blocked {
                reason: "tool `shell` blocked: autonomy level is read_only".into()
            }
        );
        assert_eq!(p.check_tool("file_read"), ApprovalDecision::Approved);
    }

    #[test]
    fn supervised_requires_approval_for_non_read_tools() {
        let p = policy();
        assert_eq!(p.check_tool("file_read"), ApprovalDecision::Approved);
        assert!(matches!(
            p.check_tool("shell"),
            ApprovalDecision::NeedsApproval { .. }
        ));
    }

    #[test]
    fn full_autonomy_auto_approves_everything() {
        let mut p = policy();
        p.level = AutonomyLevel::Full;
        assert_eq!(p.check_tool("shell"), ApprovalDecision::Approved);
        assert_eq!(p.check_tool("file_write"), ApprovalDecision::Approved);
    }

    #[test]
    fn always_ask_overrides_auto_approve() {
        let mut p = policy();
        p.level = AutonomyLevel::Full;
        p.auto_approve.insert("shell".into());
        p.always_ask.insert("shell".into());
        assert!(matches!(
            p.check_tool("shell"),
            ApprovalDecision::NeedsApproval { .. }
        ));
    }

    #[test]
    fn forbidden_paths_blocks_access() {
        let p = policy();
        assert!(matches!(
            p.check_file_read("/etc/passwd"),
            ApprovalDecision::Blocked { .. }
        ));
    }

    #[test]
    fn sensitive_file_detection() {
        assert!(is_sensitive_path("/home/user/.env"));
        assert!(is_sensitive_path("/home/user/.aws/credentials"));
        assert!(is_sensitive_path("/project/.ssh/id_rsa"));
        assert!(!is_sensitive_path("/project/src/main.rs"));
    }

    #[test]
    fn sensitive_file_read_blocked_by_default() {
        let p = policy();
        assert!(matches!(
            p.check_file_read("/project/.env"),
            ApprovalDecision::Blocked { .. }
        ));
    }

    #[test]
    fn sensitive_file_read_allowed_when_configured() {
        let mut p = policy();
        p.allow_sensitive_file_reads = true;
        assert_eq!(
            p.check_file_read("/project/.env"),
            ApprovalDecision::Approved
        );
    }

    #[test]
    fn write_blocked_in_read_only() {
        let mut p = policy();
        p.level = AutonomyLevel::ReadOnly;
        assert!(matches!(
            p.check_file_write("/project/file.txt"),
            ApprovalDecision::Blocked { .. }
        ));
    }
}
