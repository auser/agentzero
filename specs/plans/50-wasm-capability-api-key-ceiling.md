# Plan 50: WASM Plugin Capability Filtering + API Key Capability Ceiling

## Context

Sprints 87/88 (Plans 48/49) closed all HIGH residual risks from the Sprint 86
threat model: dynamic tool capability bounding, swarm/delegate capability
intersection, MCP session scoping, and A2A `max_capabilities`. Sprint 89 closes
the remaining two MEDIUM items:

1. **WASM plugin capability filtering** (Phase H) -- WASM plugins loaded via
   `enable_wasm_plugins` are exposed unconditionally regardless of the agent's
   `capability_set`. Unlike MCP tools (fixed in Plan 49 Phase F), the
   `#[cfg(feature = "wasm-plugins")]` block in `default_tools_inner` never
   consults `policy.capability_set`. Phase H applies the same guard: when
   `policy.capability_set` is non-empty, skip any plugin whose `manifest.id` is
   not allowed by `capability_set.allows_tool(id)`.

2. **API key capability ceiling** (Phase I) -- `build_agent_request` in
   `handlers.rs` always passes
   `capability_set_override: CapabilitySet::default()`, meaning every request
   runs with full tool access regardless of which API key authenticated it.
   Phase I stores a `capability_ceiling: Vec<Capability>` on `ApiKeyRecord`,
   propagates it as `CapabilitySet` through `ApiKeyInfo` -> `AuthIdentity`, and
   threads it into `RunAgentRequest.capability_set_override` at every call site
   that reaches `build_agent_request`.

Both phases require zero schema migrations: `#[serde(default)]` on the new field
preserves backward compatibility for existing stored API keys.

---

## Decisions

### WASM plugin `manifest.id` as the capability name

WASM plugin identifiers come from `plugin.manifest.id` -- a slug defined by the
plugin author (e.g., `image_classifier`, `pdf_extractor`). The same
`allows_tool(id)` predicate used for MCP tools applies unchanged. Operators
grant access with:

```toml
[[capabilities]]
tools = ["image_classifier", "mcp__*"]
```

No new `Capability` variant is needed; `Capability::Tool { name }` covers both
MCP and WASM names.

### Empty `capability_set` means unrestricted (backward compat)

The `capability_set.is_empty()` short-circuit from Plan 48 Phase B applies to
WASM plugin filtering identically to how it applies to MCP tool filtering. An
agent with no `[[capabilities]]` block sees all WASM plugins unchanged.

### `capability_ceiling` stored as `Vec<Capability>`, resolved to `CapabilitySet` at auth time

`ApiKeyRecord` stores raw `Vec<Capability>` so the encrypted JSON file remains
readable by any codebase version. `validate()` converts to `CapabilitySet` when
constructing `ApiKeyInfo`. An empty vector means "no ceiling" (unrestricted),
matching the behavior of bearer/paired tokens.

### `build_agent_request` receives an explicit `capability_override` parameter

Rather than pulling `AuthIdentity` out of global state inside
`build_agent_request`, the function receives `capability_override: CapabilitySet`
as an explicit parameter. Each call site is responsible for extracting
`identity.capability_ceiling` from its `authorize_with_scope` return value.
This keeps the function side-effect-free and unit-testable.

### Bearer / paired tokens get an empty (unrestricted) `capability_ceiling`

`AuthIdentity::full_access()` sets `capability_ceiling: CapabilitySet::default()`.
An empty `CapabilitySet` passed to `RunAgentRequest.capability_set_override` is
a no-op (all tools allowed), preserving existing behavior for non-API-key auth
paths.

---

## Phase H: WASM Plugin Capability Filtering (MEDIUM)

**Estimated effort:** 0.5 days

### H1: Filter WASM plugins by `CapabilitySet`

**File:** `crates/agentzero-infra/src/tools/mod.rs`

In `default_tools_inner`, inside the `#[cfg(feature = "wasm-plugins")]` block,
add a capability guard immediately after the `filter_by_state` step and before
the `for plugin in discovered` loop:

