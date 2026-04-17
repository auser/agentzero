//! Capability-based security model for AgentZero.
//!
//! # Design
//!
//! `CapabilitySet` replaces the flat `enable_*` boolean fields on `ToolSecurityPolicy`
//! with typed, composable permissions. Key properties:
//!
//! - **Deny overrides grant** — an explicit deny always wins.
//! - **Empty means fall back** — when `capabilities` is empty, callers fall back to the
//!   legacy boolean-flag policy. This preserves backward compatibility: existing configs
//!   that don't specify `[[capabilities]]` are completely unaffected.
//! - **Child never exceeds parent** — use [`CapabilitySet::intersect`] when building
//!   sub-agent policies to ensure delegation never expands permissions.
//!
//! # Migration
//!
//! Phase 1 (this sprint): `CapabilitySet` is additive. `ToolSecurityPolicy` gains a
//! `capability_set` field that defaults to empty. When empty, all existing boolean checks
//! apply unchanged. When non-empty (opt-in via `[[capabilities]]` in `agentzero.toml`),
//! the capability set drives all permission decisions.
//!
//! Phase 2 (future): deprecation warnings on `enable_*` booleans.
//! Phase 3 (future): remove `enable_*` booleans.

use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Capability ────────────────────────────────────────────────────────────────

/// A single typed permission that can be granted to an agent.
///
/// Capabilities compose via intersection: when agent A delegates to agent B,
/// B's effective capabilities = `A.intersect(B's configured capabilities)`.
///
/// Serializes with a `type` discriminant for clean TOML/JSON config:
///
/// ```toml
/// [[capabilities]]
/// type = "tool"
/// name = "web_search"
///
/// [[capabilities]]
/// type = "file_write"
/// glob = "src/**/*.rs"
///
/// [[capabilities]]
/// type = "shell"
/// commands = ["ls", "git", "cargo"]
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Capability {
    /// Read files whose path matches the given glob within the workspace.
    FileRead { glob: String },

    /// Write files whose path matches the given glob within the workspace.
    FileWrite { glob: String },

    /// Execute shell commands that appear in the given allowlist.
    Shell { commands: Vec<String> },

    /// HTTP/WebSocket access to domains matching the given glob patterns.
    /// Example: `["*.openai.com", "api.anthropic.com"]`
    Network { domains: Vec<String> },

    /// Access a specific tool by name. Supports glob patterns:
    /// `"mcp__*"` → all MCP tools, `"cron_*"` → all cron tools.
    Tool { name: String },

    /// Access the memory store, optionally restricted to a named scope.
    /// `None` → full memory access; `Some("agent-x")` → scoped to `agent-x`.
    Memory { scope: Option<String> },

    /// Spawn sub-agents bounded by at most these capabilities.
    /// `max_capabilities` is the ceiling — child receives
    /// `intersect(parent, child_config, max_capabilities)`.
    Delegate { max_capabilities: Vec<Capability> },
}

// ── CapabilitySet ─────────────────────────────────────────────────────────────

/// A composable set of capability grants with optional explicit denials.
///
/// ## Rules
///
/// 1. **Deny overrides grant**: an explicit deny always wins, even if a
///    matching grant exists.
/// 2. **Empty means fall back**: when [`is_empty`][Self::is_empty] is `true`,
///    callers should fall back to the legacy `enable_*` boolean checks.
/// 3. **Child never exceeds parent**: use [`intersect`][Self::intersect] when
///    constructing sub-agent policies.
///
/// ## Example
///
/// ```rust
/// use agentzero_core::security::capability::{Capability, CapabilitySet};
///
/// let set = CapabilitySet::new(
///     vec![
///         Capability::Tool { name: "web_search".into() },
///         Capability::Tool { name: "cron_*".into() },
///         Capability::FileRead { glob: "**/*".into() },
///     ],
///     vec![
///         Capability::Tool { name: "cron_delete".into() }, // deny overrides grant
///     ],
/// );
///
/// assert!(set.allows_tool("web_search"));
/// assert!(set.allows_tool("cron_list"));
/// assert!(!set.allows_tool("cron_delete")); // denied
/// assert!(!set.allows_tool("git_operations")); // not granted
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySet {
    /// Capabilities that are explicitly granted.
    #[serde(default)]
    pub capabilities: Vec<Capability>,

    /// Capabilities that are explicitly denied. Deny always overrides any grant.
    #[serde(default)]
    pub deny: Vec<Capability>,

    /// Optional composite intersection representation.
    /// When present, this `CapabilitySet` represents the logical AND of the
    /// two operand sets. Permission checks evaluate against both operands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite: Option<Box<(CapabilitySet, CapabilitySet)>>,
}

impl CapabilitySet {
    /// Create a new capability set with the given grants and explicit denials.
    pub fn new(capabilities: Vec<Capability>, deny: Vec<Capability>) -> Self {
        Self {
            capabilities,
            deny,
            composite: None,
        }
    }

