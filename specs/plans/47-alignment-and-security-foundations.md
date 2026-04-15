# Plan 47: Alignment & Security Foundations

## Context

Sprint 85 completed a strategic refactor — stripping Composio, Canvas, CLI emulators, and Supabase,
replacing them with a leaner, more coherent surface. The capability-based security *design* was also
written (Plan 46). However, five specific gaps remain before the project is genuinely aligned with its
"security-first, lightweight, single-binary" identity:

1. **Sprint 85 has one unclosed checkbox** — the threat model documents (`docs/security/THREAT_MODEL.md`
   and the site mirror) haven't been updated with the five new attack surfaces identified in Plan 46.
   The design doc documents them; the canonical threat model files do not.

2. **Capability-based security has no implementation sprint** — Plan 46's design is complete but zero
   implementation is scheduled. The 33-boolean `ToolSecurityPolicy` still doesn't compose, doesn't
   scale, and doesn't cover MCP/A2A sessions. This is the single largest gap relative to the project's
   stated identity.

3. **README workspace layout is stale** — references `agentzero-ffi` and `agentzero-bench` (neither
   exists in `Cargo.toml`), omits `agentzero-config-ui`, `agentzero-autopilot`, and `agentzero-macros`,
   and claims "16 crates" when the actual workspace has 19 members.

4. **Sprints 72–75 (Swarms + Self-Evolution) have no prerequisite gate** — they introduce dynamic tool
   creation at runtime (Sprint 73B), which is explicitly one of the five capability attack surfaces from
   Plan 46. Without capability-based security in place first, these sprints would ship unchecked
   privilege escalation.

5. **Channel simplification was identified in Plan 45 Phase 5 but never scheduled** — 8+ channel
   integrations create maintenance burden without proportional user value. A tier classification and
   audit task is overdue.

This plan covers all five items as **Sprint 86**.

---

## Decisions

### Swarms & Self-Evolution: Already Shipped — Needs Retroactive Capability Bounding

Sprints 72–75 are **complete**. The autonomous swarm and self-evolving agent features are already
live in the codebase:

- **Sprint 72** — Parallel `JoinSet` execution, `WorktreeSandbox`, `ContainerSandbox`,
  `MicroVmSandbox`, `SwarmSupervisor`, goal decomposition, dead agent recovery.
- **Sprint 73** — `GoalPlanner`, runtime `DynamicToolRegistry` (tool creation mid-session),
  NL agent definitions, `RecipeStore`.
- **Sprint 74** — `ToolEvolver` (AUTO-FIX, AUTO-IMPROVE), execution telemetry, user feedback.
- **Sprint 75** — AUTO-LEARN pattern capture, recipe evolution, two-stage tool selection,
  tool/recipe bundle sharing.

The concern is not about gating future work — it is about confirming that Phase A's `CapabilitySet`
properly secures these already-shipped systems. Specifically:

- `DynamicToolRegistry` creates WASM tools at runtime (Sprint 73B). Currently the only gate is
  the `enable_dynamic_tools` kill-switch (Sprint 84B). Once Phase A is wired in,
  `allows_tool("tool_create")` will be the enforced check.
- `ToolEvolver` (AUTO-FIX/AUTO-IMPROVE) modifies existing tools. Evolved tools must not gain
  broader permissions than the tool they replaced.
- `SwarmSupervisor` spawns sub-agents. Sub-agents must receive the intersection of the swarm's
  capability set and their own config, not the full server policy.

**Action:** Phase C is a targeted security audit of these already-shipped systems to confirm that
Phase A wiring will close the known gaps — or to surface additional work if it won't.

### Sprint 72F (MicroVmSandbox): Deprioritized, Not Removed

Sprint 72 Phase F is already complete — `ContainerSandbox` and `MicroVmSandbox` are implemented
and tested. The Sprint 85 strategic decision was to deprioritize *further investment* in Firecracker
(handled by the `mvm` project), not to remove existing code. The `MicroVmSandbox` implementation
stays in the codebase as maintenance-only; a note is added to `BACKLOG-EXTERNAL.md` that new
Firecracker features belong to `mvm`.
</thinking>

