# Plan 51: Memory Scope Isolation + Delegate `max_capabilities` Ceiling

## Status: COMPLETE (Sprint 90)

## Context

Plans 48-50 (Sprints 87-89) closed all HIGH residual risks and most MEDIUM risks
from the Sprint 86 threat model: dynamic tool capability bounding, swarm/delegate
capability intersection, MCP session scoping, A2A `max_capabilities`, WASM plugin
filtering, and API key capability ceilings. Sprint 90 closes the final two MEDIUM
items:

1. **Memory-scope isolation** -- `Capability::Memory { scope }` was declared in
   the threat model and stored in `CapabilitySet` but was never enforced at
   runtime. Any agent operating under a restricted `[[capabilities]]` block could
   freely call `memory_store`, `memory_recall`, and `memory_list` on any
   namespace, even those belonging to other agents. Phase J adds the
   `allows_memory(namespace)` predicate to `CapabilitySet` and wires it into
   all three memory tools so that a denied read or write returns a descriptive
   error instead of silently succeeding.

2. **`Delegate { max_capabilities }` enforcement** -- `Capability::Delegate {
   max_capabilities }` was modeled as the mechanism by which a parent agent
   constrains what capabilities a sub-agent may receive. In practice
   `run_agentic` in `delegate.rs` never consulted the parent context's
   `Delegate` grants before assembling the child's tool set. Phase K extracts
   those grants via `delegate_ceiling()` and applies them as an additional
   filter in `run_agentic`, then propagates `config.capability_set` through
   `build_child_ctx` so that nested delegations and memory tools in the child
   are equally constrained.

Both phases share a single prerequisite: `ToolContext` must carry the agent's
effective `CapabilitySet`. Phase J adds that field (with `#[serde(skip, default)]`
for backward compat), Phase J3 threads it through `RuntimeExecution`, and
Phases J4/K1 consume it at enforcement points.

---

## Decisions

### 1. `allows_memory(namespace)` is the enforcement predicate

Rather than reusing `allows_tool`, memory access uses a dedicated predicate that
understands the `scope` subfield of `Capability::Memory`. This keeps the
semantics clean:

- `Memory { scope: None }` grants access to every namespace.
- `Memory { scope: Some("agent_a") }` grants access only to the `agent_a`
  namespace.
- A non-empty `CapabilitySet` that contains no `Memory` variant at all is an
  implicit denial -- the agent was given an explicit set of grants and memory
  was not among them.
- An empty `CapabilitySet` (the default) is treated as unrestricted, preserving
  backward compatibility for all existing agents that have no `[[capabilities]]`
  block.

### 2. `ToolContext` carries a `capability_set` field (`#[serde(skip, default)]`)

Enforcing memory access inside `memory_tools.rs` requires that the executing
tool can see the agent's `CapabilitySet`. The cleanest injection point is
`ToolContext`, which is already threaded into every tool's `execute()` call.
`#[serde(skip, default)]` ensures the field is invisible to JSON serialization
(it is never stored), and `Default` (an empty set) preserves unrestricted
behavior for any context constructed without an explicit capability set.

### 3. Memory tools fail-fast with an informative error

When `allows_memory` returns `false`, the tool returns `Err(anyhow::anyhow!(...))` 
immediately -- before any store I/O. The error message names the denied
namespace so operators can diagnose misconfigured agents. No partial write occurs.

### 4. `delegate_ceiling()` flattens all `Delegate` grants

`delegate_ceiling()` iterates `CapabilitySet::capabilities`, collects all
`max_capabilities` vectors from `Capability::Delegate { .. }` variants, dedupes
them, and returns a fresh `CapabilitySet`. When the parent has no `Delegate`
grant at all the returned set is empty, which is the correct no-ceiling
behavior -- `run_agentic` short-circuits the filter in that case, preserving
existing behavior for agents that have not adopted `Capability::Delegate`.

### 5. `build_child_ctx` propagates `config.capability_set`

`build_child_ctx` already sets child budget limits from `DelegateConfig`. Phase K
adds one line: `child_ctx.capability_set = config.capability_set.clone()`. This
means the child context carries the per-agent capability set (computed upstream
as `root ∩ per-agent caps`) into every memory-tool call and into any further
nested `delegate_ceiling()` extraction, establishing a sound recursive ceiling.

---

## Phase J: Memory Scope Isolation

**Estimated effort:** 0.75 days

### J1: `allows_memory` + `delegate_ceiling` on `CapabilitySet`

**File:** `crates/agentzero-core/src/security/capability.rs`

Add two methods to `impl CapabilitySet`:

```agentzero/crates/agentzero-core/src/security/capability.rs#L281-299
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
```

```agentzero/crates/agentzero-core/src/security/capability.rs#L308-325
/// Build a `CapabilitySet` from all `Capability::Delegate { max_capabilities }`
/// grants in this set.
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
```

