# Plan 48: Capability Enforcement Wire-Through

## Context

Sprint 86 (Plan 47) shipped `Capability` + `CapabilitySet` into `agentzero_core::security`,
wired `CapabilitySet` onto `ToolSecurityPolicy`, and added the backward-compatible
`[[capabilities]]` config array. The security foundation is in place, but three wiring
gaps remain — identified explicitly in the Sprint 86 Phase C audit:

1. **Dynamic tools** — `DynamicToolDef` has no `creator_capability_set` field. Tools
   created or evolved by agents inherit the full server-wide `ToolSecurityPolicy`
   regardless of the creator's actual permissions.

2. **Delegate sub-agents** — `DelegateConfig` (the runtime representation of a
   `[[agents]]` entry) carries no `CapabilitySet`. `build_delegate_agents()` never
   intersects the parent's capability set with the per-agent `[[capabilities]]` list.
   A sub-agent configured with narrower permissions silently receives the full root set.

3. **Swarm nodes** — `RunAgentRequest` has no capability override field. Nodes in a
   `PlannedWorkflow` are each dispatched via `build_runtime_execution`, which rebuilds
   the capability set from config every time, making it impossible for the swarm
   orchestrator to inject the root ∩ node intersection.

Two housekeeping items are bundled:

4. **Gossip relay test** — `two_node_gossip_relay` binds to hard-coded ports 19871/19872,
   causing intermittent `EADDRINUSE` failures when the port is still in TIME_WAIT from a
   previous run. Pre-existing flakiness since Sprint 40.

5. **`TursoAutopilotStore`** — deferred from Sprint 85 Phase B. Provides optional cloud
   sync for autopilot data behind the `memory-turso` feature flag.

---

## Decisions

### Phase ordering

Phase A (dynamic tools) is self-contained within `agentzero-infra` and does not touch the
`DelegateConfig` or `RunAgentRequest` types. Phase B and C both touch `runtime.rs` but in
disjoint locations (`build_delegate_agents` vs. `build_runtime_execution` + the swarm
dispatch path). All three phases can be reviewed and merged independently.

### `Option<CapabilitySet>` on `DynamicToolDef`, not `CapabilitySet`

Using `Option<CapabilitySet>` keeps backward compatibility with existing
`dynamic-tools.json` records that predate this sprint. `None` means "no creator
capability constraint recorded" — the existing `ToolSecurityPolicy` checks still apply.
`Some(set)` means the tool was registered by an agent that opted into `[[capabilities]]`.

The alternative (always storing a `CapabilitySet`, defaulting to `CapabilitySet::default()`)
is equivalent at runtime (empty set → `is_empty()` → fallback) but wastes space in
serialised records for every tool created before this sprint.

### `CapabilitySet` (not `Option`) on `DelegateConfig`

`DelegateConfig` is a runtime-only struct — it is never persisted to disk. There is no
backward-compat cost to adding a field. `CapabilitySet::default()` (is_empty → boolean
fallback) is the correct zero value.

### `capability_set_override` on `RunAgentRequest` is `#[serde(skip)]`

`RunAgentRequest` is constructed programmatically and never serialised directly.
The `skip` attribute prevents any accidental serialisation and makes it clear this
field is an injection point, not a config value.

### Gossip test: bind-to-zero throughout

The correct fix is to bind both nodes to `127.0.0.1:0`, let the OS allocate ephemeral
ports, and query the actual bound address from each `GossipEventBus` via a new
`local_addr()` method. The drop-and-restart dance that currently tries to reuse a
just-released port is the root cause of the flakiness.

### `TursoAutopilotStore` scope

Implement behind `#[cfg(feature = "memory-turso")]`. Share the same 5-table SQL schema
as `SqliteAutopilotStore`. No new config keys — reuse the existing `memory.turso_url` /
`memory.turso_auth_token` config that was already present for the memory store.

---