### Channels: Tier-Based Maintenance Model

Rather than removing channels, adopt a tier model that explicitly tracks maintenance commitment:

- **Tier 1** (actively maintained, CI-tested): Telegram, Discord, Slack, Webhook
- **Tier 2** (feature-gated, best-effort): Matrix, Email, IRC, Nostr, WhatsApp/SMS
- **Tier 3** (BACKLOG-EXTERNAL, requires paid/external APIs): any channel with no test coverage
  and no active community usage evidence

---

## Phase 0: Close Sprint 85 (CRITICAL)

Sprint 85 Phase 0 has one unchecked item: updating the canonical threat model documents. The
attack surfaces are documented in Plan 46 but have not been propagated to the live docs.

- [ ] **Update `docs/security/THREAT_MODEL.md`** — Add a new section "Attack Surfaces Added Since
  Sprint 58" covering each of the five surfaces with threat description, current mitigation state,
  and residual risk:
  - **MCP server mode** — stdio/HTTP clients currently receive the full server tool set. No
    per-session capability scoping exists. Mitigation: Plan 46 Phase 1 will add per-session
    `CapabilitySet`. Residual risk until then: HIGH.
  - **A2A protocol** — external agents submit tasks with no capability negotiation. They run with
    whatever tools are enabled globally. Mitigation: Plan 46 includes per-A2A-agent `max_capabilities`
    config. Residual risk until then: HIGH.
  - **Autopilot self-modification** — proposals can create new agents or tools. Created entities
    currently inherit the full server policy, not just the autopilot's own permissions. Mitigation:
    capability intersection on agent creation (Plan 46 `Delegate` capability). Residual risk: MEDIUM.
  - **Memory poisoning** — agents writing to shared memory can influence other agents' decisions.
    No per-agent memory isolation enforced at the policy layer. Mitigation: `Memory { scope }` cap
    (Plan 46). Residual risk: MEDIUM.
  - **Dynamic tool creation** — runtime WASM codegen creates tools that execute with server-wide
    permissions. Kill-switch exists (Sprint 84B) but no capability bounding. Mitigation: capability
    inheritance from creator (Plan 46). Residual risk: HIGH until Sprint 86 Phase A complete.
- [ ] **Mirror update to `site/src/content/docs/security/threat-model.md`** — same five surfaces,
  written for a user-facing audience (briefer, links to config docs rather than internal analysis)
- [ ] **Check off Sprint 85 Phase 0 threat model item** in `SPRINT.md`

---

## Phase A: Capability-Based Security — Phase 1 Implementation (HIGH)

**Reference design:** `specs/plans/46-capability-based-security.md`

This phase implements Plan 46's Phase 1 only — **backward-compatible** alongside existing boolean
flags. When `capabilities` is empty (the default for all current configs), behavior is 100% identical
to today. The new code path only activates when a `[[capabilities]]` array is present in config.

Estimated effort: 3–5 days.

### A1: Core Types — `Capability` + `CapabilitySet`

**File:** `crates/agentzero-core/src/security/capability.rs` (new file)

- [ ] **`Capability` enum** — 7 variants, serde-tagged:
  ```rust
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(tag = "type", rename_all = "snake_case")]
  pub enum Capability {
      FileRead   { glob: String },
      FileWrite  { glob: String },
      Shell      { commands: Vec<String> },
      Network    { domains: Vec<String> },
      Tool       { name: String },          // supports glob: "mcp:*", "cron_*"
      Memory     { scope: Option<String> },
      Delegate   { max_capabilities: Vec<Capability> },
  }
  ```