    /// Returns `true` when no capabilities are granted.
    ///
    /// Callers use this to fall back to legacy boolean checks:
    ///
    /// ```rust,ignore
    /// if policy.capability_set.is_empty() {
    ///     policy.enable_git   // legacy boolean path
    /// } else {
    ///     policy.capability_set.allows_tool("git_operations")   // new path
    /// }
    /// ```
    pub fn is_empty(&self) -> bool {
        // Consider the set empty only when there are no explicit grants and no
        // composite representation. A composite set represents a (potentially)
        // non-empty logical intersection even if the materialized grant list
        // is empty.
        self.capabilities.is_empty() && self.composite.is_none()
    }

    /// Compute the intersection of `self` and `other`.
    ///
    /// The result is represented compositionally: rather than attempting to
    /// compute a canonical list of intersected glob patterns (which is error
    /// prone), we store both operands and evaluate permission checks as the
    /// logical AND of the operands. Deny lists are merged (union — deny in
    /// either set means denied in the result).
    ///
    /// Use this when delegating: `effective = parent.intersect(child_config)`.
    pub fn intersect(&self, other: &CapabilitySet) -> CapabilitySet {
        // Preserve legacy semantics: when either operand is empty (no explicit
        // grants and no composite), the intersection must be empty. This lets
        // callers fall back to boolean `enable_*` semantics when capability
        // lists are not in use.
        if self.is_empty() || other.is_empty() {
            return CapabilitySet {
                capabilities: Vec::new(),
                deny: Vec::new(),
                composite: None,
            };
        }

        // Deny lists are unioned: deny in either operand = deny in result.
        let mut deny = self.deny.clone();
        for d in &other.deny {
            if !deny.contains(d) {
                deny.push(d.clone());
            }
        }

        CapabilitySet {
            capabilities: Vec::new(), // materialized grants omitted; evaluation uses composite
            deny,
            composite: Some(Box::new((self.clone(), other.clone()))),
        }
    }

    // ── Permission checks ─────────────────────────────────────────────────────

    /// Returns `true` if the given tool name is permitted.
    ///
    /// For composite intersections we evaluate the logical AND of both operands.
    pub fn allows_tool(&self, name: &str) -> bool {
        // Composite case: both operands must allow the tool.
        if let Some(boxed) = &self.composite {
            let (a, b) = &**boxed;
            return a.allows_tool(name) && b.allows_tool(name);
        }

        if self.is_denied(|cap| cap_matches_tool(cap, name)) {
            return false;
        }
        self.capabilities
            .iter()
            .any(|cap| cap_matches_tool(cap, name))
    }

    /// Returns `true` if reading the given path is permitted.
    pub fn allows_file_read(&self, path: &Path) -> bool {
        if let Some(boxed) = &self.composite {
            let (a, b) = &**boxed;
            return a.allows_file_read(path) && b.allows_file_read(path);
        }

        if self.is_denied(|cap| cap_matches_file_read(cap, path)) {
            return false;
        }
        self.capabilities
            .iter()
            .any(|cap| cap_matches_file_read(cap, path))
    }

    /// Returns `true` if writing the given path is permitted.
    pub fn allows_file_write(&self, path: &Path) -> bool {
        if let Some(boxed) = &self.composite {
            let (a, b) = &**boxed;
            return a.allows_file_write(path) && b.allows_file_write(path);
        }

        if self.is_denied(|cap| cap_matches_file_write(cap, path)) {
            return false;
        }
        self.capabilities
            .iter()
            .any(|cap| cap_matches_file_write(cap, path))
    }

    /// Returns `true` if accessing the given domain is permitted.
    pub fn allows_network(&self, domain: &str) -> bool {
        if let Some(boxed) = &self.composite {
            let (a, b) = &**boxed;
            return a.allows_network(domain) && b.allows_network(domain);
        }

        if self.is_denied(|cap| cap_matches_network(cap, domain)) {
            return false;
        }
        self.capabilities
            .iter()
            .any(|cap| cap_matches_network(cap, domain))
    }

    /// Returns `true` if the given shell command is permitted.
    pub fn allows_shell(&self, command: &str) -> bool {
        if let Some(boxed) = &self.composite {
            let (a, b) = &**boxed;
            return a.allows_shell(command) && b.allows_shell(command);
        }

        if self.is_denied(|cap| cap_matches_shell(cap, command)) {
            return false;
        }
        self.capabilities
            .iter()
            .any(|cap| cap_matches_shell(cap, command))
    }