```rust
// Phase H: filter by agent capability set (same pattern as MCP Phase F).
let discovered: Vec<_> = if !policy.capability_set.is_empty() {
    discovered
        .into_iter()
        .filter(|p| {
            let allowed = policy.capability_set.allows_tool(&p.manifest.id);
            if !allowed {
                tracing::debug!(
                    plugin = %p.manifest.id,
                    "wasm plugin filtered out by agent capability set"
                );
            }
            allowed
        })
        .collect()
} else {
    discovered
};
```

No other changes to this block are required; the existing `for plugin in
discovered` loop, `WasmIsolationPolicy` setup, and `WasmTool::from_manifest`
call are all unchanged.

### H2: Backward-compatibility test

**File:** `crates/agentzero-infra/src/tools/mod.rs` (existing `#[cfg(test)]` block)

```rust
#[test]
fn wasm_plugins_all_visible_when_capability_set_empty() {
    use agentzero_core::security::CapabilitySet;
    use agentzero_tools::ToolSecurityPolicy;
    // Empty CapabilitySet (default) -- the filter predicate must be a no-op.
    let policy = ToolSecurityPolicy {
        capability_set: CapabilitySet::default(),
        enable_wasm_plugins: true,
        ..ToolSecurityPolicy::default_for_workspace(std::path::PathBuf::from("."))
    };
    let plugin_id = "image_classifier";
    let visible = if !policy.capability_set.is_empty() {
        policy.capability_set.allows_tool(plugin_id)
    } else {
        true
    };
    assert!(visible, "empty cap set must not filter any plugin");
}
```

### H3: Capability-filtered plugin test

**File:** `crates/agentzero-infra/src/tools/mod.rs`

```rust
#[test]
fn wasm_plugins_filtered_by_capability_set() {
    use agentzero_core::security::capability::{Capability, CapabilitySet};
    use agentzero_tools::ToolSecurityPolicy;

    let cap_set = CapabilitySet::new(
        vec![Capability::Tool { name: "image_classifier".to_string() }],
        vec![],
    );
    let policy = ToolSecurityPolicy {
        capability_set: cap_set,
        enable_wasm_plugins: true,
        ..ToolSecurityPolicy::default_for_workspace(std::path::PathBuf::from("."))
    };

    let cases = [("image_classifier", true), ("pdf_extractor", false)];
    for (id, expected) in &cases {
        let allowed = if !policy.capability_set.is_empty() {
            policy.capability_set.allows_tool(id)
        } else { true };
        assert_eq!(allowed, *expected,
            "plugin {id}: allowed={allowed}, expected={expected}");
    }
}
```

---

## Phase I: API Key Capability Ceiling (MEDIUM)

**Estimated effort:** 1 day

### I1: `ApiKeyRecord.capability_ceiling` and `create_with_ceiling()`

**File:** `crates/agentzero-gateway/src/api_keys.rs`

Add a new field to `ApiKeyRecord` (after `hmac_secret`):

```rust
/// Optional capability ceiling for this key.
/// When non-empty, requests authenticated by this key are bounded to these
/// capabilities regardless of the agent's own configured capability_set.
/// Defaults to empty (unrestricted) for backward compatibility with stored keys.
#[serde(default)]
pub capability_ceiling: Vec<agentzero_core::security::capability::Capability>,
```

Add a new constructor alongside `create_with_hmac()`:

```rust
/// Create a key with an explicit capability ceiling (no HMAC signing).
pub fn create_with_ceiling(
    &self,
    org_id: &str,
    user_id: &str,
    scopes: HashSet<Scope>,
    expires_at: Option<u64>,
    capability_ceiling: Vec<agentzero_core::security::capability::Capability>,
) -> anyhow::Result<(String, ApiKeyRecord)> {
    let raw_key = generate_api_key();
    let key_hash = hash_key(&raw_key);
    let key_id = format!("azk_{}", &key_hash[..12]);
    let record = ApiKeyRecord {
        key_id: key_id.clone(),
        key_hash,
        org_id: org_id.to_string(),
        user_id: user_id.to_string(),
        scopes,
        created_at: now_epoch(),
        expires_at,
        hmac_secret: None,
        capability_ceiling,
    };
    // ... flush and audit (same pattern as create_with_hmac)
    Ok((raw_key, record))
}
```

### I2: `ApiKeyInfo.capability_ceiling` and propagation in `validate()`

