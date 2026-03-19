# Enterprise Security & Routing Enhancements

## Context

NVIDIA announced an enterprise security wrapper around OpenClaw at GTC 2026 that adds declarative YAML security policies, container-level sandboxing, operator-approval for unknown egress, and a "privacy router" for routing inference between local and cloud models. Analysis reveals three actionable gaps in AgentZero's security and routing layers, prioritized by effort and impact.

**Source:** GTC 2026 keynote, NVIDIA OpenShell blog.

---

## Step 0: Housekeeping (do when starting implementation)

1. Checkout branch `feat/enterprise-security-routing`
2. Update `specs/SPRINT.md` with Sprint 58/59 entries
3. Keep `specs/SPRINT.md` up to date throughout implementation

---

## Sprint 58: Privacy-Aware Model Routing + Declarative YAML Security Policies (parallel tracks)

### Track A: Privacy-Aware Model Routing (S-M)

Connect the existing `ModelRouter` (keyword/pattern-based routing to providers) with the privacy mode system (`off`/`private`/`local_only`/`encrypted`/`full`). Currently these are disconnected — model routing doesn't know about privacy modes, and privacy enforcement only disables tools, not inference routing.

NVIDIA's "privacy router" automatically keeps sensitive queries on local models and only routes to cloud when necessary. We can do the same by adding a `privacy_level` to model routes and filtering during routing.

**Modified files:**

- `crates/agentzero-core/src/routing.rs`:
  - Add `PrivacyLevel` enum: `Local`, `Cloud`, `Either` (default)
  - Add `privacy_level: PrivacyLevel` to `ModelRoute` (default `Either` for backward compat)
  - Add `privacy_level: Option<PrivacyLevel>` to `ClassificationRule` (optional override per rule)
  - New method: `route_query_with_privacy(&self, query: &str, mode: &str) -> Option<ResolvedRoute>`:
    - When mode is `"local_only"`: only consider routes where `privacy_level` is `Local`
    - When mode is `"private"`: prefer `Local` routes, fall through to `Cloud` only if no local match
    - When mode is `"off"`: all routes eligible (current behavior)
  - New method: `resolve_hint_with_privacy(&self, hint: &str, mode: &str) -> Option<ResolvedRoute>`:
    - Same filtering logic applied to explicit hint resolution

- `crates/agentzero-tools/src/model_routing_config.rs`:
  - Add `privacy_mode: Option<String>` to `Input` struct
  - New op: `"route_query_private"` — calls `route_query_with_privacy()`
  - Update `"list_routes"` to include `privacy_level` in output

- `crates/agentzero-infra/src/runtime.rs`:
  - When building provider calls, use `route_query_with_privacy()` instead of `route_query()` when privacy mode is set
  - Pass the active privacy mode from config through to the router

- `crates/agentzero-config/src/model.rs`:
  - Add `privacy_level` to model route config parsing (TOML)
  - Example config:
    ```toml
    [[routing.model_routes]]
    hint = "fast-local"
    provider = "ollama"
    model = "llama3.2"
    privacy_level = "local"

    [[routing.model_routes]]
    hint = "fast-cloud"
    provider = "anthropic"
    model = "claude-sonnet-4-20250514"
    privacy_level = "cloud"

    [[routing.model_routes]]
    hint = "reasoning"
    provider = "openai"
    model = "o1"
    privacy_level = "either"
    ```

**Tests (6+):**
- `private_mode_prefers_local_route` — With both local and cloud routes, private mode picks local
- `private_mode_falls_through_to_cloud` — When no local route matches hint, private mode allows cloud
- `local_only_mode_blocks_cloud` — `local_only` mode never selects cloud routes
- `off_mode_allows_all` — Default behavior unchanged
- `privacy_level_default_either` — Routes without explicit privacy_level default to `Either`
- `classification_rule_privacy_override` — Classification rule can force a privacy level

**Effort:** Small-Medium. Natural extension of two existing systems.

---

### Track B: Declarative YAML Security Policy File (M-L)

Add support for a standalone `security-policy.yaml` file that provides per-tool egress/filesystem/command rules, complementing the existing TOML-based `ToolSecurityPolicy`. This is the change with the highest enterprise value — a single, auditable, version-controllable policy file.

**Current state:**
- `ToolSecurityPolicy` in `agentzero-tools/src/lib.rs` is a flat struct of ~30 boolean flags + `UrlAccessPolicy`/`ShellPolicy`/`ReadFilePolicy`/`WriteFilePolicy`
- Policy is loaded from `agentzero.toml` sections (`[security]`, `[security.url_access]`, etc.) in `policy.rs`
- `UrlAccessPolicy` already supports domain allowlists/blocklists, CIDR ranges, and `require_first_visit_approval`
- `ShellPolicy` already supports command allowlists and `ShellCommandPolicy`

**Design:**
The YAML policy file sits alongside `agentzero.toml` at `.agentzero/security-policy.yaml`. When present, it **overrides** the TOML security section for per-tool rules. The TOML remains the source of truth for global flags (`enable_mcp`, `enable_git`, etc.) — the YAML adds granularity.

**New files:**