- [ ] **`CapabilitySet` struct** — `capabilities: Vec<Capability>`, `deny: Vec<Capability>`:
  - `fn new(grants: Vec<Capability>, deny: Vec<Capability>) -> Self`
  - `fn intersect(&self, other: &CapabilitySet) -> CapabilitySet` — result contains only
    capabilities present in both sets (child never exceeds parent)
  - `fn allows_tool(&self, name: &str) -> bool` — glob matching via `glob` crate
  - `fn allows_file_read(&self, path: &std::path::Path) -> bool`
  - `fn allows_file_write(&self, path: &std::path::Path) -> bool`
  - `fn allows_network(&self, domain: &str) -> bool`
  - `fn allows_shell(&self, command: &str) -> bool`
  - `fn is_empty(&self) -> bool` — true when `capabilities` is empty (triggers boolean fallback)
  - Deny always overrides grant — checked before grants in all `allows_*` methods
  - `impl Default for CapabilitySet` — empty grants and deny (is_empty → true)

- [ ] **Register in `agentzero_core::security` module** — `pub mod capability;` + re-export
  `Capability`, `CapabilitySet` from `agentzero_core::security`

### A2: Config Model Integration

**File:** `crates/agentzero-config/src/model.rs`

- [ ] **Top-level `capabilities`** — add `#[serde(default)] pub capabilities: Vec<Capability>` to
  `AgentZeroConfig`. Uses `[[capabilities]]` TOML syntax. Default: empty vec (backward compat).
- [ ] **Per-agent `capabilities`** — add `#[serde(default)] pub capabilities: Vec<Capability>` to
  `AgentConfig`. Allows per-agent capability override. Default: empty vec (inherits from root).
- [ ] **TOML example** — verify the Plan 46 TOML examples round-trip correctly through `serde`:
  ```toml
  [[capabilities]]
  type = "tool"
  name = "web_search"

  [[capabilities]]
  type = "file_write"
  glob = "src/**/*.rs"

  [[capabilities]]
  type = "shell"
  commands = ["ls", "git", "cargo"]
  ```

### A3: Policy Builder — Backward-Compatible Fallback

**File:** `crates/agentzero-config/src/policy.rs`

- [ ] **`build_capability_set(config: &AgentZeroConfig) -> CapabilitySet`** — when
  `config.capabilities` is non-empty, build a `CapabilitySet` from it; otherwise return
  `CapabilitySet::default()` (is_empty → true → callers fall back to booleans)
- [ ] **`CapabilitySet::from_policy_booleans(policy: &ToolSecurityPolicy) -> CapabilitySet`** —
  implements the 21-entry boolean-to-capability mapping from Plan 46:

  | Boolean Flag               | Equivalent Capability                                      |
  |----------------------------|------------------------------------------------------------|
  | `enable_git`               | `Tool { name: "git_operations" }`                          |
  | `enable_cron`              | `Tool { name: "cron_*" }`                                  |
  | `enable_web_search`        | `Tool { name: "web_search" }`                              |
  | `enable_browser`           | `Tool { name: "browser" }`                                 |
  | `enable_browser_open`      | `Tool { name: "browser_open" }`                            |
  | `enable_http_request`      | `Tool { name: "http_request" }`                            |
  | `enable_web_fetch`         | `Tool { name: "web_fetch" }`                               |
  | `enable_url_validation`    | `Tool { name: "url_validation" }`                          |
  | `enable_agents_ipc`        | `Tool { name: "agents_ipc" }`                              |
  | `enable_html_extract`      | `Tool { name: "html_extract" }`                            |
  | `enable_pushover`          | `Tool { name: "pushover" }`                                |
  | `enable_code_interpreter`  | `Tool { name: "code_interpreter" }`                        |
  | `enable_autopilot`         | `Tool { name: "proposal_*" }` + `Tool { name: "mission_*" }` |
  | `enable_agent_manage`      | `Tool { name: "agent_manage" }`                            |
  | `enable_domain_tools`      | `Tool { name: "domain_*" }`                                |
  | `enable_self_config`       | `Tool { name: "config_manage" }` + `Tool { name: "skill_manage" }` |
  | `enable_wasm_plugins`      | `Tool { name: "wasm_*" }`                                  |
  | `enable_a2a_tool`          | `Tool { name: "a2a" }`                                     |
  | `enable_dynamic_tools`     | `Tool { name: "tool_create" }`                             |
  | `enable_write_file`        | `FileWrite { glob: "**/*" }`                               |
  | `enable_mcp`               | `Tool { name: "mcp:*" }`                                   |