Unit tests added in the existing `mod tests` block:

```agentzero/crates/agentzero-core/src/security/capability.rs#L1148-1212
#[test]
fn allows_memory_empty_set_permits_all() {
    let s = CapabilitySet::default();
    assert!(s.allows_memory("default"));
    assert!(s.allows_memory("private"));
    assert!(s.allows_memory("any_namespace"));
}

#[test]
fn allows_memory_full_scope_permits_all() {
    let s = CapabilitySet::new(
        vec![Capability::Memory { scope: None }],
        vec![],
    );
    assert!(s.allows_memory("default"));
    assert!(s.allows_memory("private"));
}

#[test]
fn allows_memory_scoped_permits_only_own_namespace() {
    let s = CapabilitySet::new(
        vec![Capability::Memory { scope: Some("agent_a".to_string()) }],
        vec![],
    );
    assert!(s.allows_memory("agent_a"));
    assert!(!s.allows_memory("default"));
    assert!(!s.allows_memory("agent_b"));
}

#[test]
fn allows_memory_no_memory_cap_in_nonempty_set_denies() {
    let s = CapabilitySet::new(
        vec![Capability::Tool { name: "web_search".to_string() }],
        vec![],
    );
    assert!(!s.allows_memory("default"));
}

#[test]
fn delegate_ceiling_empty_when_no_delegate_cap() {
    let s = CapabilitySet::new(
        vec![Capability::Tool { name: "web_search".to_string() }],
        vec![],
    );
    assert!(s.delegate_ceiling().is_empty());
}

#[test]
fn delegate_ceiling_built_from_delegate_caps() {
    let s = CapabilitySet::new(
        vec![Capability::Delegate {
            max_capabilities: vec![
                Capability::Tool { name: "web_search".to_string() },
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
```

### J2: `capability_set` field on `ToolContext`

**File:** `crates/agentzero-core/src/types.rs`

Add a field to the `ToolContext` struct, after `tool_executions`:

```agentzero/crates/agentzero-core/src/types.rs#L681-690
/// Effective capability set for this execution (Sprint 90 -- Phase J/K).
///
/// When non-empty, memory tools enforce namespace access and delegate tools
/// apply `Delegate { max_capabilities }` ceilings.
/// Set by the runtime from `tool_policy.capability_set`.
/// Empty (default) = unrestricted / backward-compatible.
#[serde(skip, default)]
pub capability_set: crate::security::CapabilitySet,
```

`#[serde(skip, default)]` means:
- The field is never serialized or deserialized (it carries no state across I/O).
- Constructing a `ToolContext` via any existing `serde` path (JSON, TOML) gets
  `CapabilitySet::default()`, which is empty and therefore unrestricted.

### J3: Thread through `RuntimeExecution`

**File:** `crates/agentzero-infra/src/runtime.rs`

Add `capability_set` to `RuntimeExecution`:

```agentzero/crates/agentzero-infra/src/runtime.rs#L118-121
/// Effective capability set for tool context (Sprint 90 -- Phase J).
///
/// Derived from `tool_policy.capability_set` in `build_runtime_execution`.
/// Threaded into `ToolContext.capability_set` at execution time.
pub capability_set: agentzero_core::security::CapabilitySet,
```

Populate it at the end of `build_runtime_execution` (alongside `model_name`):

```agentzero/crates/agentzero-infra/src/runtime.rs#L683-684
model_name: config.provider.model.clone(),
capability_set: tool_policy.capability_set.clone(),
```

Assign to `ctx` in both the streaming and non-streaming execution paths:

```agentzero/crates/agentzero-infra/src/runtime.rs#L946-947
ctx.capability_set = execution.capability_set.clone();
```

### J4: Enforce in memory tools

**File:** `crates/agentzero-tools/src/memory_tools.rs`

The same guard is inserted at the top of the `execute` body in each of the three
memory tools (`MemoryStoreTool`, `MemoryRecallTool`, `MemoryListTool`), after the
namespace is resolved and before any store I/O:

```agentzero/crates/agentzero-tools/src/memory_tools.rs#L94-100
// Phase J -- Sprint 90: enforce memory namespace scope from capability set.
if !ctx.capability_set.is_empty() && !ctx.capability_set.allows_memory(&ns) {
    return Err(anyhow::anyhow!(
        "memory access denied: capability set does not grant \
         access to namespace '{ns}'"
    ));
}
```

The guard reads: "if the agent has any explicit capability grants **and** memory
access to this namespace is not among them, fail immediately." An agent with no
`[[capabilities]]` block (`is_empty() == true`) is unaffected.

Integration tests added in `memory_tools.rs`:

```agentzero/crates/agentzero-tools/src/memory_tools.rs#L432-447
#[tokio::test]
async fn memory_store_denied_by_capability_set() {
    use agentzero_core::security::capability::{Capability, CapabilitySet};
    let dir = temp_dir();
    let mut ctx = ToolContext::new(dir.to_string_lossy().to_string());
    ctx.capability_set = CapabilitySet::new(
        vec![Capability::Tool { name: "memory_store".to_string() }],
        vec![],
    );
    // capability_set has no Memory grant -> access denied
    let err = MemoryStoreTool
        .execute(r#"{"key": "x", "value": "v"}"#, &ctx)
        .await
        .expect_err("should be denied");
    assert!(err.to_string().contains("memory access denied"), "{err}");
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn memory_store_empty_capability_set_allows_all() {
    let dir = temp_dir();
    let ctx = ToolContext::new(dir.to_string_lossy().to_string());
    MemoryStoreTool
        .execute(r#"{"key": "k", "value": "v", "namespace": "anything"}"#, &ctx)
        .await
        .expect("empty cap set should allow all namespaces");
    std::fs::remove_dir_all(dir).ok();
}
```

---

## Phase K: Delegate `max_capabilities` Enforcement

**Estimated effort:** 0.75 days

### K1: Apply ceiling in `run_agentic`

**File:** `crates/agentzero-tools/src/delegate.rs`

After the existing Phase B capability-set filter (which applies
`config.capability_set`), add a second filter stage that extracts the
`Delegate { max_capabilities }` ceiling from the **parent** context:

```agentzero/crates/agentzero-tools/src/delegate.rs#L694-706
// Phase K -- Sprint 90: apply Delegate { max_capabilities } ceiling from the
// parent's capability set. If the parent's CapabilitySet contains any
// Capability::Delegate { max_capabilities } grants, those form an additional
// ceiling on the tools the sub-agent may receive.
let effective_tools: Vec<Box<dyn Tool>> = {
    let ceiling = ctx.capability_set.delegate_ceiling();
    if !ceiling.is_empty() {
        effective_tools
            .into_iter()
            .filter(|t| ceiling.allows_tool(t.name()))
            .collect()
    } else {
        effective_tools
    }
};
```