- `crates/agentzero-config/src/security_policy.rs` (~300 lines):
  ```rust
  /// Declarative per-tool security policy loaded from YAML.
  #[derive(Debug, Clone, Deserialize)]
  pub struct SecurityPolicyFile {
      /// Default action when no rule matches: "allow" or "deny"
      pub default: DefaultAction,
      /// Per-tool rules, evaluated in order
      pub rules: Vec<ToolRule>,
  }

  #[derive(Debug, Clone, Deserialize)]
  pub struct ToolRule {
      /// Tool name or glob pattern (e.g., "http_request", "mcp:*", "shell")
      pub tool: String,
      /// Allowed egress domains/IPs. Empty = no network access.
      #[serde(default)]
      pub egress: Vec<EgressTarget>,
      /// Allowed shell commands (for shell tool only)
      #[serde(default)]
      pub commands: Vec<String>,
      /// Allowed filesystem paths (for read/write file tools)
      #[serde(default)]
      pub filesystem: Vec<String>,
      /// "allow", "deny", or "prompt" (ask operator on first use)
      #[serde(default = "default_action_allow")]
      pub action: String,
  }

  #[derive(Debug, Clone, Deserialize)]
  #[serde(untagged)]
  pub enum EgressTarget {
      /// Exact domain or glob: "api.openai.com", "*.github.com"
      Domain(String),
      /// CIDR range: "10.0.0.0/8"
      Cidr(String),
      /// Special: "prompt" means ask operator on first access
      Prompt,  // string literal "prompt"
  }
  ```

  Functions:
  - `load_security_policy(workspace_root: &Path) -> Option<SecurityPolicyFile>` — reads `.agentzero/security-policy.yaml` if present
  - `SecurityPolicyFile::evaluate(&self, tool_name: &str, target: &EgressCheck) -> PolicyDecision` — returns `Allow`, `Deny`, or `Prompt`
  - `EgressCheck` enum: `Domain(String)`, `Cidr(String)`, `Command(String)`, `FilePath(PathBuf)`

- `.agentzero/security-policy.yaml` example file:
  ```yaml
  # AgentZero Security Policy
  # When present, provides per-tool egress and access rules.
  # Global flags (enable_mcp, enable_git, etc.) remain in agentzero.toml.

  default: deny

  rules:
    # Allow HTTP requests only to specific API endpoints
    - tool: http_request
      egress:
        - api.openai.com
        - api.anthropic.com
        - "*.ollama.local"
      action: allow

    # Allow web fetch for documentation sites
    - tool: web_fetch
      egress:
        - "*.github.com"
        - docs.rs
        - "*.readthedocs.io"
      action: allow

    # Shell: only git and cargo
    - tool: shell
      commands: [git, cargo, rustc, npm]
      filesystem: [/workspace, /tmp]
      action: allow

    # MCP tools: prompt operator on first use of each server
    - tool: "mcp:*"
      egress: prompt
      action: prompt

    # File tools: workspace only
    - tool: read_file
      filesystem: [/workspace]
      action: allow

    - tool: write_file
      filesystem: [/workspace]
      action: allow

    # Block everything else by default (from `default: deny`)
  ```

**Modified files:**

- `crates/agentzero-config/src/policy.rs`:
  - After loading `ToolSecurityPolicy` from TOML, attempt to load `security-policy.yaml`
  - If found, create a `YamlSecurityOverlay` that wraps the base policy
  - Add `yaml_overlay: Option<SecurityPolicyFile>` to the returned policy or create a new wrapper struct

- `crates/agentzero-tools/src/lib.rs`:
  - Add `yaml_policy: Option<SecurityPolicyFile>` to `ToolSecurityPolicy`
  - Add method: `check_egress(&self, tool_name: &str, domain: &str) -> PolicyDecision`
  - Add method: `check_command(&self, command: &str) -> PolicyDecision`
  - Add method: `check_filesystem(&self, tool_name: &str, path: &Path) -> PolicyDecision`

- `crates/agentzero-infra/src/tools/mod.rs`:
  - Before executing a tool, call `policy.check_egress()` / `check_command()` / `check_filesystem()` as appropriate
  - On `PolicyDecision::Prompt`, use existing approval flow (channel-based ask, or block with message)
  - On `PolicyDecision::Deny`, return error with the tool name and what was blocked

- `crates/agentzero-config/Cargo.toml`:
  - Add `serde_yaml` dependency

- `crates/agentzero-tools/src/http_request.rs`:
  - Before making HTTP request, call `ctx.policy.check_egress("http_request", &url.host())`

- `crates/agentzero-tools/src/web_fetch.rs`:
  - Same egress check pattern

- `crates/agentzero-tools/src/shell/mod.rs`:
  - Before executing command, call `ctx.policy.check_command(&command_name)`

