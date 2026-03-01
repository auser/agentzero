---
title: Tools & Plugins
description: Built-in tools, security policy, WASM plugin system, and skills.
---

AgentZero ships with a set of built-in tools and supports extension via WASM plugins and skills. Every tool enforces **fail-closed security** — capabilities are denied unless explicitly enabled.

## Built-in Tools

| Tool | Description | Default | Config |
|---|---|---|---|
| `read_file` | Read file contents within allowed root | Enabled | `[security.read_file]` |
| `write_file` | Write file contents within allowed root | **Disabled** | `[security.write_file]` |
| `shell` | Execute allowlisted shell commands | Enabled (allowlist) | `[security.shell]` |
| `http_request` | Make HTTP requests to allowed domains | **Disabled** | `[http_request]` |
| `web_fetch` | Fetch and convert web pages to markdown | **Disabled** | `[web_fetch]` |
| `web_search` | Search the web via DuckDuckGo/Brave/etc | **Disabled** | `[web_search]` |
| `browser` | Browser automation and screenshot | **Disabled** | `[browser]` |
| `memory` | Query and manage agent memory | Enabled | `[memory]` |
| `delegate` | Spawn sub-agent with scoped tools | Enabled | `[agents.*]` |
| `apply_patch` | Validate and apply structured patches | **Disabled** | `[apply_patch]` |
| `model_routing` | Query model routing configuration | Enabled | `[routing]` |

## Tool Trait

All tools implement the core `Tool` trait:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult>;
}
```

The `ToolContext` carries workspace-scoped security state:

```rust
pub struct ToolContext {
    pub workspace_root: String,
    pub allow_sensitive_file_reads: bool,
    pub allow_sensitive_file_writes: bool,
}
```

---

## File Tools

### `read_file`

Reads text file contents with full path-safety enforcement.

**Input:** Relative file path (e.g., `src/main.rs`)

**Security controls:**

- **Path traversal prevention** — rejects `..` components and absolute paths
- **Canonicalization** — resolves symlinks and validates the final path stays within `allowed_root`
- **Hard-link guard (B7)** — blocks files with multiple hard links to prevent symlink attacks
- **Binary detection** — rejects files containing null bytes or non-UTF8 content
- **Sensitive file detection** — blocks `.env`, `.aws/credentials`, `.ssh/id_rsa`, `.gnupg/`, `credentials.json`, etc. unless `allow_sensitive_file_reads` is true
- **Size cap** — 64 KiB per read (configurable)

```toml
[security.read_file]
allowed_root = "."          # workspace root
max_read_bytes = 65536      # 64 KiB
allow_binary = false
```

### `write_file`

Writes text files with the same path-safety enforcement as `read_file`.

**Input:** JSON payload:

```json
{
  "path": "src/output.txt",
  "content": "file contents here",
  "overwrite": false,
  "dry_run": false
}
```

**Security controls:**

- **Disabled by default** — requires explicit `enabled = true`
- All `read_file` protections apply (path traversal, canonicalization, hard-link guard, sensitive file detection)
- **Existence check** — errors if file exists and `overwrite: false`
- **Dry-run mode** — preview capability without writing to disk

**Output:** `dry_run={bool} path={path} bytes={count} overwrite={bool}`

```toml
[security.write_file]
enabled = true
allowed_root = "."
max_write_bytes = 65536     # 64 KiB
```

### `apply_patch`

Validates and applies structured patches using a strict envelope format.

**Input:** Patch content with BEGIN/END markers:

```
*** Begin Patch
*** Update File: src/main.rs
@@ ... patch content ...
*** End Patch
```

**Security:** Validates patch structure before any file modification. Rejects malformed envelopes.

---

## Shell Tool

### `shell`

Executes shell commands with **allowlist-driven** security. Only explicitly permitted commands can run.

**Input:** Command string (e.g., `ls -la src/`)

**Security controls:**

- **Command allowlist** — only commands listed in `allowed_commands` can execute
- **Default allowlist:** `ls`, `pwd`, `cat`, `echo`
- **Quote-aware validation** — metacharacters (`; & | > < $`) are forbidden when **unquoted**, but allowed inside single/double quotes
- **Always forbidden** — backtick (`` ` ``) and null byte are blocked even inside quotes
- **Argument limits** — max 8 arguments of 128 bytes each
- **Output truncation** — stdout/stderr capped at 8 KiB with truncation notice

**Output:** `status={code}\nstdout:\n...\nstderr:\n...`