    /// Returns `true` if this capability set permits memory access to `namespace`.
    ///
    /// - Empty capability set → `true` (backward-compatible unrestricted access).
    /// - `Memory { scope: None }` → full memory access (all namespaces).
    /// - `Memory { scope: Some(s) }` → only namespace `s`.
    /// - No `Memory` capability present in a non-empty set → `false` (not granted).
    pub fn allows_memory(&self, namespace: &str) -> bool {
        if self.is_empty() {
            return true;
        }
        self.capabilities.iter().any(|c| match c {
            Capability::Memory { scope: None } => true,
            Capability::Memory { scope: Some(s) } => s == namespace,
            _ => false,
        }) && !self.deny.iter().any(|d| match d {
            Capability::Memory { scope: None } => true,
            Capability::Memory { scope: Some(s) } => s == namespace,
            _ => false,
        })
    }

    /// Build a `CapabilitySet` from all `Capability::Delegate { max_capabilities }`
    /// grants in this set.
    ///
    /// When a parent agent calls the delegate tool, the combined `max_capabilities`
    /// from all `Delegate` grants form a ceiling on what capabilities the child
    /// agent may receive.
    ///
    /// Returns an empty `CapabilitySet` when no `Delegate` grants are present
    /// (meaning no ceiling beyond what is already computed from config intersection).
    pub fn delegate_ceiling(&self) -> CapabilitySet {
        let mut caps: Vec<Capability> = self
            .capabilities
            .iter()
            .filter_map(|c| {
                if let Capability::Delegate { max_capabilities } = c {
                    Some(max_capabilities.clone())
                } else {
                    None
                }
            })
            .flatten()
            .collect();
        caps.dedup();
        CapabilitySet::new(caps, vec![])
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Check whether `self` covers (allows) the given capability.
    ///
    /// This helper is used internally by `intersect` to decide whether to retain
    /// a capability grant from an operand. It is `pub(crate)` to allow other
    /// modules within the crate (for example test helpers or composition logic)
    /// to call it without exposing it as part of the public API.
    #[allow(dead_code)]
    pub(crate) fn allows_capability(&self, cap: &Capability) -> bool {
        match cap {
            Capability::Tool { name } => self.allows_tool(name),

            Capability::FileRead { glob } => {
                // Keep if other has any FileRead grant and the glob isn't denied.
                self.capabilities
                    .iter()
                    .any(|c| matches!(c, Capability::FileRead { .. }))
                    && !self.is_denied(|d| cap_matches_file_read(d, Path::new(glob)))
            }

            Capability::FileWrite { glob } => {
                self.capabilities
                    .iter()
                    .any(|c| matches!(c, Capability::FileWrite { .. }))
                    && !self.is_denied(|d| cap_matches_file_write(d, Path::new(glob)))
            }

            Capability::Shell { commands } => {
                // Keep if at least one command in the capability is allowed.
                commands.iter().any(|cmd| self.allows_shell(cmd))
            }

            Capability::Network { domains } => domains.iter().any(|d| self.allows_network(d)),

            Capability::Memory { scope } => {
                self.capabilities.iter().any(|c| match c {
                    Capability::Memory { scope: s } => {
                        // None grants full memory access (covers any scope).
                        // Some(x) grants only scope x.
                        s.is_none() || s == scope
                    }
                    _ => false,
                }) && !self
                    .deny
                    .iter()
                    .any(|d| matches!(d, Capability::Memory { .. }))
            }

            Capability::Delegate { .. } => {
                self.capabilities
                    .iter()
                    .any(|c| matches!(c, Capability::Delegate { .. }))
                    && !self
                        .deny
                        .iter()
                        .any(|d| matches!(d, Capability::Delegate { .. }))
            }
        }
    }

    /// Returns `true` if any deny entry matches the predicate.
    fn is_denied(&self, matcher: impl Fn(&Capability) -> bool) -> bool {
        self.deny.iter().any(matcher)
    }

    /// Build a `CapabilitySet` from the legacy boolean `ToolSecurityPolicy` fields.
    ///
    /// This implements the 21-entry mapping table from Plan 46 and is used by
    /// unit tests to verify that boolean-to-capability equivalence holds.
    ///
    /// This function is **not** used in production code paths — booleans still
    /// drive production behavior in Phase 1. Its purpose is to document and
    /// test the authoritative mapping so Phase 2 (deprecation) has a verified
    /// reference implementation.
    pub fn from_policy_booleans(flags: &PolicyBooleanFlags) -> CapabilitySet {
        let mut capabilities: Vec<Capability> = Vec::new();

        macro_rules! push_tool {
            ($cond:expr, $name:expr) => {
                if $cond {
                    capabilities.push(Capability::Tool { name: $name.into() });
                }
            };
        }

        // Plan 46 §Boolean-to-Capability Mapping Table
        push_tool!(flags.enable_git, "git_operations");
        push_tool!(flags.enable_cron, "cron_*");
        push_tool!(flags.enable_web_search, "web_search");
        push_tool!(flags.enable_browser, "browser");
        push_tool!(flags.enable_browser_open, "browser_open");
        push_tool!(flags.enable_http_request, "http_request");
        push_tool!(flags.enable_web_fetch, "web_fetch");
        push_tool!(flags.enable_url_validation, "url_validation");
        push_tool!(flags.enable_agents_ipc, "agents_ipc");
        push_tool!(flags.enable_html_extract, "html_extract");
        push_tool!(flags.enable_pushover, "pushover");
        push_tool!(flags.enable_code_interpreter, "code_interpreter");
        // enable_autopilot maps to two tool globs
        push_tool!(flags.enable_autopilot, "proposal_*");
        push_tool!(flags.enable_autopilot, "mission_*");
        push_tool!(flags.enable_agent_manage, "agent_manage");
        push_tool!(flags.enable_domain_tools, "domain_*");
        // enable_self_config maps to two tool names
        push_tool!(flags.enable_self_config, "config_manage");
        push_tool!(flags.enable_self_config, "skill_manage");
        push_tool!(flags.enable_wasm_plugins, "wasm_*");
        push_tool!(flags.enable_a2a_tool, "a2a");
        push_tool!(flags.enable_dynamic_tools, "tool_create");
        push_tool!(flags.enable_mcp, "mcp__*");

        if flags.enable_write_file {
            capabilities.push(Capability::FileWrite {
                glob: "**/*".into(),
            });
        }

        CapabilitySet {
            capabilities,
            deny: vec![],
            composite: None,
        }
    }
}

// ── Helper: boolean flags struct ─────────────────────────────────────────────

/// Boolean flags mirroring the `ToolSecurityPolicy::enable_*` fields.
///
/// All fields default to `false`. Used by [`CapabilitySet::from_policy_booleans`]
/// and by unit tests to verify the Plan 46 boolean-to-capability mapping table.
#[derive(Debug, Default, Clone)]
pub struct PolicyBooleanFlags {
    pub enable_git: bool,
    pub enable_cron: bool,
    pub enable_web_search: bool,
    pub enable_browser: bool,
    pub enable_browser_open: bool,
    pub enable_http_request: bool,
    pub enable_web_fetch: bool,
    pub enable_url_validation: bool,
    pub enable_agents_ipc: bool,
    pub enable_html_extract: bool,
    pub enable_pushover: bool,
    pub enable_code_interpreter: bool,
    pub enable_autopilot: bool,
    pub enable_agent_manage: bool,
    pub enable_domain_tools: bool,
    pub enable_self_config: bool,
    pub enable_wasm_plugins: bool,
    pub enable_a2a_tool: bool,
    pub enable_dynamic_tools: bool,
    pub enable_write_file: bool,
    pub enable_mcp: bool,
}

// ── Glob match helpers ────────────────────────────────────────────────────────

fn glob_match(pattern: &str, value: &str) -> bool {
    Pattern::new(pattern)
        .map(|p| p.matches(value))
        .unwrap_or(false)
}

fn glob_match_path(pattern: &str, path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    Pattern::new(pattern)
        .map(|p| p.matches(path_str.as_ref()))
        .unwrap_or(false)
}

fn cap_matches_tool(cap: &Capability, name: &str) -> bool {
    matches!(cap, Capability::Tool { name: pattern } if glob_match(pattern, name))
}

fn cap_matches_file_read(cap: &Capability, path: &Path) -> bool {
    matches!(cap, Capability::FileRead { glob } if glob_match_path(glob, path))
}

fn cap_matches_file_write(cap: &Capability, path: &Path) -> bool {
    matches!(cap, Capability::FileWrite { glob } if glob_match_path(glob, path))
}

fn cap_matches_network(cap: &Capability, domain: &str) -> bool {
    matches!(
        cap,
        Capability::Network { domains }
            if domains.iter().any(|d| glob_match(d, domain))
    )
}

fn cap_matches_shell(cap: &Capability, command: &str) -> bool {
    matches!(
        cap,
        Capability::Shell { commands }
            if commands.iter().any(|c| c == command || glob_match(c, command))
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn tool_set(names: &[&str]) -> CapabilitySet {
        CapabilitySet::new(
            names
                .iter()
                .map(|n| Capability::Tool {
                    name: n.to_string(),
                })
                .collect(),
            vec![],
        )
    }

    // ── Empty set — all allows_* return false ─────────────────────────────────

    #[test]
    fn empty_set_denies_tool() {
        let s = CapabilitySet::default();
        assert!(!s.allows_tool("read_file"));
        assert!(!s.allows_tool("web_search"));
        assert!(!s.allows_tool("git_operations"));
    }

    #[test]
    fn empty_set_denies_file_read() {
        assert!(!CapabilitySet::default().allows_file_read(Path::new("src/main.rs")));
    }

    #[test]
    fn empty_set_denies_file_write() {
        assert!(!CapabilitySet::default().allows_file_write(Path::new("src/main.rs")));
    }

    #[test]
    fn empty_set_denies_network() {
        assert!(!CapabilitySet::default().allows_network("api.openai.com"));
    }

    #[test]
    fn empty_set_denies_shell() {
        assert!(!CapabilitySet::default().allows_shell("ls"));
    }

    #[test]
    fn empty_set_is_empty() {
        assert!(CapabilitySet::default().is_empty());
    }

    #[test]
    fn non_empty_set_is_not_empty() {
        assert!(!tool_set(&["web_search"]).is_empty());
    }

    // ── Tool grants ───────────────────────────────────────────────────────────

    #[test]
    fn tool_exact_name_granted() {
        let s = tool_set(&["web_search"]);
        assert!(s.allows_tool("web_search"));
        assert!(!s.allows_tool("web_fetch"));
    }

    #[test]
    fn tool_glob_prefix_matches() {
        let s = tool_set(&["cron_*"]);
        assert!(s.allows_tool("cron_list"));
        assert!(s.allows_tool("cron_add"));
        assert!(s.allows_tool("cron_delete"));
        assert!(!s.allows_tool("git_operations"));
    }

    #[test]
    fn tool_mcp_glob_matches_all_mcp_tools() {
        let s = tool_set(&["mcp__*"]);
        assert!(s.allows_tool("mcp__filesystem__read_file"));
        assert!(s.allows_tool("mcp__github__list_prs"));
        assert!(!s.allows_tool("git_operations"));
    }

    #[test]
    fn mcp_tool_name_format_matches_capability_pattern() {
        // Verify the double-underscore naming convention used by create_mcp_tools
        // is correctly matched by the mcp__* capability pattern.
        let s = CapabilitySet::new(
            vec![Capability::Tool {
                name: "mcp__*".to_string(),
            }],
            vec![],
        );
        for tool in &[
            "mcp__fs__read",
            "mcp__github__create_pr",
            "mcp__slack__send_message",
        ] {
            assert!(s.allows_tool(tool), "{tool} should be allowed by mcp__*");
        }
        // Per-server pattern
        let s2 = CapabilitySet::new(
            vec![Capability::Tool {
                name: "mcp__filesystem__*".to_string(),
            }],
            vec![],
        );
        assert!(s2.allows_tool("mcp__filesystem__read_file"));
        assert!(!s2.allows_tool("mcp__github__list_prs"));
        // Non-MCP tools are unaffected
        assert!(!s.allows_tool("web_search"));
    }

    #[test]
    fn tool_wildcard_matches_everything() {
        let s = tool_set(&["*"]);
        assert!(s.allows_tool("anything"));
        assert!(s.allows_tool("mcp:foo:bar"));
    }

    // ── Deny overrides grant ──────────────────────────────────────────────────

    #[test]
    fn deny_overrides_exact_grant() {
        let s = CapabilitySet::new(
            vec![Capability::Tool {
                name: "web_search".into(),
            }],
            vec![Capability::Tool {
                name: "web_search".into(),
            }],
        );
        assert!(!s.allows_tool("web_search"));
    }

    #[test]
    fn deny_glob_blocks_subset_of_granted_glob() {
        let s = CapabilitySet::new(
            vec![Capability::Tool {
                name: "cron_*".into(),
            }],
            vec![Capability::Tool {
                name: "cron_delete".into(),
            }],
        );
        assert!(s.allows_tool("cron_list"));
        assert!(s.allows_tool("cron_add"));
        assert!(!s.allows_tool("cron_delete"));
    }

    #[test]
    fn deny_without_matching_grant_has_no_extra_effect() {
        let s = CapabilitySet::new(
            vec![Capability::Tool {
                name: "web_search".into(),
            }],
            vec![Capability::Tool {
                name: "git_operations".into(),
            }],
        );
        assert!(s.allows_tool("web_search"));
        assert!(!s.allows_tool("git_operations")); // not granted anyway
    }

    // ── File read / write ─────────────────────────────────────────────────────

    #[test]
    fn file_read_glob_permits_matching_paths() {
        let s = CapabilitySet::new(
            vec![Capability::FileRead {
                glob: "src/**/*.rs".into(),
            }],
            vec![],
        );
        assert!(s.allows_file_read(Path::new("src/main.rs")));
        assert!(s.allows_file_read(Path::new("src/security/policy.rs")));
        assert!(!s.allows_file_read(Path::new("tests/integration.rs")));
        assert!(!s.allows_file_read(Path::new("Cargo.toml")));
    }

    #[test]
    fn file_read_wildcard_permits_all() {
        let s = CapabilitySet::new(
            vec![Capability::FileRead {
                glob: "**/*".into(),
            }],
            vec![],
        );
        assert!(s.allows_file_read(Path::new("anything/deep/file.txt")));
    }

    #[test]
    fn file_write_glob_permits_matching_paths() {
        let s = CapabilitySet::new(
            vec![Capability::FileWrite {
                glob: "content/**/*".into(),
            }],
            vec![],
        );
        assert!(s.allows_file_write(Path::new("content/post.md")));
        assert!(!s.allows_file_write(Path::new("src/main.rs")));
    }

    // ── Network ───────────────────────────────────────────────────────────────

    #[test]
    fn network_exact_domain() {
        let s = CapabilitySet::new(
            vec![Capability::Network {
                domains: vec!["api.anthropic.com".into()],
            }],
            vec![],
        );
        assert!(s.allows_network("api.anthropic.com"));
        assert!(!s.allows_network("evil.example.com"));
    }

    #[test]
    fn network_wildcard_subdomain() {
        let s = CapabilitySet::new(
            vec![Capability::Network {
                domains: vec!["*.openai.com".into()],
            }],
            vec![],
        );
        assert!(s.allows_network("api.openai.com"));
        assert!(s.allows_network("platform.openai.com"));
        assert!(!s.allows_network("evil.example.com"));
        assert!(!s.allows_network("openai.com")); // no wildcard match for root
    }

    // ── Shell ─────────────────────────────────────────────────────────────────

    #[test]
    fn shell_exact_command_match() {
        let s = CapabilitySet::new(
            vec![Capability::Shell {
                commands: vec!["ls".into(), "git".into()],
            }],
            vec![],
        );
        assert!(s.allows_shell("ls"));
        assert!(s.allows_shell("git"));
        assert!(!s.allows_shell("rm"));
    }

    // ── Intersect ─────────────────────────────────────────────────────────────

    #[test]
    fn intersect_of_two_empty_sets_is_empty() {
        let result = CapabilitySet::default().intersect(&CapabilitySet::default());
        assert!(result.is_empty());
    }

    #[test]
    fn intersect_empty_with_full_is_empty() {
        let empty = CapabilitySet::default();
        let full = tool_set(&["*"]);
        assert!(empty.intersect(&full).is_empty());
        assert!(full.intersect(&empty).is_empty());
    }

    #[test]
    fn intersect_keeps_only_shared_tools() {
        let a = tool_set(&["web_search", "git_operations", "cron_list"]);
        let b = tool_set(&["web_search", "cron_list"]);
        let c = a.intersect(&b);
        assert!(c.allows_tool("web_search"));
        assert!(c.allows_tool("cron_list"));
        assert!(!c.allows_tool("git_operations"));
    }

    #[test]
    fn intersect_merges_deny_lists_as_union() {
        let a = CapabilitySet::new(
            vec![Capability::Tool {
                name: "cron_*".into(),
            }],
            vec![Capability::Tool {
                name: "cron_delete".into(),
            }],
        );
        let b = CapabilitySet::new(
            vec![Capability::Tool {
                name: "cron_*".into(),
            }],
            vec![Capability::Tool {
                name: "cron_add".into(),
            }],
        );
        let c = a.intersect(&b);
        assert!(!c.allows_tool("cron_delete")); // from a's deny
        assert!(!c.allows_tool("cron_add")); // from b's deny
        assert!(c.allows_tool("cron_list")); // not denied by either
    }

    #[test]
    fn intersect_deny_in_either_operand_blocks_result() {
        let a = CapabilitySet::new(
            vec![Capability::Tool {
                name: "web_search".into(),
            }],
            vec![],
        );
        let b = CapabilitySet::new(
            vec![Capability::Tool {
                name: "web_search".into(),
            }],
            vec![Capability::Tool {
                name: "web_search".into(),
            }],
        );
        // b denies web_search → intersection cannot allow it
        let c = a.intersect(&b);
        assert!(!c.allows_tool("web_search"));
    }

    // ── Boolean mapping (Plan 46 — 21 entries) ────────────────────────────────

    macro_rules! bool_map_test_tool {
        ($test_name:ident, field = $field:ident, tool = $tool:expr) => {
            #[test]
            fn $test_name() {
                // All false: nothing allowed
                let all_false = CapabilitySet::from_policy_booleans(&PolicyBooleanFlags::default());
                assert!(!all_false.allows_tool($tool));

                // Only the relevant flag true
                let flags = PolicyBooleanFlags {
                    $field: true,
                    ..Default::default()
                };
                let set = CapabilitySet::from_policy_booleans(&flags);
                assert!(
                    set.allows_tool($tool),
                    "expected {} to allow {:?} when {} = true",
                    stringify!($test_name),
                    $tool,
                    stringify!($field),
                );
            }
        };
    }

    bool_map_test_tool!(
        bool_map_enable_git,
        field = enable_git,
        tool = "git_operations"
    );
    bool_map_test_tool!(
        bool_map_enable_cron,
        field = enable_cron,
        tool = "cron_list"
    );
    bool_map_test_tool!(
        bool_map_enable_web_search,
        field = enable_web_search,
        tool = "web_search"
    );
    bool_map_test_tool!(
        bool_map_enable_browser,
        field = enable_browser,
        tool = "browser"
    );
    bool_map_test_tool!(
        bool_map_enable_browser_open,
        field = enable_browser_open,
        tool = "browser_open"
    );
    bool_map_test_tool!(
        bool_map_enable_http_request,
        field = enable_http_request,
        tool = "http_request"
    );
    bool_map_test_tool!(
        bool_map_enable_web_fetch,
        field = enable_web_fetch,
        tool = "web_fetch"
    );
    bool_map_test_tool!(
        bool_map_enable_url_validation,
        field = enable_url_validation,
        tool = "url_validation"
    );
    bool_map_test_tool!(
        bool_map_enable_agents_ipc,
        field = enable_agents_ipc,
        tool = "agents_ipc"
    );
    bool_map_test_tool!(
        bool_map_enable_html_extract,
        field = enable_html_extract,
        tool = "html_extract"
    );
    bool_map_test_tool!(
        bool_map_enable_pushover,
        field = enable_pushover,
        tool = "pushover"
    );
    bool_map_test_tool!(
        bool_map_enable_code_interpreter,
        field = enable_code_interpreter,
        tool = "code_interpreter"
    );
    bool_map_test_tool!(
        bool_map_enable_autopilot_proposal,
        field = enable_autopilot,
        tool = "proposal_create"
    );
    bool_map_test_tool!(
        bool_map_enable_autopilot_mission,
        field = enable_autopilot,
        tool = "mission_status"
    );
    bool_map_test_tool!(
        bool_map_enable_agent_manage,
        field = enable_agent_manage,
        tool = "agent_manage"
    );
    bool_map_test_tool!(
        bool_map_enable_domain_tools,
        field = enable_domain_tools,
        tool = "domain_info"
    );
    bool_map_test_tool!(
        bool_map_enable_self_config,
        field = enable_self_config,
        tool = "config_manage"
    );
    bool_map_test_tool!(
        bool_map_enable_self_config_skill,
        field = enable_self_config,
        tool = "skill_manage"
    );
    bool_map_test_tool!(
        bool_map_enable_wasm_plugins,
        field = enable_wasm_plugins,
        tool = "wasm_exec"
    );
    bool_map_test_tool!(
        bool_map_enable_a2a_tool,
        field = enable_a2a_tool,
        tool = "a2a"
    );
    bool_map_test_tool!(
        bool_map_enable_dynamic_tools,
        field = enable_dynamic_tools,
        tool = "tool_create"
    );
    bool_map_test_tool!(
        bool_map_enable_mcp,
        field = enable_mcp,
        tool = "mcp__filesystem__read"
    );

    #[test]
    fn bool_map_enable_write_file_grants_file_write() {
        let flags = PolicyBooleanFlags {
            enable_write_file: true,
            ..Default::default()
        };
        let set = CapabilitySet::from_policy_booleans(&flags);
        assert!(set.allows_file_write(Path::new("src/any/file.rs")));
        assert!(set.allows_file_write(Path::new("README.md")));
    }

    #[test]
    fn bool_map_all_false_grants_nothing() {
        let set = CapabilitySet::from_policy_booleans(&PolicyBooleanFlags::default());
        assert!(set.is_empty());
        assert!(!set.allows_tool("anything"));
        assert!(!set.allows_file_write(Path::new("foo.txt")));
    }

    // ── Serialization round-trip ──────────────────────────────────────────────

    #[test]
    fn capability_tool_roundtrips_json() {
        let cap = Capability::Tool {
            name: "mcp:*".into(),
        };
        let json = serde_json::to_string(&cap).expect("serialize");
        let back: Capability = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cap, back);
    }

    #[test]
    fn capability_file_read_roundtrips_json() {
        let cap = Capability::FileRead {
            glob: "src/**/*.rs".into(),
        };
        let json = serde_json::to_string(&cap).expect("serialize");
        let back: Capability = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cap, back);
    }

