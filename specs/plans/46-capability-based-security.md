# Plan 46: Capability-Based Security Model

## Problem

`ToolSecurityPolicy` has 33 fields — mostly flat booleans (`enable_git`, `enable_browser`, etc.) plus a few structured policies (`ReadFilePolicy`, `ShellPolicy`). This design has three problems:

1. **Doesn't compose.** A sub-agent inherits the parent's entire policy or nothing. You can't say "this agent can read files in /data but not /etc" without a separate policy struct.
2. **Doesn't scale.** Every new tool adds another boolean. Removing tools (Sprint 85) required touching 6 files to remove each flag.
3. **Doesn't cover MCP/A2A.** MCP sessions and A2A external agents inherit the full server's policy — no per-session scoping.

The YAML security policy (`SecurityPolicyFile`) is a step toward granularity but lives outside the core and only covers egress/commands/filesystem per tool name.

## Design

Replace the 20+ `enable_*` booleans with a **capability set** — a collection of typed, parameterized permissions that compose via intersection.

### Capability Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Capability {
    /// Read files matching glob within workspace
    FileRead { glob: String },
    /// Write files matching glob within workspace
    FileWrite { glob: String },
    /// Execute shell commands from allowlist
    Shell { commands: Vec<String> },
    /// HTTP/WebSocket access to domains matching glob
    Network { domains: Vec<String> },
    /// Access specific tool by name (supports glob: "mcp:*", "cron_*")
    Tool { name: String },
    /// Access memory store with optional scope
    Memory { scope: Option<String> },
    /// Spawn sub-agents with at-most these capabilities
    Delegate { max_capabilities: Vec<Capability> },
}
```

### Capability Set

```rust
pub struct CapabilitySet {
    capabilities: Vec<Capability>,
    deny: Vec<Capability>,  // explicit denials override grants
}

impl CapabilitySet {
    /// Intersection: result has only capabilities present in BOTH sets.
    /// Used for sub-agent delegation — child never exceeds parent.
    pub fn intersect(&self, other: &CapabilitySet) -> CapabilitySet;

    /// Check if a specific action is permitted.
    pub fn allows_tool(&self, tool_name: &str) -> bool;
    pub fn allows_file_read(&self, path: &Path) -> bool;
    pub fn allows_file_write(&self, path: &Path) -> bool;
    pub fn allows_network(&self, domain: &str) -> bool;
    pub fn allows_shell(&self, command: &str) -> bool;
}
```

### TOML Config

```toml
# Current (boolean flags):
[security]
enable_git = true
enable_web_search = true
enable_browser = false

# Proposed (capability grants):
[[capabilities]]
type = "tool"
name = "git_operations"

[[capabilities]]
type = "tool"
name = "web_search"

[[capabilities]]
type = "network"
domains = ["*.duckduckgo.com", "api.openai.com"]

[[capabilities]]
type = "file_read"
glob = "**/*"

[[capabilities]]
type = "file_write"
glob = "src/**/*.rs"

[[capabilities]]
type = "shell"
commands = ["ls", "pwd", "cat", "git", "cargo"]
```

### Per-Agent Capabilities

```toml
[agents.researcher]
system_prompt = "You are a research agent..."
capabilities = [
    { type = "tool", name = "web_search" },
    { type = "tool", name = "web_fetch" },
    { type = "tool", name = "memory_*" },
    { type = "network", domains = ["*"] },
]