## Phase A: `DynamicToolDef` Capability Bounding (HIGH) ✅ COMPLETE

**Actual effort:** 1 day  
**Status:** Shipped. 23/23 dynamic-tool tests pass. All Phase A acceptance criteria met.

### A1: Add `creator_capability_set` to `DynamicToolDef` ✅

**File:** `crates/agentzero-infra/src/tools/dynamic_tool.rs`

Field added immediately after `user_rated`:

```rust
/// Capability set of the agent that created this tool (Sprint 87).
///
/// `None` means the tool was created before capability enforcement was wired
/// in, or by an agent not using `[[capabilities]]`. The server-wide
/// `ToolSecurityPolicy` booleans apply as before.
///
/// When `Some(set)`, any agent invoking this tool via
/// [`ToolSecurityPolicy::allows_dynamic_tool`] must have a capability set
/// that satisfies the intersection check.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub creator_capability_set: Option<CapabilitySet>,
```

All existing `DynamicToolDef` construction sites updated (`tool_create.rs`,
`pattern_capture.rs`, `tool_evolver.rs`, test helpers) to set `creator_capability_set: None`.
Existing JSON records without the field deserialize to `None` — backward-compatible.

### A2: `register()` signature — kept unchanged ✅

**Decision deviation from plan:** The plan proposed adding a `creator_cap_set` parameter
to `register()`. The simpler approach was adopted instead: callers set
`def.creator_capability_set` directly on the struct before calling `register()`, which
keeps the `register()` signature stable and avoids a redundant second source of truth.

All production callers (`tool_create.rs` × 3, `pattern_capture.rs` × 1) set
`creator_capability_set: None` (tools created before an agent opts into `[[capabilities]]`
are unconstrained, which is the correct backward-compatible default).

### A3: `ToolEvolver` — propagate capability set through evolution ✅

**File:** `crates/agentzero-infra/src/tool_evolver.rs`

Both `fix()` and `improve()` include `creator_capability_set: def.creator_capability_set.clone()`
in the returned `DynamicToolDef` literal. The struct doc comment updated to note Sprint 87
wiring is complete for the storage/propagation layer.

### A4: `ToolSecurityPolicy::allows_dynamic_tool()` ✅

**File:** `crates/agentzero-tools/src/lib.rs`

Added as a new method (does not replace `allows_tool()`). Dependency-cycle-safe signature
takes `creator_cap_set: Option<&CapabilitySet>` rather than the whole `DynamicToolDef`:

```rust
pub fn allows_dynamic_tool(
    &self,
    tool_name: &str,
    creator_cap_set: Option<&agentzero_core::security::CapabilitySet>,
    caller_caps: &agentzero_core::security::CapabilitySet,
) -> bool {
    // Gate 1: static policy (capability set or boolean fallback).
    if !self.allows_tool(tool_name) { return false; }
    // Gate 2: creator capability constraint.
    if let Some(required) = creator_cap_set {
        if !required.is_empty() {
            return caller_caps.intersect(required).allows_tool(tool_name);
        }
    }
    true
}
```

### A5: Unit tests ✅

Added to `dynamic_tool.rs` test module:

- `register_preserves_creator_cap_set` — registers a tool with a `web_search`-only
  `CapabilitySet`; verifies `get_def()` returns it intact.
- `allows_dynamic_tool_denies_mismatched_caller` — caller with only `Shell` capability
  cannot invoke a tool whose creator required `Tool { name: "web_search" }`.

Also fixed a pre-existing race in `test_data_dir()`: added an `AtomicU64` counter to the
directory name so parallel tests can never collide on the same nanosecond timestamp.

---

## Phase B: `DelegateConfig` Capability Field + Intersection (HIGH)

**Estimated effort:** 1 day

### B1: Add `capability_set` to `DelegateConfig`

**File:** `crates/agentzero-core/src/delegation.rs`

Add after `instruction_method`:

```rust
/// Effective capability set for this delegate agent (Sprint 87).
///
/// Computed as the intersection of the parent's `CapabilitySet` and the
/// per-agent `[[capabilities]]` list from config. When `is_empty()` (the
/// default), the sub-agent falls back to the parent's boolean flags.
#[serde(default)]
pub capability_set: agentzero_core::security::CapabilitySet,
```

Update `impl Default for DelegateConfig` to set `capability_set: CapabilitySet::default()`.

### B2: `build_delegate_agents()` — add `root_cap_set` parameter

**File:** `crates/agentzero-infra/src/runtime.rs`

Change the signature:

```rust
fn build_delegate_agents(
    config: &agentzero_config::AgentZeroConfig,
    root_cap_set: &agentzero_core::security::CapabilitySet,
) -> Option<HashMap<String, DelegateConfig>>
```

Inside the `.map()` closure, after building the `DelegateConfig`, add:

```rust
let agent_cap_set = agentzero_config::policy::build_agent_capability_set(
    root_cap_set,
    &agent.capabilities,
);
```

And include it in the `DelegateConfig` struct literal:
```rust
capability_set: agent_cap_set,
```

### B3: Update the call site

In `build_runtime_execution`, the existing call is:

```rust
build_delegate_agents(&config)
```

Change to:

```rust
build_delegate_agents(&config, &policy.capability_set)
```

The `policy` variable (a `ToolSecurityPolicy`) is already in scope at this point.

### B4: Consume `capability_set` in `DelegateTool`

**File:** Find the `DelegateTool` implementation — run:
`hypergrep --layer 1 "DelegateTool" crates/agentzero-tools/src/`

When `DelegateTool::execute()` builds the sub-agent runtime (via
`build_runtime_execution` or equivalent), it has access to `DelegateConfig`.
If `delegate_config.capability_set` is non-empty, pass it through as
`RunAgentRequest::capability_set_override` (the field added in Phase C).

Since Phase B and Phase C touch the same `RunAgentRequest` type, implement Phase C's
`capability_set_override` field first (or in the same commit), then wire it here.

### B5: Unit tests

Add to `crates/agentzero-infra/src/runtime.rs` test module:

```rust
fn build_delegate_agents_intersects_capabilities() {
    // Root cap set allows "web_search" only.
    // Agent config has [[capabilities]] allowing "web_search" + "shell".
    // Expected intersection: "web_search" only.
}

fn build_delegate_agents_no_per_agent_caps_returns_empty() {
    // Agent config has no [[capabilities]].
    // Expected: DelegateConfig::capability_set.is_empty() == true.
}

fn build_delegate_agents_agent_subset_of_root() {
    // Agent config has [[capabilities]] allowing only "git_operations".
    // Root cap set allows everything.
    // Expected: DelegateConfig::capability_set allows only "git_operations".
}
```

---

## Phase C: Swarm Node Capability Propagation (HIGH)

**Estimated effort:** 1.5 days

### C1: Add `capability_set_override` to `RunAgentRequest`

**File:** `crates/agentzero-infra/src/runtime.rs`

Add after `memory_window_override`:

```rust
/// Capability set injected by the swarm orchestrator or delegation layer.
///
/// When non-empty, overrides the `CapabilitySet` that would normally be
/// built from `config.capabilities` in `build_runtime_execution`. This is
/// the mechanism by which swarm nodes and delegate agents receive the
/// root ∩ node intersection instead of the full server-wide set.
///
/// Default: `CapabilitySet::default()` (is_empty → use config as normal).
#[serde(skip)]
pub capability_set_override: agentzero_core::security::CapabilitySet,
```

Update `RunAgentRequest`'s construction sites to add the field (existing callers
set `capability_set_override: CapabilitySet::default()`).

### C2: Honour override in `build_runtime_execution`

After `load_tool_security_policy` returns `policy`, add:

```rust
// If the caller injected a capability override (e.g. from a swarm
// orchestrator), apply it on top of whatever the config built.
if !req.capability_set_override.is_empty() {
    policy.capability_set = req.capability_set_override.clone();
}
```

This single insertion makes the entire downstream pipeline (tools, delegation,
audit) honour the injected set without further changes.

### C3: Locate the swarm dispatch path

Run: `hypergrep --callers "build_runtime_execution" .`

Expected callers: `run_agent`, `build_swarm_with_presence`, `run_agent_once`,
`register_dynamic_agent_from_record`, `run_streaming`, `v1_chat_completions_stream`,
`handle_text_message`.

The swarm entry point is `build_swarm_with_presence` (or the function that creates
individual node `RunAgentRequest`s from the `PlannedWorkflow`). Identify the loop
that constructs one `RunAgentRequest` per swarm node.

### C4: Inject capability intersection into swarm node requests

In the swarm node construction loop:

```rust
// Resolve per-node capability set: root ∩ node config capabilities.
// When the node has no explicit capabilities, `build_agent_capability_set`
// returns CapabilitySet::default() (is_empty) which passes through to
// use the config-built set — safe and correct.
let node_config = config.agents.get(&node.agent_name);
let node_cap_set = node_config
    .map(|c| agentzero_config::policy::build_agent_capability_set(
        &root_cap_set, &c.capabilities,
    ))
    .unwrap_or_default();

node_req.capability_set_override = node_cap_set;
```

Where `root_cap_set` is the root `CapabilitySet` already built in the enclosing
function from `load_tool_security_policy`.

### C5: Unit tests

Add to `crates/agentzero-infra/src/runtime.rs` test module:

```rust
fn capability_set_override_replaces_config_built_set() {
    // Build a RunAgentRequest with capability_set_override = {web_search only}.
    // Verify the resulting RuntimeExecution::policy.capability_set
    // allows only "web_search" regardless of what config says.
}

fn capability_set_override_empty_does_not_replace() {
    // Build a RunAgentRequest with capability_set_override = default (empty).
    // Verify the resulting policy matches what config builds.
}
```

Property test (add to `capability.rs` or a new `runtime_props.rs`):

```rust
proptest! {
    fn swarm_node_never_exceeds_root(root: CapabilitySet, node: CapabilitySet) {
        let intersection = root.intersect(&node);
        // No tool allowed by intersection can be disallowed by root.
        for name in SAMPLE_TOOL_NAMES {
            if intersection.allows_tool(name) {
                prop_assert!(root.allows_tool(name));
            }
        }
    }
}
```

---

## Phase D: Gossip Relay Test Stabilisation (LOW)

**Estimated effort:** 0.5 days

### D1: Expose `local_addr()` from `GossipEventBus`

**File:** `crates/agentzero-orchestrator/src/gossip.rs`

The `GossipEventBus::start()` function binds a `TcpListener` and spawns the
accept loop. Before handing the listener to the accept task, store the bound
`SocketAddr`:

```rust
pub struct GossipEventBus {
    // ... existing fields ...
    local_addr: SocketAddr,  // actual bound address
}

impl GossipEventBus {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}
```

In `start()`, after `listener.local_addr()?`, store it in the struct.

### D2: Rewrite `two_node_gossip_relay`

Replace the drop-and-restart dance:

```rust
#[tokio::test]
async fn two_node_gossip_relay() {
    let bus1 = GossipEventBus::start(GossipConfig {
        listen_addr: "127.0.0.1:0".parse().unwrap(),
        peers: vec![],
        db_path: temp_db_path(),
        capacity: 64,
    }).await.unwrap();

    let addr1 = bus1.local_addr();

    let bus2 = GossipEventBus::start(GossipConfig {
        listen_addr: "127.0.0.1:0".parse().unwrap(),
        peers: vec![addr1],
        db_path: temp_db_path(),
        capacity: 64,
    }).await.unwrap();

    // ... rest of relay assertions using bus1/bus2 directly ...
}
```