**Examples:**

```bash
# Allowed — semicolon is inside single quotes
echo 'hello;world'

# Blocked — unquoted semicolon is shell injection
echo hello;world

# Blocked — backtick is always forbidden
echo `whoami`
```

**Shell tokenizer:** The command parser performs quote-aware tokenization supporting single quotes (no interpretation), double quotes (with escape handling), and backslash escapes. Each character's quoting context is tracked for policy validation.

```toml
[security]
allowed_commands = ["ls", "pwd", "cat", "echo", "grep", "find", "git"]

[security.shell]
max_args = 8
max_arg_length = 128
max_output_bytes = 8192
forbidden_chars = ";&|><$`\n\r"
```

---

## Network Tools

All network tools share the **URL Access Policy** for SSRF prevention.

### URL Access Policy

```toml
[url_access]
block_private_ip = true        # blocks 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
allow_loopback = false         # blocks 127.0.0.0/8
enforce_domain_allowlist = false
domain_allowlist = []
domain_blocklist = []
```

**Private IP ranges blocked:**

| Range | Description |
|---|---|
| `10.0.0.0/8` | Private Class A |
| `172.16.0.0/12` | Private Class B |
| `192.168.0.0/16` | Private Class C |
| `169.254.0.0/16` | Link-local |
| `100.64.0.0/10` | Carrier-grade NAT |
| `0.0.0.0/8` | Unspecified |
| `240.0.0.0/4` | Reserved |
| `fc00::/7` | IPv6 unique local |
| `fe80::/10` | IPv6 link-local |

**DNS rebinding protection:** Domain names are resolved to IP addresses and checked against the private IP blocklist. This prevents attackers from registering a domain, then changing DNS to point to an internal IP.

### `http_request`

Performs HTTP requests (GET, POST, PUT, DELETE) to allowed domains.

**Input:** `<METHOD> <URL> [JSON_BODY]`

```
GET https://api.example.com/data
POST https://api.example.com/items {"name": "item1"}
```

**Security pipeline:**

```
Input → URL Parse → Scheme Check (http/https only) →
Policy Check (blocklist) → IP Resolution → Private IP Check →
DNS Rebinding Check → Domain Allowlist Check → Execute
```

```toml
[http_request]
enabled = true
allowed_domains = ["api.example.com", "*.internal.dev"]
max_response_size = 1000000   # 1 MB
timeout_secs = 30
```

### `web_fetch`

Fetches content from URLs and returns the response body.

**Input:** URL string

**Output:** `status={code}\n{body}`

**Security:** Same URL access policy as `http_request`. Automatic response truncation at 64 KiB default.

### `web_search`

Searches the web via configurable provider.

```toml
[web_search]
enabled = true
provider = "duckduckgo"       # duckduckgo, brave, perplexity, exa, jina
max_results = 5
timeout_secs = 15
```

---

## Agent Tools

### `delegate`

Spawns a sub-agent with scoped tools. Prevents infinite delegation chains.

**Input:**

```json
{
  "agent": "researcher",
  "prompt": "Find the latest Rust async runtime benchmarks"
}
```

**Security controls:**

- **Depth limiting** — `max_depth` prevents infinite delegation chains
- **Tool blocklist** — the `delegate` tool itself is forbidden in sub-agent tool lists (prevents recursion)
- **Scoped tool access** — each delegate configuration specifies its own `allowed_tools`

**Per-agent configuration:**

```toml
[agents.researcher]
provider = "https://api.openai.com/v1"
model = "gpt-4"
max_depth = 2
agentic = true
max_iterations = 10
allowed_tools = ["read_file", "web_search", "memory"]
```

### `agents_ipc`

Inter-process communication between agents with encrypted storage.

**Operations:**

| Operation | Input | Description |
|---|---|---|
| `send` | `{"op":"send","from":"a1","to":"a2","payload":"msg"}` | Send message between agents |
| `recv` | `{"op":"recv","to":"agent_name"}` | Receive pending messages |
| `list` | `{"op":"list","to":"?","from":"?","limit":"?"}` | List messages with filters |
| `clear` | `{"op":"clear","to":"?","from":"?"}` | Clear messages |

**Security:** All IPC data is encrypted at rest using `EncryptedJsonStore`. Messages include timestamps for audit trail.

### `model_routing`

Queries the model routing configuration to determine which provider/model handles a request.

**Operations:**

| Operation | Description |
|---|---|
| `list_routes` | Returns all model route hints |
| `list_embedding_routes` | Returns all embedding route hints |
| `resolve_hint` | Find route by hint name |
| `classify_query` | Classify query to appropriate hint |
| `route_query` | Complete routing decision for query |

---

## Autonomy Levels

Tools are gated by the autonomy policy, which controls what requires user approval:

| Level | Read Tools | Write Tools | Network Tools |
|---|---|---|---|
| `ReadOnly` | Auto-approve | **Blocked** | **Blocked** |
| `Supervised` | Auto-approve | Requires approval | Requires approval |
| `Full` | Auto-approve | Auto-approve | Auto-approve |

**Read tools (auto-approved):** `read_file`, `glob`, `search`, `memory_read`

**Write tools (gated):** `write_file`, `shell`, `apply_patch`, `browser`, `http_request`

**Forbidden paths (all levels):** `/etc`, `/root`, `/proc`, `/sys`, `~/.ssh`, `~/.gnupg`, `~/.aws`

```toml
[autonomy]
level = "supervised"
workspace_only = true
forbidden_paths = ["/etc", "/root", "/proc", "/sys"]
auto_approve = ["read_file", "memory"]
always_ask = ["shell", "write_file"]
allow_sensitive_file_reads = false
allow_sensitive_file_writes = false
```

---

## WASM Plugins

AgentZero runs plugins in a sandboxed WASM environment with strict resource limits.

### Plugin Structure

```
my-plugin/
├── manifest.json    # metadata + capabilities
└── plugin.wasm      # compiled WASM module
```

### Plugin Lifecycle

```bash
# Scaffold a new plugin manifest
agentzero plugin new --id my-plugin