- [ ] This function is used in unit tests only (Phase A5); callers still use booleans directly via
  the existing `ToolSecurityPolicy` checks — the mapping table is the authoritative reference for
  Phase 2 (deprecation) work

### A4: `ToolSecurityPolicy` Integration

- [ ] **Locate `ToolSecurityPolicy`** — run `hypergrep --layer 1 "ToolSecurityPolicy" .` to confirm
  the owning file (expected: `crates/agentzero-tools/src/policy.rs` or `crates/agentzero-core/`)
- [ ] **Add `capability_set: CapabilitySet` field** — with `#[serde(default)]` and
  `impl Default` returning `CapabilitySet::default()` (is_empty). Existing deserialization
  of `ToolSecurityPolicy` from TOML is unaffected.
- [ ] **Update `ToolSecurityPolicy::allows_tool(name: &str) -> bool`** (or equivalent check sites)
  — if `self.capability_set.is_empty()`, use existing boolean logic; otherwise use
  `self.capability_set.allows_tool(name)`. This is the **only** behavioral change for tools.
- [ ] Verify with `cargo test --workspace` that zero existing tests regress

### A5: Property Tests

**Files:** `crates/agentzero-core/src/security/capability.rs` (inline `#[cfg(test)]`) and/or
`crates/agentzero-core/tests/capability_props.rs`

- [ ] **Intersection subset invariant** (`proptest`):
  ```rust
  proptest! {
      fn intersection_is_always_subset_of_both(a: CapabilitySet, b: CapabilitySet) {
          let c = a.intersect(&b);
          // Every tool allowed by c must be allowed by both a and b
          for name in SAMPLE_TOOL_NAMES {
              if c.allows_tool(name) {
                  prop_assert!(a.allows_tool(name));
                  prop_assert!(b.allows_tool(name));
              }
          }
      }
  }
  ```
- [ ] **Deny overrides grant** (`proptest`):
  ```rust
  proptest! {
      fn deny_always_wins_over_grant(name: String) {
          let cap = Capability::Tool { name: name.clone() };
          let set = CapabilitySet::new(vec![cap.clone()], vec![cap]);
          prop_assert!(!set.allows_tool(&name));
      }
  }
  ```
- [ ] **Empty set — all `allows_*` return false** — unit tests for each of the 5 methods
- [ ] **Boolean fallback fidelity** — 21 unit tests, one per mapping entry: verify that
  `CapabilitySet::from_policy_booleans()` with a single boolean set produces the expected
  `allows_tool()` result

### A6: Deprecation Warning + Documentation

- [ ] **Startup deprecation log** — in `agentzero-config` or `agentzero-infra` startup path, after
  loading config: if any `enable_*` boolean is `true` AND `capabilities` is empty, emit once:
  ```
  WARN [agentzero_config] security.enable_* fields are deprecated; migrate to [[capabilities]] \
       array. See: https://auser.github.io/agentzero/config/capabilities/
  ```
  Use `std::sync::OnceLock<()>` to ensure the warning fires at most once per process.
- [ ] **Config reference update** — `site/src/content/docs/config/reference.md` — add
  `[[capabilities]]` section with the TOML examples from Plan 46 (tool glob, file_write glob,
  shell allowlist, network domains, per-agent override). Link to Plan 46 for migration guide.
- [ ] **Architecture note** — `site/src/content/docs/architecture/index.md` — add a paragraph
  noting that `ToolSecurityPolicy` now carries a `CapabilitySet` alongside legacy boolean flags,
  with a pointer to the config reference for the migration path.

---

## Phase B: README Workspace Layout Fix (MEDIUM)

**File:** `README.md`

The "Workspace layout" table is stale. Current incorrect state:

| Issue | Detail |
|---|---|
| `agentzero-ffi` listed | Not in `Cargo.toml`; FFI work deferred |
| `agentzero-bench` listed | Not in `Cargo.toml`; benchmarks not yet extracted to their own crate |
| `agentzero-config-ui` missing | In workspace — browser-based config editor |
| `agentzero-autopilot` missing | In workspace — self-running company engine |
| `agentzero-macros` missing | In workspace — proc macros (`#[tool_fn]`) |
| `agentzero-testkit` description absent | In workspace but not in README table |
| Crate count "16 crates" wrong | Actual workspace: 19 members |