### D3: Increase relay timeout to 2 s

Wherever `tokio::time::timeout` gates the relay assertion, change the value from
its current setting to `Duration::from_secs(2)` to absorb scheduling jitter on
slow CI runners.

### D4: Verification

Run `cargo test -p agentzero-orchestrator --lib -- gossip` three times in
succession. All runs must pass.

---

## Phase E: `TursoAutopilotStore` Optional Backend (LOW)

**Estimated effort:** 1 day

### E1: New file `crates/agentzero-autopilot/src/turso_store.rs`

Implement `AutopilotStore` trait behind `#[cfg(feature = "memory-turso")]`.

The SQL schema is identical to `SqliteAutopilotStore` (5 tables: `proposals`,
`missions`, `events`, `cap_gate_ledger`, `content`). Use `libsql::Connection`
instead of `rusqlite::Connection`.

Key implementation notes:
- `libsql` uses an async connection; match the trait's `async fn` signatures
- WAL mode is set by the Turso server; no `PRAGMA journal_mode=WAL` needed
- Re-use the migration SQL already defined in `SqliteAutopilotStore::new()`
  (extract it to a `const SCHEMA_SQL: &str` shared by both impls)

### E2: `Cargo.toml` feature gate

**File:** `crates/agentzero-autopilot/Cargo.toml`

```toml
[features]
memory-turso = ["dep:libsql"]

[dependencies]
libsql = { version = "0.6", optional = true }
```

### E3: Config wiring

**File:** `crates/agentzero-config/src/model.rs` and `crates/agentzero-autopilot/src/lib.rs`

When `memory-turso` feature is enabled and `config.memory.turso_url` is non-empty,
construct `TursoAutopilotStore` in the autopilot startup path instead of
`SqliteAutopilotStore`. The selection logic:

```rust
#[cfg(feature = "memory-turso")]
if !config.memory.turso_url.is_empty() {
    return Ok(Box::new(TursoAutopilotStore::new(&config.memory.turso_url,
        config.memory.turso_auth_token.as_deref()).await?));
}
// Fallback — always available
Ok(Box::new(SqliteAutopilotStore::new(data_dir).await?))
```

### E4: Tests

Add to `turso_store.rs` behind `#[cfg(all(feature = "memory-turso", test))]`:

```rust
async fn create_and_read_proposal_roundtrip() { ... }
async fn mission_status_update() { ... }
async fn stale_missions_query() { ... }
```

Use an in-memory libsql URL (`":memory:"`) so tests run without a Turso account.

### E5: CI

Add an optional matrix entry to `ci.yml`:

```yaml
- name: Check memory-turso feature
  run: cargo check -p agentzero-autopilot --features memory-turso
```

This verifies the feature compiles without running the full test suite against a
live Turso endpoint.

---

## Files to Modify

| File | Phase | Change |
|---|---|---|
| `crates/agentzero-infra/src/tools/dynamic_tool.rs` | A1, A2, A5 | `creator_capability_set` field, `register()` signature, tests |
| `crates/agentzero-infra/src/tool_evolver.rs` | A3 | Propagate capability set through `evolve_tool()` |
| `crates/agentzero-tools/src/lib.rs` | A4 | Add `allows_dynamic_tool()` to `ToolSecurityPolicy` |
| `crates/agentzero-core/src/delegation.rs` | B1 | `capability_set` field on `DelegateConfig` |
| `crates/agentzero-infra/src/runtime.rs` | B2, B3, C1, C2, C4, B5, C5 | `build_delegate_agents` param, `capability_set_override`, honour override, swarm wiring, tests |
| `crates/agentzero-orchestrator/src/gossip.rs` | D1, D2, D3 | `local_addr()`, rewrite test, increase timeout |
| `crates/agentzero-autopilot/src/turso_store.rs` | E1, E4 | New file — `TursoAutopilotStore` impl |
| `crates/agentzero-autopilot/Cargo.toml` | E2 | `memory-turso` feature + `libsql` dep |
| `crates/agentzero-autopilot/src/lib.rs` | E3 | Store selection logic |
| `crates/agentzero-config/src/model.rs` | E3 | (no change if `turso_url` field already exists in `MemoryConfig`) |
| `.github/workflows/ci.yml` | E5 | Optional `memory-turso` compile check |