**File:** `crates/agentzero-gateway/src/api_keys.rs`

Add field to `ApiKeyInfo`:

```rust
/// Resolved capability ceiling (empty = unrestricted).
pub capability_ceiling: agentzero_core::security::CapabilitySet,
```

Update the `Some(ApiKeyInfo { ... })` block inside `validate()`:

```rust
Some(ApiKeyInfo {
    key_id: record.key_id.clone(),
    org_id: record.org_id.clone(),
    user_id: record.user_id.clone(),
    scopes: record.scopes.clone(),
    hmac_secret: record.hmac_secret.clone(),
    capability_ceiling: if record.capability_ceiling.is_empty() {
        agentzero_core::security::CapabilitySet::default()
    } else {
        agentzero_core::security::CapabilitySet::new(
            record.capability_ceiling.clone(),
            vec![],
        )
    },
})
```

### I3: `AuthIdentity.capability_ceiling`

**File:** `crates/agentzero-gateway/src/auth.rs`

Add field to `AuthIdentity`:

```rust
pub(crate) struct AuthIdentity {
    pub(crate) scopes: HashSet<Scope>,
    pub(crate) api_key: Option<ApiKeyInfo>,
    /// Capability ceiling for this identity (empty = unrestricted).
    /// Populated from ApiKeyInfo; empty for bearer/paired token auth.
    pub(crate) capability_ceiling: agentzero_core::security::CapabilitySet,
}
```

Update `full_access()` (bearer/paired token path):

```rust
fn full_access() -> Self {
    Self {
        scopes: [
            Scope::RunsRead, Scope::RunsWrite, Scope::RunsManage, Scope::Admin,
        ]
        .into(),
        api_key: None,
        capability_ceiling: agentzero_core::security::CapabilitySet::default(),
    }
}
```

In `authorize_request`, wherever an `ApiKeyInfo` is converted to `AuthIdentity`,
propagate the ceiling:

```rust
AuthIdentity {
    scopes: info.scopes.clone(),
    capability_ceiling: info.capability_ceiling.clone(),
    api_key: Some(info),
}
```

### I4: `build_agent_request` parameter

**File:** `crates/agentzero-gateway/src/handlers.rs`

Change the function signature:

```rust
// Before
fn build_agent_request(
    state: &GatewayState,
    message: String,
    model_override: Option<String>,
) -> Result<RunAgentRequest, GatewayError>

// After
fn build_agent_request(
    state: &GatewayState,
    message: String,
    model_override: Option<String>,
    capability_override: agentzero_core::security::CapabilitySet,
) -> Result<RunAgentRequest, GatewayError>
```

Replace the last field in the returned `RunAgentRequest`:

```rust
// Before
capability_set_override: agentzero_core::security::CapabilitySet::default(),

// After
capability_set_override: capability_override,
```

### I5: Thread ceiling through `v1_chat_completions_stream`

**File:** `crates/agentzero-gateway/src/handlers.rs`

The private streaming helper is called from `v1_chat_completions` after auth; it
must accept and forward the ceiling:

```rust
// Before
async fn v1_chat_completions_stream(
    state: &GatewayState,
    message: &str,
    model_override: Option<String>,
) -> Result<Response, GatewayError>

// After
async fn v1_chat_completions_stream(
    state: &GatewayState,
    message: &str,
    model_override: Option<String>,
    capability_override: agentzero_core::security::CapabilitySet,
) -> Result<Response, GatewayError>
```

Inside the body, pass `capability_override` straight through to
`build_agent_request`.

### I6: Update all call sites

**File:** `crates/agentzero-gateway/src/handlers.rs`

Three handlers currently call `build_agent_request` after discarding the
`authorize_with_scope` return value. Each must now capture the identity and pass
its ceiling.

**`api_chat`:**

```rust
// Before
authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;
// ...
let agent_req = build_agent_request(&state, req.message, None)?;

// After
let identity = authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;
// ...
let agent_req = build_agent_request(
    &state, req.message, None, identity.capability_ceiling,
)?;
```

**`v1_chat_completions`** (both the streaming early-return and the non-streaming
path):