    #[test]
    fn capability_set_roundtrips_json() {
        let set = CapabilitySet::new(
            vec![
                Capability::Tool {
                    name: "web_search".into(),
                },
                Capability::FileWrite {
                    glob: "**/*".into(),
                },
            ],
            vec![Capability::Tool {
                name: "cron_delete".into(),
            }],
        );
        let json = serde_json::to_string(&set).expect("serialize");
        let back: CapabilitySet = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(set.capabilities.len(), back.capabilities.len());
        assert_eq!(set.deny.len(), back.deny.len());
    }

    #[test]
    fn empty_capability_set_deserializes_from_empty_object() {
        let set: CapabilitySet = serde_json::from_str("{}").expect("deserialize");
        assert!(set.is_empty());
    }

    // ── Property tests ────────────────────────────────────────────────────────

    /// Arbitrary tool names for property testing (simple lowercase identifiers
    /// and colon-separated paths matching real tool naming conventions).
    fn arb_tool_name() -> impl Strategy<Value = String> {
        prop_oneof!["[a-z][a-z_]{0,19}", "[a-z][a-z_]{0,10}:[a-z][a-z_]{0,10}",]
    }

    /// Arbitrary tool capability (possibly a glob).
    fn arb_tool_cap() -> impl Strategy<Value = Capability> {
        prop_oneof![
            arb_tool_name().prop_map(|n| Capability::Tool { name: n }),
            "[a-z][a-z_]{0,8}\\*".prop_map(|n| Capability::Tool { name: n }),
        ]
    }