- [ ] **Remove `agentzero-ffi` row** — note in PR description that FFI is planned but not yet
  in workspace
- [ ] **Remove `agentzero-bench` row** — note that benchmarks currently live in `fuzz/`
- [ ] **Add `agentzero-config-ui`** with description:
  `Browser-based configuration editor; React app served by gateway on /config-ui`
- [ ] **Add `agentzero-autopilot`** with description:
  `Self-running company engine: proposals, missions, cap gates, SQLite store`
- [ ] **Add `agentzero-macros`** with description:
  `Proc macros: #[tool_fn] for zero-boilerplate tool registration and codegen helpers`
- [ ] **Add `agentzero-testkit`** with description (if missing):
  `Shared test harness, mock providers, and integration test utilities`
- [ ] **Fix the heading** — change "Workspace layout (16 crates)" to the correct count; run
  `cargo metadata --no-deps --format-version 1 | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d['workspace_members']))"` to get the exact count
- [ ] **Verify** — after edits, manually cross-check every row against `Cargo.toml` workspace
  members list

---

## Phase C: Self-Evolution & Swarm Security Audit (MEDIUM)

Sprints 72–75 are complete and shipped. This phase audits the security posture of the delivered
systems and documents precisely how Phase A's `CapabilitySet` will close the known gaps. No
feature code is written here — only verification, annotation, and documentation.

### C1: Audit `DynamicToolRegistry` (Sprint 73B)

- [ ] **Locate the `tool_create` tool registration** — `hypergrep --layer 1 "DynamicToolRegistry" .`
  to find the call site where `enable_dynamic_tools` is checked
- [ ] **Verify the gap** — confirm that when `ToolSecurityPolicy::allows_tool("tool_create")` is
  wired in Phase A4, dynamic tool creation is blocked for agents whose `CapabilitySet` does not
  include `Tool { name: "tool_create" }`. If the check site is not in a place Phase A4 reaches,
  add it as an explicit sub-task in Phase A4.
- [ ] **Add code comment** to `DynamicToolRegistry` noting: "Creation is capability-gated via
  `Tool { name: \"tool_create\" }`. The `enable_dynamic_tools` bool (Sprint 84B kill-switch) is
  a coarser fallback; Phase A `CapabilitySet` is the authoritative enforcement once populated."

### C2: Audit `ToolEvolver` (Sprint 74B — AUTO-FIX / AUTO-IMPROVE)

- [ ] **Confirm evolved tools inherit creator's capability scope** — inspect `ToolEvolver::auto_fix()`
  and `auto_improve()` to verify that evolved `DynamicToolDef` records do not expand the tool's
  effective permissions beyond what the original had. Document the finding.
