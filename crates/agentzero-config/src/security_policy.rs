//! Declarative YAML security policy for per-tool egress/command/filesystem rules.
//!
//! When `.agentzero/security-policy.yaml` is present, it provides granular
//! per-tool access control that overlays the TOML-based `ToolSecurityPolicy`.

use serde::Deserialize;
use std::path::Path;

/// Policy decision returned by rule evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny,
    Prompt,
}

/// Default action when no rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefaultAction {
    Allow,
    Deny,
}

/// A declarative security policy loaded from YAML.
#[derive(Debug, Clone, Deserialize)]
pub struct SecurityPolicyFile {
    /// Default action when no rule matches a tool.
    pub default: DefaultAction,
    /// Per-tool rules, evaluated in order.
    #[serde(default)]
    pub rules: Vec<ToolRule>,
}

/// A single per-tool rule.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolRule {
    /// Tool name or glob pattern (e.g., "http_request", "mcp:*", "shell").
    pub tool: String,
    /// Allowed egress domains/IPs. Empty = no network access for this tool.
    /// Special value "prompt" means ask operator on first use.
    #[serde(default)]
    pub egress: Vec<String>,
    /// Allowed shell commands (for shell tool only).
    #[serde(default)]
    pub commands: Vec<String>,
    /// Allowed filesystem paths (for read/write file tools).
    #[serde(default)]
    pub filesystem: Vec<String>,
    /// Action: "allow", "deny", or "prompt".
    #[serde(default = "default_action_allow")]
    pub action: String,
}

fn default_action_allow() -> String {
    "allow".to_string()
}

impl SecurityPolicyFile {
    /// Load from a YAML file. Returns `None` if the file doesn't exist.
    pub fn load(workspace_root: &Path) -> Option<Self> {
        let path = workspace_root
            .join(".agentzero")
            .join("security-policy.yaml");
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        serde_yaml::from_str(&content).ok()
    }

    /// Evaluate whether a tool is allowed to perform an action.
    pub fn check_tool(&self, tool_name: &str) -> PolicyDecision {
        for rule in &self.rules {
            if tool_matches(&rule.tool, tool_name) {
                return match rule.action.as_str() {
                    "deny" => PolicyDecision::Deny,
                    "prompt" => PolicyDecision::Prompt,
                    _ => PolicyDecision::Allow,
                };
            }
        }
        match self.default {
            DefaultAction::Allow => PolicyDecision::Allow,
            DefaultAction::Deny => PolicyDecision::Deny,
        }
    }

    /// Check if a tool is allowed to access a specific egress domain.
    pub fn check_egress(&self, tool_name: &str, domain: &str) -> PolicyDecision {
        for rule in &self.rules {
            if tool_matches(&rule.tool, tool_name) {
                if rule.egress.is_empty() {
                    return match rule.action.as_str() {
                        "deny" => PolicyDecision::Deny,
                        "prompt" => PolicyDecision::Prompt,
                        _ => PolicyDecision::Allow,
                    };
                }
                if rule.egress.iter().any(|e| e == "prompt") {
                    return PolicyDecision::Prompt;
                }
                if rule.egress.iter().any(|e| domain_matches(e, domain)) {
                    return PolicyDecision::Allow;
                }
                return PolicyDecision::Deny;
            }
        }
        match self.default {
            DefaultAction::Allow => PolicyDecision::Allow,
            DefaultAction::Deny => PolicyDecision::Deny,
        }
    }

    /// Check if a shell command is allowed.
    pub fn check_command(&self, command: &str) -> PolicyDecision {
        for rule in &self.rules {
            if tool_matches(&rule.tool, "shell") {
                if rule.commands.is_empty() {
                    return match rule.action.as_str() {
                        "deny" => PolicyDecision::Deny,
                        "prompt" => PolicyDecision::Prompt,
                        _ => PolicyDecision::Allow,
                    };
                }
                if rule.commands.iter().any(|c| c == command) {
                    return PolicyDecision::Allow;
                }
                return PolicyDecision::Deny;
            }
        }
        match self.default {
            DefaultAction::Allow => PolicyDecision::Allow,
            DefaultAction::Deny => PolicyDecision::Deny,
        }
    }

    /// Check if a tool is allowed to access a filesystem path.
    ///
    /// The path is canonicalized before comparison to prevent traversal attacks
    /// (e.g. `/workspace/../etc/passwd` resolving outside allowed roots).
    pub fn check_filesystem(&self, tool_name: &str, path: &str) -> PolicyDecision {
        let canonical = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string());

        for rule in &self.rules {
            if tool_matches(&rule.tool, tool_name) {
                if rule.filesystem.is_empty() {
                    return match rule.action.as_str() {
                        "deny" => PolicyDecision::Deny,
                        "prompt" => PolicyDecision::Prompt,
                        _ => PolicyDecision::Allow,
                    };
                }
                if rule.filesystem.iter().any(|allowed| {
                    let allowed_canonical = std::fs::canonicalize(allowed)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| allowed.clone());
                    canonical.starts_with(&allowed_canonical)
                }) {
                    return PolicyDecision::Allow;
                }
                return PolicyDecision::Deny;
            }
        }
        match self.default {
            DefaultAction::Allow => PolicyDecision::Allow,
            DefaultAction::Deny => PolicyDecision::Deny,
        }
    }
}