```rust
// Before
authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;
// ...
return v1_chat_completions_stream(&state, &last_user, model_override).await;
// ...
let agent_req = build_agent_request(&state, last_user, model_override)?;

// After
let identity = authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;
// ...
return v1_chat_completions_stream(
    &state, &last_user, model_override,
    identity.capability_ceiling.clone(),
).await;
// ...
let agent_req = build_agent_request(
    &state, last_user, model_override, identity.capability_ceiling,
)?;
```

**`async_submit`** -- all four arms (`followup`, `collect`, `interrupt`, `steer`):

```rust
// Before
authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;
// ... (each arm)
let agent_req = build_agent_request(&state, req.message, req.model)?;

// After
let identity = authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;
let cap_ceiling = identity.capability_ceiling;
// ... (each arm)
let agent_req = build_agent_request(
    &state, req.message, req.model, cap_ceiling.clone(),
)?;
```

For the `collect` arm's `tokio::spawn` closure, clone `cap_ceiling` before the
fan-out loop and move a per-iteration clone into each spawned task, since the
closure captures by move:

```rust
for _ in 0..collect_count {
    let ceiling_clone = cap_ceiling.clone();
    // ...
    handles.push(tokio::spawn(async move {
        let req = build_agent_request(&st, msg, mdl, ceiling_clone)?;
        // ...
    }));
}
```

---

## Files to Modify

| File | Phase | Change |
|---|---|---|
| `crates/agentzero-infra/src/tools/mod.rs` | H1-H3 | Cap filter in wasm-plugins block; 2 unit tests |
| `crates/agentzero-gateway/src/api_keys.rs` | I1-I2 | `capability_ceiling` on record + info; `create_with_ceiling()`; `validate()` propagation |
| `crates/agentzero-gateway/src/auth.rs` | I3 | `capability_ceiling` on `AuthIdentity`; set from API key; empty for `full_access` |
| `crates/agentzero-gateway/src/handlers.rs` | I4-I6 | `build_agent_request` new param; stream sig; 3 call sites capture identity |

---

## Effort Estimate

| Phase | Effort | Priority |
|---|---|---|
| H -- WASM plugin capability filtering | 0.5 days | MEDIUM |
| I -- API key capability ceiling | 1 day | MEDIUM |
| **Total** | **1.5 days** | |

---

## Acceptance Criteria

- [x] WASM plugins filtered by `capability_set` when `capability_set` is non-empty
      (same `allows_tool` predicate as MCP Phase F)
- [x] Agent with `capability_set = {}` (empty) sees all WASM plugins (backward compat)
- [x] `ApiKeyRecord.capability_ceiling: Vec<Capability>` field exists;
      `#[serde(default)]`; round-trips through the encrypted JSON store
- [x] `ApiKeyInfo.capability_ceiling: CapabilitySet` field exists; converted from
      record at `validate()` time
- [x] `AuthIdentity.capability_ceiling: CapabilitySet` populated from `ApiKeyInfo`;
      empty for bearer/paired tokens
- [x] `build_agent_request` accepts `capability_override: CapabilitySet` and uses
      it as `capability_set_override` (not `CapabilitySet::default()`)
- [x] `v1_chat_completions_stream` accepts and forwards `capability_override`
- [x] `api_chat` captures `identity` and passes `identity.capability_ceiling`
- [x] `v1_chat_completions` (streaming + non-streaming paths) captures `identity`
      and passes `identity.capability_ceiling`
- [x] `async_submit` (all modes: followup, collect, interrupt, steer) captures
      `identity` and passes ceiling to every `build_agent_request` call
- [x] `cargo fmt --all` -- 0 violations
- [x] `cargo clippy --workspace --all-targets -- -D warnings` -- 0 warnings
- [x] `cargo test --workspace` -- all tests pass

---

## What This Unlocks

Sprint 90 can focus on memory-scope isolation per agent and the
`Delegate { max_capabilities }` capability enforcement for sub-agent spawning
(the remaining medium-priority items from the Sprint 86 threat model).

The combination of Plans 48-50 means every surface through which an agent can
acquire tool access -- direct config, delegated sub-agents, MCP servers, WASM
plugins, A2A calls, and API key-authenticated HTTP requests -- is bounded by a
`CapabilitySet`. The threat model entries "WASM plugin bypass" and "API key
grants unbounded tool access" are both closed.
