use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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

/// Per-tool rate limit: max invocations within a sliding window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRateLimit {
    /// Maximum invocations allowed per window.
    pub max_calls: u32,
    /// Window duration in seconds.
    pub window_secs: u64,
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
    /// Optional per-tool rate limits (tool_name -> limit).
    pub tool_rate_limits: HashMap<String, ToolRateLimit>,
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
            tool_rate_limits: HashMap::new(),
        }
    }
}

impl AutonomyLevel {
    /// Return the more restrictive of two levels.
    fn most_restrictive(self, other: Self) -> Self {
        match (self, other) {
            (Self::ReadOnly, _) | (_, Self::ReadOnly) => Self::ReadOnly,
            (Self::Supervised, _) | (_, Self::Supervised) => Self::Supervised,
            _ => Self::Full,
        }
    }
}

impl AutonomyPolicy {
    /// Intersect two policies, producing a child policy that is at least as
    /// restrictive as the parent on every dimension. Used when delegating to
    /// sub-agents so the child can never escalate beyond the parent's privileges.
    pub fn intersect(&self, child: &AutonomyPolicy) -> AutonomyPolicy {
        // Most restrictive level wins.
        let level = self.level.most_restrictive(child.level);

        // workspace_only: true if either requires it.
        let workspace_only = self.workspace_only || child.workspace_only;

        // Forbidden paths: union of both (more paths forbidden).
        let mut forbidden_paths = self.forbidden_paths.clone();
        for p in &child.forbidden_paths {
            if !forbidden_paths.contains(p) {
                forbidden_paths.push(p.clone());
            }
        }

        // Allowed roots: intersection (only roots allowed by both).
        // Empty means "all", so if one side is empty, use the other.
        let allowed_roots = if self.allowed_roots.is_empty() {
            child.allowed_roots.clone()
        } else if child.allowed_roots.is_empty() {
            self.allowed_roots.clone()
        } else {
            self.allowed_roots
                .iter()
                .filter(|r| child.allowed_roots.contains(r))
                .cloned()
                .collect()
        };

        // auto_approve: intersection (only tools both approve).
        let auto_approve = self
            .auto_approve
            .intersection(&child.auto_approve)
            .cloned()
            .collect();

        // always_ask: union (if either side requires asking, ask).
        let always_ask = self.always_ask.union(&child.always_ask).cloned().collect();

        // Sensitive file access: only if both allow.
        let allow_sensitive_file_reads =
            self.allow_sensitive_file_reads && child.allow_sensitive_file_reads;
        let allow_sensitive_file_writes =
            self.allow_sensitive_file_writes && child.allow_sensitive_file_writes;

        // Tool rate limits: take the more restrictive (lower max_calls) for each tool.
        let mut tool_rate_limits = self.tool_rate_limits.clone();
        for (tool, child_limit) in &child.tool_rate_limits {
            tool_rate_limits
                .entry(tool.clone())
                .and_modify(|parent_limit| {
                    if child_limit.max_calls < parent_limit.max_calls {
                        *parent_limit = child_limit.clone();
                    }
                })
                .or_insert_with(|| child_limit.clone());
        }

        AutonomyPolicy {
            level,
            workspace_only,
            forbidden_paths,
            allowed_roots,
            auto_approve,
            always_ask,
            allow_sensitive_file_reads,
            allow_sensitive_file_writes,
            tool_rate_limits,
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
///
/// Checked via `ends_with`, `contains("/pattern")`, and exact file-name match,
/// so both full paths and bare filenames are covered.
const SENSITIVE_FILE_PATTERNS: &[&str] = &[
    // Environment / dotenv
    ".env",
    ".env.local",
    ".env.production",
    ".env.staging",
    ".env.development",
    // Cloud credentials
    ".aws/credentials",
    ".aws/config",
    ".azure/accessTokens.json",
    ".config/gcloud/credentials.db",
    ".config/gcloud/application_default_credentials.json",
    // SSH keys
    ".ssh/id_rsa",
    ".ssh/id_ed25519",
    ".ssh/id_ecdsa",
    ".ssh/id_dsa",
    // GPG
    ".gnupg/",
    // Kubernetes
    ".kube/config",
    // Docker
    ".docker/config.json",
    // Package registry tokens
    ".npmrc",
    ".pypirc",
    ".gem/credentials",
    // Database
    ".pgpass",
    ".my.cnf",
    ".netrc",
    // Service account / OAuth
    "credentials.json",
    "service-account.json",
    "client_secret.json",
    // Certificate private keys
    ".pem",
    ".key",
    ".p12",
    ".pfx",
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
        let display = sanitize_path_for_display(path);
        if self.is_forbidden_path(path) {
            return ApprovalDecision::Blocked {
                reason: format!("path `{display}` is in forbidden_paths"),
            };
        }
        if !self.allow_sensitive_file_reads && is_sensitive_path(path) {
            return ApprovalDecision::Blocked {
                reason: format!(
                    "path `{display}` is a sensitive file (allow_sensitive_file_reads = false)"
                ),
            };
        }
        if self.workspace_only && !self.is_within_allowed_roots(path) {
            return ApprovalDecision::Blocked {
                reason: format!("path `{display}` is outside allowed workspace roots"),
            };
        }
        ApprovalDecision::Approved
    }

    /// Check whether a file path is allowed for writing.
    pub fn check_file_write(&self, path: &str) -> ApprovalDecision {
        let display = sanitize_path_for_display(path);
        if !self.level.allows_writes() {
            return ApprovalDecision::Blocked {
                reason: "writes blocked: autonomy level is read_only".into(),
            };
        }
        if self.is_forbidden_path(path) {
            return ApprovalDecision::Blocked {
                reason: format!("path `{display}` is in forbidden_paths"),
            };
        }
        if !self.allow_sensitive_file_writes && is_sensitive_path(path) {
            return ApprovalDecision::Blocked {
                reason: format!(
                    "path `{display}` is a sensitive file (allow_sensitive_file_writes = false)"
                ),
            };
        }
        if self.workspace_only && !self.is_within_allowed_roots(path) {
            return ApprovalDecision::Blocked {
                reason: format!("path `{display}` is outside allowed workspace roots"),
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

    // Catch any `.env.*` variant (e.g. `.env.test`, `.env.ci`).
    if let Some(fname) = Path::new(&normalized).file_name() {
        let fname = fname.to_string_lossy();
        if fname.starts_with(".env.") || fname == ".env" {
            return true;
        }
    }

    SENSITIVE_FILE_PATTERNS.iter().any(|pattern| {
        normalized.ends_with(pattern)
            || normalized.contains(&format!("/{pattern}"))
            || Path::new(&normalized)
                .file_name()
                .is_some_and(|f| f.to_string_lossy() == *pattern)
    })
}

/// Replace the user's home directory with `~` in paths shown to the LLM,
/// preventing leakage of the full filesystem layout.
fn sanitize_path_for_display(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
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
    fn sensitive_file_detects_expanded_patterns() {
        // .env.* wildcard
        assert!(is_sensitive_path("/app/.env.test"));
        assert!(is_sensitive_path("/app/.env.ci"));
        // New additions
        assert!(is_sensitive_path("/home/user/.docker/config.json"));
        assert!(is_sensitive_path("/home/user/.kube/config"));
        assert!(is_sensitive_path("/home/user/.ssh/id_ecdsa"));
        assert!(is_sensitive_path("/home/user/.pgpass"));
        assert!(is_sensitive_path("/home/user/.netrc"));
        assert!(is_sensitive_path("/certs/server.key"));
        assert!(is_sensitive_path("/certs/server.pem"));
        // Still negative
        assert!(!is_sensitive_path("/project/Cargo.toml"));
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

    // ─── intersect() tests ──────────────────────────────────────────────

    #[test]
    fn intersect_level_takes_most_restrictive() {
        let parent = AutonomyPolicy {
            level: AutonomyLevel::Full,
            ..policy()
        };
        let child = AutonomyPolicy {
            level: AutonomyLevel::Supervised,
            ..policy()
        };
        assert_eq!(parent.intersect(&child).level, AutonomyLevel::Supervised);

        let parent2 = AutonomyPolicy {
            level: AutonomyLevel::ReadOnly,
            ..policy()
        };
        assert_eq!(parent2.intersect(&child).level, AutonomyLevel::ReadOnly);
    }

    #[test]
    fn intersect_workspace_only_is_union() {
        let parent = AutonomyPolicy {
            workspace_only: false,
            ..policy()
        };
        let child = AutonomyPolicy {
            workspace_only: true,
            ..policy()
        };
        assert!(parent.intersect(&child).workspace_only);
    }

    #[test]
    fn intersect_forbidden_paths_is_union() {
        let parent = AutonomyPolicy {
            forbidden_paths: vec!["/etc".into(), "/root".into()],
            ..policy()
        };
        let child = AutonomyPolicy {
            forbidden_paths: vec!["/root".into(), "/tmp".into()],
            ..policy()
        };
        let result = parent.intersect(&child);
        assert!(result.forbidden_paths.contains(&"/etc".to_string()));
        assert!(result.forbidden_paths.contains(&"/root".to_string()));
        assert!(result.forbidden_paths.contains(&"/tmp".to_string()));
        // No duplicates.
        assert_eq!(result.forbidden_paths.len(), 3);
    }

    #[test]
    fn intersect_allowed_roots_is_intersection() {
        let parent = AutonomyPolicy {
            allowed_roots: vec!["/project".into(), "/shared".into()],
            ..policy()
        };
        let child = AutonomyPolicy {
            allowed_roots: vec!["/project".into(), "/other".into()],
            ..policy()
        };
        let result = parent.intersect(&child);
        assert_eq!(result.allowed_roots, vec!["/project".to_string()]);
    }

    #[test]
    fn intersect_allowed_roots_empty_parent_uses_child() {
        let parent = AutonomyPolicy {
            allowed_roots: vec![],
            ..policy()
        };
        let child = AutonomyPolicy {
            allowed_roots: vec!["/project".into()],
            ..policy()
        };
        let result = parent.intersect(&child);
        assert_eq!(result.allowed_roots, vec!["/project".to_string()]);
    }

    #[test]
    fn intersect_auto_approve_is_intersection() {
        let mut parent = policy();
        parent.auto_approve.insert("shell".into());
        parent.auto_approve.insert("file_read".into());
        let mut child = policy();
        child.auto_approve.insert("file_read".into());
        child.auto_approve.insert("web_search".into());
        let result = parent.intersect(&child);
        assert!(result.auto_approve.contains("file_read"));
        assert!(!result.auto_approve.contains("shell"));
        assert!(!result.auto_approve.contains("web_search"));
    }

    #[test]
    fn intersect_always_ask_is_union() {
        let mut parent = policy();
        parent.always_ask.insert("shell".into());
        let mut child = policy();
        child.always_ask.insert("http_request".into());
        let result = parent.intersect(&child);
        assert!(result.always_ask.contains("shell"));
        assert!(result.always_ask.contains("http_request"));
    }

    #[test]
    fn intersect_sensitive_only_if_both_allow() {
        let parent = AutonomyPolicy {
            allow_sensitive_file_reads: true,
            allow_sensitive_file_writes: false,
            ..policy()
        };
        let child = AutonomyPolicy {
            allow_sensitive_file_reads: true,
            allow_sensitive_file_writes: true,
            ..policy()
        };
        let result = parent.intersect(&child);
        assert!(result.allow_sensitive_file_reads);
        assert!(!result.allow_sensitive_file_writes);
    }
}