    proptest! {
        /// Intersection invariant: every tool allowed by the intersection must
        /// be allowed by BOTH operands individually.
        #[test]
        fn prop_intersection_is_subset_of_both(
            grant_a in proptest::collection::vec(arb_tool_cap(), 0..8usize),
            grant_b in proptest::collection::vec(arb_tool_cap(), 0..8usize),
            probe_names in proptest::collection::vec(arb_tool_name(), 0..20usize),
        ) {
            let a = CapabilitySet::new(grant_a, vec![]);
            let b = CapabilitySet::new(grant_b, vec![]);
            let c = a.intersect(&b);

            for name in &probe_names {
                if c.allows_tool(name) {
                    prop_assert!(
                        a.allows_tool(name),
                        "intersection allows '{}' but 'a' does not",
                        name
                    );
                    prop_assert!(
                        b.allows_tool(name),
                        "intersection allows '{}' but 'b' does not",
                        name
                    );
                }
            }
        }

        /// Deny-overrides-grant invariant: when the same capability appears in
        /// both grants and deny, it must never be allowed.
        #[test]
        fn prop_deny_overrides_grant(
            name in arb_tool_name(),
        ) {
            let cap = Capability::Tool { name: name.clone() };
            let set = CapabilitySet::new(
                vec![cap.clone()],
                vec![cap],
            );
            prop_assert!(
                !set.allows_tool(&name),
                "deny should have overridden grant for '{}'",
                name
            );
        }

        /// Empty set invariant: an empty capability set must deny every tool.
        #[test]
        fn prop_empty_set_denies_all(name in arb_tool_name()) {
            let set = CapabilitySet::default();
            prop_assert!(!set.allows_tool(&name));
        }

        /// Intersection commutativity for the allows_tool predicate: the set
        /// of allowed tools must be the same regardless of operand order.
        #[test]
        fn prop_intersection_allows_same_tools_both_orders(
            grant_a in proptest::collection::vec(arb_tool_cap(), 0..8usize),
            grant_b in proptest::collection::vec(arb_tool_cap(), 0..8usize),
            probe_names in proptest::collection::vec(arb_tool_name(), 0..20usize),
        ) {
            let a = CapabilitySet::new(grant_a, vec![]);
            let b = CapabilitySet::new(grant_b, vec![]);
            let ab = a.intersect(&b);
            let ba = b.intersect(&a);

            for name in &probe_names {
                prop_assert_eq!(
                    ab.allows_tool(name),
                    ba.allows_tool(name),
                    "intersection not commutative for '{}'",
                    name
                );
            }
        }

    }
    #[test]
    fn allows_memory_empty_set_permits_all() {
        let s = CapabilitySet::default();
        assert!(s.allows_memory("default"));
        assert!(s.allows_memory("private"));
        assert!(s.allows_memory("any_namespace"));
    }