/// Match a tool name against a pattern (supports `*` glob at the end).
fn tool_matches(pattern: &str, tool_name: &str) -> bool {
    if pattern == tool_name {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return tool_name.starts_with(prefix);
    }
    false
}

/// Match a domain against a pattern (supports `*.example.com` wildcard).
fn domain_matches(pattern: &str, domain: &str) -> bool {
    if pattern == domain {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return domain.ends_with(suffix) && domain.len() > suffix.len();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_policy() -> SecurityPolicyFile {
        let yaml = r#"
default: deny
rules:
  - tool: http_request
    egress:
      - api.openai.com
      - "*.github.com"
    action: allow
  - tool: shell
    commands: [git, cargo, rustc]
    action: allow
  - tool: read_file
    filesystem: [/workspace, /tmp]
    action: allow
  - tool: "mcp:*"
    egress: [prompt]
    action: prompt
"#;
        serde_yaml::from_str(yaml).expect("should parse")
    }

    #[test]
    fn yaml_policy_parses() {
        let policy = test_policy();
        assert_eq!(policy.default, DefaultAction::Deny);
        assert_eq!(policy.rules.len(), 4);
    }

    #[test]
    fn default_deny_blocks_unlisted_tool() {
        let policy = test_policy();
        assert_eq!(policy.check_tool("unknown_tool"), PolicyDecision::Deny);
    }

    #[test]
    fn default_allow_permits_unlisted_tool() {
        let yaml = "default: allow\nrules: []\n";
        let policy: SecurityPolicyFile = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(policy.check_tool("anything"), PolicyDecision::Allow);
    }

    #[test]
    fn egress_domain_match() {
        let policy = test_policy();
        assert_eq!(
            policy.check_egress("http_request", "api.openai.com"),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn egress_glob_match() {
        let policy = test_policy();
        assert_eq!(
            policy.check_egress("http_request", "api.github.com"),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn egress_unlisted_domain_denied() {
        let policy = test_policy();
        assert_eq!(
            policy.check_egress("http_request", "evil.com"),
            PolicyDecision::Deny
        );
    }

    #[test]
    fn egress_prompt_returns_prompt() {
        let policy = test_policy();
        assert_eq!(
            policy.check_egress("mcp:filesystem", "any.domain.com"),
            PolicyDecision::Prompt
        );
    }

    #[test]
    fn command_allowlist() {
        let policy = test_policy();
        assert_eq!(policy.check_command("git"), PolicyDecision::Allow);
        assert_eq!(policy.check_command("rm"), PolicyDecision::Deny);
    }

    #[test]
    fn filesystem_check() {
        // Use a temp directory that definitely exists for canonicalization.
        let dir = std::env::temp_dir();
        let dir_str = dir.to_string_lossy().to_string();

        // Build a policy that allows the canonical temp dir.
        // Use YAML single-quoted scalars so backslashes in Windows paths
        // are not interpreted as escape sequences by the YAML parser.
        let yaml = format!(
            "default: deny\nrules:\n  - tool: read_file\n    filesystem: ['{dir_str}']\n    action: allow\n"
        );
        let policy: SecurityPolicyFile = serde_yaml::from_str(&yaml).expect("parse");

        // Create a real file so canonicalize() works (it requires the path to exist).
        let test_file = dir.join("agentzero_policy_test.txt");
        std::fs::write(&test_file, "test").expect("create temp file");

        assert_eq!(
            policy.check_filesystem("read_file", &test_file.to_string_lossy()),
            PolicyDecision::Allow
        );

        std::fs::remove_file(&test_file).ok();

        // A path outside the temp dir should be denied.
        assert_eq!(
            policy.check_filesystem("read_file", "/etc/passwd"),
            PolicyDecision::Deny
        );
    }

    #[test]
    fn filesystem_check_traversal_blocked() {
        // Path that tries to escape via traversal should be denied
        // because it canonicalizes outside the allowed root.
        let dir = std::env::temp_dir();
        let dir_str = dir.to_string_lossy().to_string();

        // Use YAML single-quoted scalars so backslashes in Windows paths
        // are not interpreted as escape sequences by the YAML parser.
        let yaml = format!(
            "default: deny\nrules:\n  - tool: read_file\n    filesystem: ['{dir_str}']\n    action: allow\n"
        );
        let policy: SecurityPolicyFile = serde_yaml::from_str(&yaml).expect("parse");

        // /tmp/../etc/passwd canonicalizes to /etc/passwd which is outside /tmp.
        let traversal = format!("{dir_str}/../etc/passwd");
        assert_eq!(
            policy.check_filesystem("read_file", &traversal),
            PolicyDecision::Deny
        );
    }

    #[test]
    fn tool_glob_matches_prefix() {
        assert!(tool_matches("mcp:*", "mcp:filesystem"));
        assert!(tool_matches("mcp:*", "mcp:github"));
        assert!(!tool_matches("mcp:*", "shell"));
    }

    #[test]
    fn domain_wildcard_matches() {
        assert!(domain_matches("*.github.com", "api.github.com"));
        assert!(domain_matches("*.github.com", "raw.github.com"));
        assert!(!domain_matches("*.github.com", "github.com")); // No subdomain
        assert!(!domain_matches("*.github.com", "evil.com"));
    }

    #[test]
    fn missing_yaml_returns_none() {
        let result = SecurityPolicyFile::load(Path::new("/nonexistent/path"));
        assert!(result.is_none());
    }
}