**Tests (8+):**
- `yaml_policy_loads_from_workspace` — Valid YAML parsed into `SecurityPolicyFile`
- `yaml_policy_missing_is_none` — No file means no overlay
- `default_deny_blocks_unlisted_tool` — Tool not in rules is denied
- `default_allow_permits_unlisted_tool` — When default is `allow`, unlisted tools pass
- `egress_domain_match` — Listed domain is allowed
- `egress_glob_match` — `*.github.com` matches `api.github.com`
- `egress_prompt_returns_prompt_decision` — `prompt` egress returns `Prompt` decision
- `command_allowlist` — Only listed commands pass
- `filesystem_check` — Paths inside allowed dirs pass, outside denied
- `yaml_overrides_toml` — When YAML present, it takes precedence for per-tool checks

**Effort:** Medium-Large. High impact for enterprise positioning.

---

## Sprint 59: Container Sandbox Mode (L)

Optional Docker-based sandbox that enforces the YAML security policy at the OS/network level in addition to the application layer. This is the NVIDIA OpenShell equivalent but without the K3s complexity — a single Docker container with iptables rules derived from the YAML policy.

**Depends on:** Sprint 58 Track B (YAML security policy must exist first).

### Phase A: Sandbox Dockerfile & Entrypoint

**New files:**

- `docker/sandbox/Dockerfile` (~60 lines):
  - Multi-stage: builder (cargo-chef) + runtime (Debian slim)
  - Install `iptables`, `ca-certificates`
  - Create `/workspace` (read-write) and `/sandbox` (read-write)
  - Mount workspace as read-only by default, `/sandbox` and `/tmp` writable
  - Entrypoint: `sandbox-entrypoint.sh`

- `docker/sandbox/sandbox-entrypoint.sh` (~80 lines):
  - Read `security-policy.yaml` from `/workspace/.agentzero/`
  - Parse egress rules and generate iptables rules:
    - Default: `iptables -P OUTPUT DROP`
    - For each `egress` domain: resolve to IP, `iptables -A OUTPUT -d <ip> -j ACCEPT`
    - Always allow: loopback, DNS (port 53), established connections
  - Apply filesystem restrictions via `chmod` / bind mount options
  - Execute `agentzero gateway` as non-root user

- `docker/sandbox/policy-to-iptables.py` (~100 lines):
  - Python script that reads `security-policy.yaml` and generates iptables rules
  - Handles glob domains by resolving wildcard DNS
  - Outputs shell commands for the entrypoint

### Phase B: CLI Command

**Modified files:**

- `crates/agentzero-cli/src/commands/sandbox.rs` (~150 lines):
  - `agentzero sandbox` subcommand:
    - `agentzero sandbox start` — builds/pulls sandbox image, mounts current workspace, starts container
    - `agentzero sandbox stop` — stops sandbox container
    - `agentzero sandbox status` — shows running sandbox, applied policy, active iptables rules
    - `agentzero sandbox shell` — exec into running sandbox for debugging
  - Uses `tokio::process::Command` to run `docker` commands
  - Reads `security-policy.yaml` to validate before launching

- `crates/agentzero-cli/src/lib.rs`:
  - Register `sandbox` subcommand

### Phase C: Documentation

- `site/src/content/docs/security/sandbox.mdx`:
  - What sandboxing provides (network + filesystem isolation)
  - How to write `security-policy.yaml`
  - `agentzero sandbox start` quickstart
  - Architecture diagram: YAML → iptables + app-layer enforcement
  - Comparison with NVIDIA OpenShell approach (similar goals, simpler implementation)

**Tests (4+):**
- `sandbox_dockerfile_builds` — Dockerfile builds successfully
- `sandbox_starts_with_policy` — Container starts and applies iptables rules
- `sandbox_blocks_unlisted_egress` — Outbound to unlisted domain is blocked at network level
- `sandbox_allows_listed_egress` — Outbound to listed domain succeeds

**Effort:** Large. Defer until core protocol work (MCP, A2A) and Sprint 58 land.

---

## Verification Plan

1. **Privacy-aware routing:** Configure routes with mixed privacy levels, verify `private` mode prefers local, `local_only` blocks cloud, `off` allows all
2. **YAML security policy:** Create policy file, verify per-tool egress/command/filesystem enforcement, verify `prompt` decision triggers approval flow
3. **Container sandbox:** `agentzero sandbox start` with policy, verify iptables match YAML rules, verify blocked egress returns errors to agent

## Dependency Graph

```
Sprint 58:  Privacy Routing ────────────> done  (no deps)
            YAML Security Policy ───────> done  (no deps, parallel)

Sprint 59:  Container Sandbox ──────────> done  (depends on YAML policy from 58B)
```

Track A and Track B of Sprint 58 are fully independent. Sprint 59 depends on Sprint 58 Track B.

## What We Explicitly Don't Do (and Why)

- **No K3s orchestrator** — NVIDIA uses K3s because they have infrastructure DNA. Our users want a single binary. Docker container is sufficient for sandbox isolation.
- **No TUI approval screen** — Our existing `require_first_visit_approval` + channel-based approval (Telegram/Discord/CLI) is more flexible than a dedicated TUI.
- **No GPU detection / hardware-aware routing** — We're already provider-agnostic. Adding GPU detection adds complexity with little benefit for our user base.
- **No blueprint/subprocess architecture** — NVIDIA's TypeScript plugin + Python blueprint is a two-language indirection. Our Rust-native approach is simpler and faster.