---

## Effort Estimate

| Phase | Effort | Priority |
|---|---|---|
| A — `DynamicToolDef` cap bounding | 1 day | HIGH |
| B — `DelegateConfig` intersection | 1 day | HIGH |
| C — Swarm node propagation | 1.5 days | HIGH |
| D — Gossip test fix | 0.5 days | LOW |
| E — `TursoAutopilotStore` | 1 day | LOW |
| **Total** | **5 days** | |

Phases A, B, C are the priority items and together close all three security gaps
identified in Sprint 86. Phases D and E are independent and can be deferred if
time is tight without affecting the security posture.

---

## Acceptance Criteria

- [ ] `DynamicToolDef::creator_capability_set: Option<CapabilitySet>` field exists; existing JSON records with no such key deserialise to `None` without error
- [ ] `DynamicToolRegistry::register()` accepts and stores a creator capability set
- [ ] `ToolEvolver::maybe_fix()` and `maybe_improve()` produce evolved tools that carry the original's `creator_capability_set` unchanged
- [ ] `ToolSecurityPolicy::allows_dynamic_tool()` denies a caller whose capability set does not satisfy the tool's `creator_capability_set`
- [ ] `DelegateConfig::capability_set: CapabilitySet` field exists; `Default` impl produces `CapabilitySet::default()`
- [ ] `build_delegate_agents()` accepts `root_cap_set` parameter and populates each `DelegateConfig::capability_set` with `root ∩ per_agent`
- [ ] A delegate agent configured with `[[capabilities]]` narrower than the root can only perform actions within that narrower set
- [ ] `RunAgentRequest::capability_set_override: CapabilitySet` field exists; default is `CapabilitySet::default()` (is_empty → no override)
- [ ] `build_runtime_execution()` replaces `policy.capability_set` with `req.capability_set_override` when the override is non-empty
- [ ] Swarm node `RunAgentRequest`s are constructed with `capability_set_override = root ∩ node`; property test confirms node never exceeds root
- [ ] `two_node_gossip_relay` uses ephemeral ports; passes 3× in succession
- [ ] `TursoAutopilotStore` compiles under `--features memory-turso`; 3 round-trip tests pass against in-memory libsql
- [ ] Default build (no `memory-turso`) is unaffected — `cargo build` clean
- [ ] `cargo fmt --all` — 0 violations
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — 0 warnings
- [ ] `cargo test --workspace` — all tests pass (Sprint 86 baseline + new Phase A/B/C/D/E tests)

---

## What This Unlocks

Once Sprint 87 is complete:

- **Dynamic tool creation is fully capability-bounded.** An agent running under
  `[[capabilities]]` can only create tools it could itself invoke; AUTO-FIX and
  AUTO-IMPROVE cannot silently expand scope.

- **Sub-agent delegation is capability-safe.** The operator can configure a
  narrow `[[capabilities]]` list on any `[[agents]]` entry and trust that the
  sub-agent will never exceed it, regardless of the root config.

- **Swarms are capability-safe.** Each node in a `PlannedWorkflow` is bounded by
  the intersection of the root policy and its own agent config — no node can
  exceed the swarm supervisor's own permissions.

- **Sprint 88 can focus on MCP session scoping and A2A `max_capabilities`** —
  the two remaining HIGH residual risks from the Sprint 86 threat model update.
  Both use the same `capability_set_override` mechanism introduced in Phase C.

- **`TursoAutopilotStore` enables cloud-sync autopilot deployments** without
  pulling Turso into the default dependency set.