    #[test]
    fn allows_memory_full_scope_permits_all() {
        let s = CapabilitySet::new(vec![Capability::Memory { scope: None }], vec![]);
        assert!(s.allows_memory("default"));
        assert!(s.allows_memory("private"));
    }

    #[test]
    fn allows_memory_scoped_permits_only_own_namespace() {
        let s = CapabilitySet::new(
            vec![Capability::Memory {
                scope: Some("agent_a".to_string()),
            }],
            vec![],
        );
        assert!(s.allows_memory("agent_a"));
        assert!(!s.allows_memory("default"));
        assert!(!s.allows_memory("agent_b"));
    }

    #[test]
    fn allows_memory_no_memory_cap_in_nonempty_set_denies() {
        // A non-empty capability set without any Memory grant should deny memory access.
        let s = CapabilitySet::new(
            vec![Capability::Tool {
                name: "web_search".to_string(),
            }],
            vec![],
        );
        assert!(!s.allows_memory("default"));
    }

    #[test]
    fn delegate_ceiling_empty_when_no_delegate_cap() {
        let s = CapabilitySet::new(
            vec![Capability::Tool {
                name: "web_search".to_string(),
            }],
            vec![],
        );
        assert!(s.delegate_ceiling().is_empty());
    }

    #[test]
    fn delegate_ceiling_built_from_delegate_caps() {
        let s = CapabilitySet::new(
            vec![Capability::Delegate {
                max_capabilities: vec![
                    Capability::Tool {
                        name: "web_search".to_string(),
                    },
                    Capability::Memory { scope: None },
                ],
            }],
            vec![],
        );
        let ceiling = s.delegate_ceiling();
        assert!(!ceiling.is_empty());
        assert!(ceiling.allows_tool("web_search"));
        assert!(!ceiling.allows_tool("shell"));
        assert!(ceiling.allows_memory("any_namespace"));
    }
}