- [ ] **If no capability field exists on `DynamicToolDef`**, add a `capability_set: Option<CapabilitySet>`
  field (default `None` = inherits parent agent's set) as part of Phase A work — flag this as an
  A4 sub-task if discovered
- [ ] **Add code comment** to `ToolEvolver` noting the capability inheritance requirement

### C3: Audit `SwarmSupervisor` Sub-Agent Spawning (Sprint 72)

- [ ] **Locate sub-agent construction in `SwarmSupervisor`** — confirm where each swarm node's
  agent is instantiated and what policy/capability set it receives
- [ ] **Verify intersection semantics** — once Phase A4 is complete, confirm that
  `SwarmSupervisor` passes the intersection of the swarm's `CapabilitySet` and the per-node
  config's capability set when constructing each agent's `ToolSecurityPolicy`. If the
  intersection is not applied, add it as an explicit Phase A4 sub-task.
- [ ] **Add code comment** in `SwarmSupervisor` noting the intended capability inheritance model

### C4: Note MicroVmSandbox Deprioritization (Sprint 72F)

- [ ] **Add a `// NOTE:` comment** to `MicroVmSandbox` in the codebase: "This sandbox backend is
  maintenance-only. New Firecracker/microVM investment belongs to the `mvm` project
  (gomicrovm.com); see BACKLOG-EXTERNAL.md."
- [ ] **Add entry to `BACKLOG-EXTERNAL.md`** under a new "MicroVM Agent Backends" section:
  document that `MicroVmSandbox` exists in AgentZero as a proof-of-concept; production
  Firecracker isolation is handled by the `mvm` project; AgentZero will integrate `mvm` as an
  external dependency when the interface stabilizes

### C5: Summarize Audit Findings

- [x] **Update this plan document** with audit findings from C1–C4:

#### C1 — DynamicToolRegistry (Sprint 73B)

`ToolSecurityPolicy::allows_tool("tool_create")` is the Phase A4 gate for dynamic tool
creation. When `CapabilitySet` is non-empty, agents lacking a matching `Tool { name:
"tool_create" }` capability will have creation blocked with no further changes needed.

The check site is:
`build_runtime_execution` → `load_tool_security_policy` → the `ToolSecurityPolicy` returned
carries the `CapabilitySet` built from config.

A code comment has been added to `DynamicToolRegistry` pointing at this gate.

**Gap**: `DynamicToolDef` does not yet carry a `capability_set` field. Evolved and
dynamically-created tools therefore inherit the full server policy and cannot be bounded
more tightly than their creator. Adding `capability_set: Option<CapabilitySet>` to
`DynamicToolDef` is an explicit Phase A4 follow-up sub-task (not blocking Sprint 86).

#### C2 — ToolEvolver (Sprint 74B — AUTO-FIX / AUTO-IMPROVE)

Evolved tools inherit the same `ToolSecurityPolicy` as the enclosing agent (server-wide
policy). With no `capability_set` field on `DynamicToolDef`, evolved tools cannot be
bounded more tightly than the creator.

**Gap**: Same as C1 — `capability_set: Option<CapabilitySet>` on `DynamicToolDef` is
required before per-tool capability bounding is possible. A code comment has been added
to the `ToolEvolver` implementation pointing at this gap.

#### C3 — SwarmSupervisor Sub-Agent Spawning (Sprint 72)

`SwarmSupervisor::execute()` compiles a `PlannedWorkflow` and dispatches steps via
`StepDispatcher`. Each swarm node is instantiated via `build_runtime_execution()` with
the full server-level `ToolSecurityPolicy`. No capability intersection happens at swarm
node instantiation today.

**Intended model**: root `CapabilitySet` ∩ node `CapabilitySet` (from the node's
`[[agent.capabilities]]` config). This will be enforced once Phase A4 wires
`CapabilitySet` through `build_runtime_execution`. A code comment has been added to
`SwarmSupervisor`.

**Gap**: Until Phase A4 wiring is complete, all swarm nodes run with the full root policy
regardless of per-agent capability lists.

#### C4 — MicroVmSandbox (Sprint 72F)

Deprioritized. Maintenance-only comment added to the `MicroVmSandbox` stub. A
`BACKLOG-EXTERNAL.md` entry has been added under "MicroVM Agent Backends" noting that
this requires an external microVM backend (e.g., Firecracker) and is not on the near-term
roadmap.

#### What Phase A Closes Automatically

Once `CapabilitySet` is non-empty in config:

- `ToolSecurityPolicy::allows_tool("tool_create")` blocks dynamic tool creation for
  agents lacking the capability — **no further code changes needed**.
- MCP tool access is gated via `allows_tool("mcp:*")` pattern matching.
- A2A tool access is gated via `allows_tool("a2a")`.

#### Required Follow-Up Sub-Tasks (not blocking Sprint 86)

1. Add `capability_set: Option<CapabilitySet>` to `DynamicToolDef` (Phase A4 sub-task) —
   enables per-tool and per-evolved-tool capability bounding.
2. Wire capability intersection into `build_runtime_execution` for swarm node
   instantiation — enforces the root ∩ node model described in C3.

#### Accepted Risks

- **Autopilot self-modification**: The `Delegate` capability type is defined in the
  `Capability` enum but is not yet enforced in any dispatch path. Autopilot-driven
  self-modification remains ungated until `Delegate` is wired through the delegation
  machinery.
- **Memory poisoning**: The `Memory` capability type is defined but not yet enforced at
  the `MemoryStore` layer. Memory namespace isolation remains a future sprint item.

Both gaps are accepted until Phase A4 / a dedicated follow-up sprint.

---

## Phase D: Channel Simplification Audit (LOW)

**Reference:** `specs/plans/45-strategic-review-strip-pull.md` Phase 5

This is a documentation and audit task. No channel code is removed this sprint — the goal is
to establish the tier classification so future maintenance decisions are explicit.

### D1: Audit

For each channel integration in `crates/agentzero-channels/src/`, evaluate:
1. Does it have an integration test (even a stub)?
2. Is it covered by CI?
3. Is it feature-gated (e.g., `channels-standard`, `channels-extended`)?
4. Is there evidence of community usage (issues, PRs, docs requests)?

- [ ] **Produce a tier classification table** (to be added to `site/src/content/docs/reference/channels.md`):

  | Channel     | Tier | Tests | CI | Feature Flag         | Notes |
  |-------------|------|-------|----|----------------------|-------|
  | Telegram    | 1    | ✅    | ✅ | `channels-standard`  | Most-used |
  | Discord     | 1    | ✅    | ✅ | `channels-standard`  | |
  | Slack       | 1    | ✅    | ✅ | `channels-standard`  | |
  | Webhook     | 1    | ✅    | ✅ | default              | HTTP egress |
  | Matrix      | 2    | ⚠️   | ⚠️ | `channels-extended`  | |
  | Email       | 2    | ⚠️   | ⚠️ | `channels-extended`  | |
  | IRC         | 2    | ⚠️   | ⚠️ | `channels-extended`  | |
  | Nostr       | 2    | ⚠️   | ⚠️ | `channels-extended`  | Privacy-relevant |
  | WhatsApp    | 2    | ⚠️   | ⚠️ | `channels-extended`  | Requires Meta API |
  | SMS         | 2    | ⚠️   | ⚠️ | `channels-extended`  | Requires Twilio |

  *(Adjust based on actual code audit findings — the table above is the expected starting state.)*

### D2: Ensure Tier 1 Test Coverage

- [ ] For each Tier 1 channel, verify at least one integration test exists (constructor + send path)
- [ ] If a Tier 1 channel has no test, add a stub test that exercises the channel struct creation
  and a mock send — does not require live credentials

### D3: Documentation Update

- [ ] **Update `site/src/content/docs/reference/channels.md`** — add tier table, explain what
  Tier 1 vs Tier 2 means for maintenance commitment and test coverage expectations
- [ ] **Update `BACKLOG-EXTERNAL.md`** if any channels are reclassified to Tier 3 during the audit

---

## Files to Modify

| File | Phase | Change |
|---|---|---|
| `docs/security/THREAT_MODEL.md` | Phase 0 | Add 5 new attack surfaces |
| `site/src/content/docs/security/threat-model.md` | Phase 0 | Mirror threat model update |
| `crates/agentzero-core/src/security/capability.rs` | Phase A1 | New file |
| `crates/agentzero-core/src/security/mod.rs` | Phase A1 | Add `pub mod capability;` |
| `crates/agentzero-config/src/model.rs` | Phase A2 | Add `capabilities` fields |
| `crates/agentzero-config/src/policy.rs` | Phase A3 | Add `build_capability_set()`, mapping fn |
| `crates/agentzero-tools/src/policy.rs` (or equivalent) | Phase A4 | Add `capability_set` field |
| `crates/agentzero-core/tests/capability_props.rs` | Phase A5 | New proptest file |
| `site/src/content/docs/config/reference.md` | Phase A6 | Add `[[capabilities]]` section |
| `site/src/content/docs/architecture/index.md` | Phase A6 | Note capability model |
| `README.md` | Phase B | Fix workspace table |
| `crates/agentzero-infra/src/dynamic_tool.rs` (or equivalent) | Phase C | Add `capability_set` field + code comment |
| `crates/agentzero-infra/src/tool_evolver.rs` (or equivalent) | Phase C | Add capability inheritance comment |
| `crates/agentzero-orchestrator/src/swarm_supervisor.rs` (or equiv) | Phase C | Add capability intersection comment |
| `crates/agentzero-orchestrator/src/sandbox.rs` (MicroVmSandbox) | Phase C | Add maintenance-only comment |
| `specs/BACKLOG-EXTERNAL.md` | Phase C | Add MicroVM/Fleet section |
| `site/src/content/docs/reference/channels.md` | Phase D | Add tier table |

---

## Effort Estimate

| Phase | Effort | Type |
|---|---|---|
| Phase 0: Close Sprint 85 | 2–4 hours | Writing |
| Phase A: Capability Security Phase 1 | 3–5 days | Code + tests |
| Phase B: README Fix | 30 minutes | Mechanical |
| Phase C: Scope Gating | 2–4 hours | Writing + annotation |
| Phase D: Channel Audit | 2–4 hours | Research + docs |
| **Total** | **~4–6 days** | |

---

## Acceptance Criteria

- [ ] `docs/security/THREAT_MODEL.md` updated with 5 attack surfaces (MCP, A2A, autopilot,
  memory poisoning, dynamic tools)
- [ ] Site mirror (`site/src/content/docs/security/threat-model.md`) matches
- [ ] Sprint 85 Phase 0 threat model checkbox marked `[x]`
- [ ] `Capability` enum and `CapabilitySet` struct exist in `agentzero_core::security`
- [ ] `CapabilitySet::intersect()` implemented and covered by property test (1,000+ iterations)
- [ ] Deny-overrides-grant property test passes (1,000+ iterations)
- [ ] All 5 `allows_*` methods return `false` on empty `CapabilitySet`
- [ ] 21 boolean-to-capability mapping unit tests all pass
- [ ] `ToolSecurityPolicy` has `capability_set: CapabilitySet` field
- [ ] Existing configs with no `[[capabilities]]` key behave identically to before (verified by
  existing test suite — zero regressions)
- [ ] Config model accepts `[[capabilities]]` at top-level and per-agent
- [ ] Deprecation warning fires once at startup when `enable_*` booleans used without capability upgrade
- [ ] Config reference doc updated with `[[capabilities]]` examples
- [ ] README workspace table matches `Cargo.toml` exactly (correct crate count, no stale entries,
  no missing entries)
- [ ] Sprint 72, 73, 74, 75 in `SPRINT.md` have prerequisite gate annotations
- [ ] Sprint 72F removed from `SPRINT.md` and added to `BACKLOG-EXTERNAL.md`
- [ ] Channel tier classification table written and published to site docs
- [ ] Tier 1 channels each have at least one integration test
- [ ] `cargo fmt --all` — 0 violations
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — 0 warnings
- [ ] `cargo test --workspace` — all tests pass (3,079+ baseline)

---

## What This Unlocks

Completing Sprint 86 gates the following work that cannot be safely scheduled today:

- **Full capability enforcement for Sprints 72–75's shipped code** — Dynamic tool creation
  (`DynamicToolRegistry`), AUTO-FIX (`ToolEvolver`), and sub-agent spawning (`SwarmSupervisor`)
  all gain proper capability bounding once Phase A4 is wired in. These features are already live;
  Phase A makes them properly secured.
- **MCP per-session scoping** (Plan 46 Phase 1, `crates/agentzero-infra/src/mcp_server.rs`) —
  MCP server mode currently gives every stdio/HTTP client the full tool set. Adding per-session
  `CapabilitySet` requires the types from Phase A1 to exist first.
- **A2A capability negotiation** (`crates/agentzero-gateway/src/a2a.rs`) — A2A task submission
  needs to enforce `max_capabilities` from config. Requires Phase A1 types.
- **Autopilot agent-creation safety** — When the autopilot creates a new agent, its `CapabilitySet`
  should be the intersection of the autopilot's own caps and the new agent's requested caps. This
  requires Phase A4 (ToolSecurityPolicy integration) to be complete.