The two-stage filter means a sub-agent's tool set is bounded by the tightest of:
1. `config.capability_set` (root ∩ per-agent caps, Phase B, Plan 48),
2. `ctx.capability_set.delegate_ceiling()` (parent's `Delegate` grant ceiling).

When both are non-empty the intersection is implicit (both filters are applied
sequentially); when either is empty its stage is a no-op.

Test added to `delegate.rs`:

```agentzero/crates/agentzero-tools/src/delegate.rs#L1179-1196
#[test]
fn delegate_ceiling_filters_child_tools() {
    use agentzero_core::security::capability::{Capability, CapabilitySet};

    // Parent has Delegate { max_capabilities: [web_search] } -- shell blocked.
    let parent_cap = CapabilitySet::new(
        vec![Capability::Delegate {
            max_capabilities: vec![
                Capability::Tool { name: "web_search".to_string() },
            ],
        }],
        vec![],
    );
    let ceiling = parent_cap.delegate_ceiling();
    assert!(ceiling.allows_tool("web_search"));
    assert!(!ceiling.allows_tool("shell"));
    assert!(!ceiling.allows_tool("memory_store"));
}
```

### K2: Propagate `capability_set` in `build_child_ctx`

**File:** `crates/agentzero-tools/src/delegate.rs`

At the end of `build_child_ctx`, after budget propagation and the unique
`conversation_id` assignment:

```agentzero/crates/agentzero-tools/src/delegate.rs#L516-520
// Phase K -- Sprint 90: propagate the child's capability set (already computed
// as root ∩ agent_caps in build_delegate_agents) into the child ToolContext.
// This ensures memory tools and nested delegations enforce the child's scope.
child_ctx.capability_set = config.capability_set.clone();
```

Test added:

```agentzero/crates/agentzero-tools/src/delegate.rs#L1199-1216
#[test]
fn build_child_ctx_propagates_capability_set() {
    use agentzero_core::security::capability::{Capability, CapabilitySet};

    let cap_set = CapabilitySet::new(
        vec![Capability::Memory { scope: Some("child_ns".to_string()) }],
        vec![],
    );
    let config = DelegateConfig {
        capability_set: cap_set.clone(),
        ..Default::default()
    };
    let parent_ctx = test_ctx();
    let child_ctx = build_child_ctx(&parent_ctx, &config, "worker");
    assert!(child_ctx.capability_set.allows_memory("child_ns"));
    assert!(!child_ctx.capability_set.allows_memory("other_ns"));
}
```

---

## Files to Modify

| File | Phase | Change |
|---|---|---|
| `crates/agentzero-core/src/security/capability.rs` | J1 | `allows_memory()` + `delegate_ceiling()` methods; 6 unit tests |
| `crates/agentzero-core/src/types.rs` | J2 | `capability_set: CapabilitySet` field on `ToolContext` (`#[serde(skip, default)]`) |
| `crates/agentzero-infra/src/runtime.rs` | J3 | `capability_set` on `RuntimeExecution`; populate in `build_runtime_execution`; assign to `ctx` in streaming + non-streaming paths |
| `crates/agentzero-tools/src/memory_tools.rs` | J4 | `allows_memory` guard in `MemoryStoreTool`, `MemoryRecallTool`, `MemoryListTool`; 3 integration tests |
| `crates/agentzero-tools/src/delegate.rs` | K1 | Phase K filter stage in `run_agentic`; `delegate_ceiling_filters_child_tools` test |
| `crates/agentzero-tools/src/delegate.rs` | K2 | `child_ctx.capability_set` assignment in `build_child_ctx`; `build_child_ctx_propagates_capability_set` test |

---

## Effort Estimate

| Phase | Effort | Priority |
|---|---|---|
| J -- Memory scope isolation | 0.75 days | MEDIUM |
| K -- Delegate `max_capabilities` enforcement | 0.75 days | MEDIUM |
| **Total** | **1.5 days** | |

---

## Acceptance Criteria

- [x] `allows_memory` returns `true` for an empty `CapabilitySet` (backward compat)
- [x] `allows_memory` returns `true` for `Memory { scope: None }` (full access grant)
- [x] `allows_memory` returns `true` for `Memory { scope: Some("x") }` only when
      `namespace == "x"`; returns `false` for any other namespace
- [x] `allows_memory` returns `false` for a non-empty `CapabilitySet` that contains
      no `Memory` variant (explicit grant set, memory not included)
- [x] `ToolContext.capability_set` field exists; `#[serde(skip, default)]`;
      default is `CapabilitySet::default()` (empty, unrestricted)
- [x] `RuntimeExecution.capability_set` populated from `tool_policy.capability_set`
      in `build_runtime_execution`; threaded into `ctx` in both run paths
- [x] `memory_store` / `memory_recall` / `memory_list` deny access with an
      informative error when `capability_set` is non-empty and `allows_memory`
      returns `false`; succeed when `capability_set` is empty
- [x] `delegate_ceiling()` returns an empty `CapabilitySet` when no
      `Capability::Delegate` grant is present
- [x] `delegate_ceiling()` flattens all `max_capabilities` from all `Delegate`
      grants into a single `CapabilitySet` that enforces `allows_tool` correctly
- [x] `run_agentic` applies the `delegate_ceiling()` filter after the Phase B
      `config.capability_set` filter; child receives only the intersection
- [x] `build_child_ctx` sets `child_ctx.capability_set = config.capability_set`
      so memory tools and nested delegation ceilings see the child's scope
- [x] `cargo check --workspace` -- 0 errors
- [x] `cargo test --workspace` -- all tests pass

---

## What This Unlocks

Sprint 90 closes the last two items from the Sprint 86 threat model, achieving
full coverage of all eight attack surfaces identified in that audit:

| Surface | Threat | Closed by |
|---|---|---|
| Dynamic tool creation | Unbounded tool capabilities at spawn time | Plan 48 Phase A |
| Swarm / delegate config | Child agent exceeds parent's capability set | Plan 48 Phases B+C |
| MCP session | Agent calls any MCP tool regardless of `[[capabilities]]` | Plan 49 Phase F |
| A2A request | Remote agent calls exceed local capability policy | Plan 49 Phase G |
| WASM plugin | Plugin loaded unconditionally, bypassing capability set | Plan 50 Phase H |
| API key | HTTP request runs with full tool access despite key ceiling | Plan 50 Phase I |
| Memory namespace | Agent reads/writes any namespace regardless of `Memory { scope }` | Plan 51 Phase J |
| Delegate ceiling | Sub-agent receives tools beyond parent's `Delegate` grant | Plan 51 Phase K |

With all eight surfaces closed, the Sprint 86 threat model is fully resolved.

Sprint 91 can focus on the next phase of the capability migration roadmap:

- **Phase 2: Deprecation warnings for `enable_*` booleans** -- emit a `tracing::warn!`
  at config-load time when an agent uses `enable_git`, `enable_web_search`, or
  any other legacy boolean that has a `Capability`-based equivalent. This nudges
  operators toward the new `[[capabilities]]` syntax without breaking existing
  configs.

- **FileRead / FileWrite enforcement in file tools** -- `Capability::FileRead {
  glob }` and `Capability::FileWrite { glob }` are already stored in
  `CapabilitySet` but the file tools (`read_file`, `write_file`, `patch_file`)
  do not yet call `allows_file_read` / `allows_file_write`. Closing this surface
  will make the capability system complete across all tool categories.