# Validate manifest
agentzero plugin validate --manifest manifest.json

# Test plugin (preflight + optional execution)
agentzero plugin test --manifest manifest.json --wasm plugin.wasm --execute

# Package for distribution
agentzero plugin package --manifest manifest.json --wasm plugin.wasm --out my-plugin.tar.gz

# Install a packaged plugin
agentzero plugin install --package my-plugin.tar.gz

# List installed plugins
agentzero plugin list

# Remove
agentzero plugin remove --id my-plugin
```

### Security Controls

```toml
[runtime.wasm]
fuel_limit = 1000000         # execution budget
memory_limit_mb = 64         # max memory
max_module_size_mb = 50      # max .wasm file size
allow_workspace_read = false
allow_workspace_write = false
allowed_hosts = []           # network access allowlist

[runtime.wasm.security]
require_workspace_relative_tools_dir = true
reject_symlink_modules = true
reject_symlink_tools_dir = true
capability_escalation_mode = "deny"
module_hash_policy = "warn"  # warn or enforce
```

Plugin integrity is verified via SHA-256 checksums at install time. Tampered packages are rejected.

### WASM Isolation Policy

- Network: **disabled** by default
- Filesystem write: **disabled** by default
- Bounded execution: fuel limits cap CPU usage
- Bounded memory: configurable max memory
- Symlink rejection: prevents escape from tools directory
- Capability escalation: denied by default

---

## Skills

Skills are higher-level composable behaviors built on top of tools.

```bash
# List installed skills
agentzero skill list

# Install a skill
agentzero skill install --name my-skill --source local

# Test a skill
agentzero skill test --name my-skill

# Remove a skill
agentzero skill remove --name my-skill
```

---

## MCP (Model Context Protocol)

MCP tool servers can be integrated when enabled. Servers must be explicitly allowlisted.

```toml
[security.mcp]
enabled = true
allowed_servers = ["filesystem", "github"]
```

---

## Security Defaults Summary

| Component | Setting | Default |
|---|---|---|
| `read_file` max size | `max_read_bytes` | 64 KiB |
| `write_file` max size | `max_write_bytes` | 64 KiB |
| `write_file` enabled | `enabled` | `false` |
| `shell` max args | `max_args` | 8 |
| `shell` max arg length | `max_arg_length` | 128 bytes |
| `shell` max output | `max_output_bytes` | 8 KiB |
| `web_fetch` max size | `max_bytes` | 64 KiB |
| WASM fuel limit | `fuel_limit` | 1,000,000 |
| WASM memory limit | `memory_limit_mb` | 64 MB |
| Private IP blocking | `block_private_ip` | `true` |
| Loopback access | `allow_loopback` | `false` |
| Sensitive files | `allow_sensitive_file_reads` | `false` |