[agents.writer]
system_prompt = "You write content..."
capabilities = [
    { type = "tool", name = "memory_recall" },
    { type = "file_write", glob = "content/**/*" },
]
```

### Per-MCP-Session Capabilities

When an MCP client connects, it gets a scoped capability set — not the full server's tools:

```toml
[mcp_sessions.claude_desktop]
capabilities = [
    { type = "tool", name = "read_file" },
    { type = "tool", name = "write_file" },
    { type = "tool", name = "shell" },
    { type = "file_read", glob = "**/*" },
    { type = "file_write", glob = "src/**/*" },
    { type = "shell", commands = ["ls", "cat", "git"] },
]
```

### A2A Capability Negotiation

External A2A agents declare required capabilities in their Agent Card. The server grants at most the intersection of (requested, configured):

```toml
[a2a.agents.external-researcher]
url = "https://researcher.example.com"
max_capabilities = [
    { type = "tool", name = "web_search" },
    { type = "tool", name = "memory_store" },
    { type = "network", domains = ["*.example.com"] },
]
```

### Composition Rules

1. **Child never exceeds parent.** When agent A delegates to agent B, B's capabilities = intersection(A's capabilities, B's configured capabilities).
2. **Deny overrides grant.** An explicit deny in the capability set blocks even if a matching grant exists.
3. **Privacy mode narrows.** `private` mode removes all `Network` capabilities except configured provider domains. `local_only` removes all `Network` capabilities entirely.
4. **YAML policy overlays.** `SecurityPolicyFile` rules are evaluated after capability checks — they can further restrict but never expand.

### Migration Path

Phase 1 (backward compatible):
- Add `capabilities: Vec<Capability>` to config alongside existing `enable_*` fields
- When `capabilities` is empty (default), fall back to boolean flags (current behavior)
- When `capabilities` is non-empty, ignore `enable_*` booleans entirely
- `ToolSecurityPolicy` gains a `CapabilitySet` field

Phase 2 (deprecation):
- Log warnings when `enable_*` booleans are used
- Auto-convert booleans to capabilities in config loader

Phase 3 (removal):
- Remove `enable_*` boolean fields from `ToolSecurityPolicy`
- Remove boolean mappings from `policy.rs`

### Mapping: Current Booleans to Capabilities

| Boolean Flag | Equivalent Capability |
|---|---|
| `enable_git` | `Tool { name: "git_operations" }` |
| `enable_cron` | `Tool { name: "cron_*" }` |
| `enable_web_search` | `Tool { name: "web_search" }` |
| `enable_browser` | `Tool { name: "browser" }` |
| `enable_browser_open` | `Tool { name: "browser_open" }` |
| `enable_http_request` | `Tool { name: "http_request" }` |
| `enable_web_fetch` | `Tool { name: "web_fetch" }` |
| `enable_url_validation` | `Tool { name: "url_validation" }` |
| `enable_agents_ipc` | `Tool { name: "agents_ipc" }` |
| `enable_html_extract` | `Tool { name: "html_extract" }` |
| `enable_pushover` | `Tool { name: "pushover" }` |
| `enable_code_interpreter` | `Tool { name: "code_interpreter" }` |
| `enable_autopilot` | `Tool { name: "proposal_*" }` + `Tool { name: "mission_*" }` |
| `enable_agent_manage` | `Tool { name: "agent_manage" }` |
| `enable_domain_tools` | `Tool { name: "domain_*" }` |
| `enable_self_config` | `Tool { name: "config_manage" }` + `Tool { name: "skill_manage" }` |
| `enable_wasm_plugins` | `Tool { name: "wasm_*" }` |
| `enable_a2a_tool` | `Tool { name: "a2a" }` |
| `enable_dynamic_tools` | `Tool { name: "tool_create" }` |
| `enable_write_file` | `FileWrite { glob: "**/*" }` |
| `enable_mcp` | `Tool { name: "mcp:*" }` |

### Threat Model Additions

Attack surfaces that must be covered by capability checks:

1. **MCP server mode** — stdio/HTTP clients get full tool access. Must scope per-session.
2. **A2A protocol** — external agents can submit tasks. Must enforce capability negotiation.
3. **Autopilot self-modification** — proposals can create new agents or tools. Must enforce that created entities never exceed the autopilot's own capabilities.
4. **Memory poisoning** — agents can write to shared memory. Must scope memory access per agent.
5. **Dynamic tool creation** — codegen creates WASM tools at runtime. Must inherit creator's capabilities, not server-wide.

### Property Tests for Capabilities

```rust
// Intersection is always a subset of both inputs
proptest! {
    fn intersection_never_exceeds_either(a: CapabilitySet, b: CapabilitySet) {
        let c = a.intersect(&b);
        // For every capability in c, it must be in both a and b
        for cap in c.capabilities() {
            assert!(a.allows(cap) && b.allows(cap));
        }
    }

    // Deny always wins over grant
    fn deny_overrides_grant(cap: Capability) {
        let set = CapabilitySet::new(vec![cap.clone()], vec![cap.clone()]);
        assert!(!set.allows(&cap));
    }
}
```

### Files to Modify

- `crates/agentzero-core/src/security/capability.rs` — new module: `Capability`, `CapabilitySet`
- `crates/agentzero-tools/src/lib.rs` — add `CapabilitySet` to `ToolSecurityPolicy`
- `crates/agentzero-config/src/model.rs` — add `capabilities` to `AgentZeroConfig` + per-agent
- `crates/agentzero-config/src/policy.rs` — build `CapabilitySet` from config, fallback to booleans
- `crates/agentzero-infra/src/tools/mod.rs` — check capabilities instead of booleans in `default_tools_inner`
- `crates/agentzero-infra/src/mcp_server.rs` — per-session capability scoping
- `crates/agentzero-gateway/src/a2a.rs` — capability negotiation on task submission
- `crates/agentzero-core/src/delegation.rs` — intersection on sub-agent creation

### Estimated Effort

- Phase 1 (backward compat): 2-3 sprints
- Phase 2 (deprecation): 1 sprint
- Phase 3 (removal): 1 sprint

This is architectural — get Phase 1 right, Phase 2-3 are mechanical.
