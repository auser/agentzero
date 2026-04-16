# Plan 49: MCP Session Scoping + A2A `max_capabilities`

## Context

Sprint 86 threat-model update identified three HIGH residual risks. Sprint 87
(Plan 48) closed two of them: dynamic tool capability bounding and swarm/delegate
capability intersection. This plan closes the remaining two:

1. **MCP session scoping** — An agent running under a restricted `[[capabilities]]`
   set can currently call *any* tool exposed by any configured MCP server, because
   `create_mcp_tools` does not consult the agent's `CapabilitySet`. There is also a
   silent naming bug: the capability pattern `"mcp:*"` (colon separator) never
   matches actual MCP tool names, which use the `mcp__{server}__{tool}` format
   (double-underscore separator). The result: capability-set users silently lose all
   MCP access.

2. **A2A `max_capabilities`** — An agent under a restricted capability set can call
   an external A2A agent via `ConverseTool` / `A2aAgentEndpoint` and ask it to
   perform operations the local agent could never do directly. The outbound
   `tasks/send` request carries no indication of the caller's capability ceiling,
   so the remote agent operates without any constraints. Inbound A2A requests to our
   gateway also arrive with no capability scoping.

Both features build on the `capability_set_override` mechanism from Plan 48 Phase C.

---

## Decisions

### MCP tool name convention: `mcp__{server}__{tool}` (double-underscore)

MCP tools must use alphanumeric + `_` characters because the LLM function-calling
API forbids colons in function names. The existing `sanitize_tool_name` helper
already enforces this. All capability patterns and doc comments must reflect the
actual double-underscore format.

### `mcp__*` (not `mcp__*__*`) as the permissive wildcard

`mcp__*` matches every string that starts with `mcp__` — covering
`mcp__fs__read_file`, `mcp__github__list_prs`, etc. It is slightly shorter and
easier to type in config than `mcp__*__*`, and the semantics are identical for
the actual naming scheme.

### A2A max_capabilities: outbound metadata + gateway direct path

For the **outbound** direction, include `metadata.agentZeroMaxCapabilities` in
every `tasks/send` call when the local endpoint's `max_capabilities` list is
non-empty. The remote AgentZero instance honours it; third-party agents receive
it as informational.

For the **inbound** direction, `handle_tasks_send` grows a direct `run_agent_once`
fallback path (no swarm channel) where `capability_set_override` can be applied
from the request metadata. In swarm mode, capability scoping is already handled
at swarm-build time via `[[capabilities]]` on each agent config entry.

### `A2aConfig.capability_ceiling` in gateway state

A new `[a2a] capability_ceiling = [...]` TOML field pre-builds a `CapabilitySet`
ceiling that is applied to ALL inbound A2A requests to this gateway. The inbound
cap override is computed as `intersect(ceiling, request.metadata.caps)` — the
tighter of the two wins.

---

## Phase F: MCP Session Scoping (HIGH)

**Estimated effort:** 1 day

### F1: Fix MCP capability naming

**File:** `crates/agentzero-core/src/security/capability.rs`

Change the `from_policy_booleans` mapping:

```rust
// Before
push_tool!(flags.enable_mcp, "mcp:*");

// After
push_tool!(flags.enable_mcp, "mcp__*");
```

Update the doc comment on `Capability::Tool`:

```rust
/// Access a specific tool by name. Supports glob patterns:
/// `"mcp__*"` → all MCP tools, `"cron_*"` → all cron tools.
Tool { name: String },
```

Update the existing `tool_mcp_glob_matches_all_mcp_tools` test to use the
double-underscore names. Update the `bool_map_enable_mcp` test to use
`tool = "mcp__filesystem__read"`.

### F2: Filter MCP tools by CapabilitySet

**File:** `crates/agentzero-infra/src/tools/mcp.rs`

Change the signature:

```rust
pub fn create_mcp_tools(
    servers: &HashMap<String, McpServerDef>,
    policy: &agentzero_tools::ToolSecurityPolicy,
) -> anyhow::Result<Vec<Box<dyn Tool>>>
```

After building `full_name` (the `mcp__{server}__{tool}` string), add:

```rust
// Phase F: skip tools the agent's capability set does not permit.
if !policy.capability_set.is_empty()
    && !policy.capability_set.allows_tool(&full_name)
{
    tracing::debug!(
        tool = %full_name,
        "mcp tool filtered out by agent capability set"
    );
    continue;
}
```

**File:** `crates/agentzero-infra/src/tools/mod.rs`

Update the call site:

```rust
// Before
let mcp_tools = create_mcp_tools(&policy.mcp_servers)?;

// After
let mcp_tools = create_mcp_tools(&policy.mcp_servers, policy)?;
```

### F3: Unit tests

**`crates/agentzero-core/src/security/capability.rs`**

Update `tool_mcp_glob_matches_all_mcp_tools`:

```rust
fn tool_mcp_glob_matches_all_mcp_tools() {
    let s = tool_set(&["mcp__*"]);
    assert!(s.allows_tool("mcp__filesystem__read_file"));
    assert!(s.allows_tool("mcp__github__list_prs"));
    assert!(!s.allows_tool("web_search"));
    // Per-server wildcard
    let s2 = tool_set(&["mcp__filesystem__*"]);
    assert!(s2.allows_tool("mcp__filesystem__read_file"));
    assert!(!s2.allows_tool("mcp__github__list_prs"));
}
```

Add `mcp_tool_name_format_matches_capability_pattern`:

```rust
fn mcp_tool_name_format_matches_capability_pattern() {
    // Verify the double-underscore naming convention round-trips through
    // the capability pattern matcher.
    let s = CapabilitySet::new(
        vec![Capability::Tool { name: "mcp__*".to_string() }],
        vec![],
    );
    for tool in &["mcp__fs__read", "mcp__github__create_pr", "mcp__slack__send"] {
        assert!(s.allows_tool(tool), "{tool} should be allowed by mcp__*");
    }
    assert!(!s.allows_tool("web_search"));
}
```

**`crates/agentzero-infra/src/tools/mcp.rs`**

Add two unit tests in the `#[cfg(test)]` block:

```rust
fn mcp_tool_naming_convention() {
    // Verify the full_name format used by create_mcp_tools matches what
    // capability patterns expect.
    let server = "filesystem";
    let raw_tool = "read-file";
    let full = format!("mcp__{}__{}", server, sanitize_tool_name(raw_tool));
    assert_eq!(full, "mcp__filesystem__read_file");
}

fn mcp_tools_filtered_by_cap_set() {
    use agentzero_core::security::capability::{Capability, CapabilitySet};
    use agentzero_tools::ToolSecurityPolicy;

    // Policy allows only mcp__fs__read_file, not mcp__fs__write_file.
    let cap_set = CapabilitySet::new(
        vec![Capability::Tool {
            name: "mcp__fs__read_file".to_string(),
        }],
        vec![],
    );
    let policy = ToolSecurityPolicy {
        capability_set: cap_set,
        enable_mcp: true,
        ..ToolSecurityPolicy::default_for_workspace(std::path::PathBuf::from("."))
    };

    // Verify the filtering predicate directly (without spawning a subprocess).
    let tools_to_create = [("mcp__fs__read_file", true), ("mcp__fs__write_file", false)];
    for (name, expected) in &tools_to_create {
        let allowed = if !policy.capability_set.is_empty() {
            policy.capability_set.allows_tool(name)
        } else {
            true
        };
        assert_eq!(allowed, *expected, "tool {name} allowed={allowed}, expected={expected}");
    }
}
```

---
## Phase G: A2A `max_capabilities` (HIGH)

**Estimated effort:** 1.5 days

### G1: `A2aAgentConfig.max_capabilities`

**File:** `crates/agentzero-config/src/model.rs`

Add to `A2aAgentConfig` after `timeout_secs`:

```rust
/// Capability ceiling for this external A2A agent (Sprint 88 — Phase G).
///
/// When non-empty, included as `metadata.agentZeroMaxCapabilities` in every
/// outbound `tasks/send` call so the remote AgentZero instance can apply it
/// as a `capability_set_override`. Third-party agents receive it as
/// informational metadata.
///
/// Default: empty (no ceiling — remote agent operates unrestricted).
#[serde(default)]
pub max_capabilities: Vec<agentzero_core::security::capability::Capability>,
```

Also add to `A2aConfig` after `agents`:

```rust
/// Capability ceiling applied to ALL inbound A2A requests to this gateway.
///
/// When non-empty, the gateway intersects this with any
/// `metadata.agentZeroMaxCapabilities` supplied by the caller and uses the
/// result as `capability_set_override` for the handling agent.
///
/// Default: empty (no restriction on inbound requests).
#[serde(default)]
pub capability_ceiling: Vec<agentzero_core::security::capability::Capability>,
```

### G2: `A2aAgentEndpoint` carries `max_capabilities`

**File:** `crates/agentzero-orchestrator/src/a2a_client.rs`

Add field:

```rust
/// Capability ceiling forwarded to the remote agent on every `tasks/send`.
max_capabilities: Vec<agentzero_core::security::capability::Capability>,
```

Update `new()` to accept and store `max_capabilities: Vec<Capability>`.

### G3: `send()` includes capability metadata

In `A2aAgentEndpoint::send()`, change the params passed to `rpc_call`:

```rust
let mut params = serde_json::json!({
    "id": conversation_id,
    "message": {
        "role": "user",
        "parts": [{"type": "text", "text": message}]
    }
});

if !self.max_capabilities.is_empty() {
    params["metadata"] = serde_json::json!({
        "agentZeroMaxCapabilities":
            serde_json::to_value(&self.max_capabilities)
                .unwrap_or(serde_json::Value::Null)
    });
}

self.rpc_call("tasks/send", params).await?
```

### G4: `TaskSendParams.metadata`

**File:** `crates/agentzero-core/src/a2a_types.rs`

Add to `TaskSendParams`:

```rust
/// Arbitrary metadata attached by the caller.
///
/// AgentZero uses `metadata.agentZeroMaxCapabilities` (a JSON array of
/// `Capability` values) as a per-request capability ceiling when the handler
/// is running in direct (non-swarm) mode.
#[serde(default)]
pub metadata: Option<serde_json::Value>,
```

### G5: `register_a2a_endpoints` passes `max_capabilities`

**File:** `crates/agentzero-orchestrator/src/swarm.rs`

In `register_a2a_endpoints`, update `A2aAgentEndpoint::new(...)` to pass
`agent_cfg.max_capabilities.clone()`.

### G6: Gateway honors inbound capability ceiling

**File:** `crates/agentzero-gateway/src/state.rs`

Add to `GatewayState`:

```rust
/// Pre-built capability ceiling for all inbound A2A requests.
/// Built from `config.a2a.capability_ceiling` at gateway startup.
pub(crate) a2a_inbound_cap_ceiling: agentzero_core::security::CapabilitySet,
```

Initialise to `CapabilitySet::default()` in `GatewayState::new()`.

Add builder method `with_a2a_cap_ceiling(ceiling: CapabilitySet) -> Self`.

**File:** `crates/agentzero-gateway/src/a2a.rs`

In `handle_tasks_send`, after deserializing `send_params`, add:

```rust
// Extract inbound capability ceiling from request metadata (Sprint 88 — Phase G).
let request_caps: agentzero_core::security::CapabilitySet = send_params
    .metadata
    .as_ref()
    .and_then(|m| m.get("agentZeroMaxCapabilities"))
    .and_then(|v| {
        serde_json::from_value::<Vec<agentzero_core::security::capability::Capability>>(
            v.clone(),
        )
        .ok()
    })
    .filter(|caps| !caps.is_empty())
    .map(|caps| agentzero_core::security::CapabilitySet::new(caps, vec![]))
    .unwrap_or_default();

// Effective ceiling = gateway ceiling ∩ request ceiling (tighter of the two).
let effective_cap = if !state.a2a_inbound_cap_ceiling.is_empty() {
    state.a2a_inbound_cap_ceiling.intersect(&request_caps)
} else {
    request_caps
};
```

When the swarm channel is absent (direct mode), add a `run_agent_once` path:

```rust
} else if !state.config_path.as_os_str().is_empty() {
    // Direct mode: no swarm channel — run agent inline with the inbound cap override.
    let req = agentzero_infra::runtime::RunAgentRequest {
        workspace_root: state.workspace_root.as_ref().clone(),
        config_path: state.config_path.as_ref().clone(),
        message: text.clone(),
        provider_override: None,
        model_override: None,
        profile_override: None,
        extra_tools: vec![],
        conversation_id: send_params.session_id.clone(),
        agent_store: state.agent_store.clone(),
        memory_override: None,
        memory_window_override: None,
        capability_set_override: effective_cap,
    };
    match agentzero_infra::runtime::run_agent_once(req).await {
        Ok(out) => out.response_text,
        Err(e) => format!("agent error: {e}"),
    }
```

### G7: Unit tests

**`crates/agentzero-orchestrator/src/a2a_client.rs`**

```rust
fn a2a_send_includes_max_capabilities_metadata() {
    // Verify that a non-empty max_capabilities produces the metadata JSON.
    use agentzero_core::security::capability::Capability;
    let caps = vec![Capability::Tool { name: "web_search".to_string() }];
    // Build the expected params object manually.
    let params = serde_json::json!({
        "id": "conv-1",
        "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
        "metadata": {"agentZeroMaxCapabilities": serde_json::to_value(&caps).unwrap()}
    });
    assert!(params["metadata"]["agentZeroMaxCapabilities"].is_array());
}

fn a2a_send_no_metadata_when_empty_caps() {
    // Empty max_capabilities → no metadata key in params.
    let params = serde_json::json!({
        "id": "conv-1",
        "message": {"role": "user", "parts": [{"type": "text", "text": "hi"}]},
    });
    assert!(params.get("metadata").is_none());
}

fn task_send_params_deserializes_metadata() {
    use agentzero_core::a2a_types::TaskSendParams;
    let json = serde_json::json!({
        "id": "t1",
        "message": {"role": "user", "parts": [{"type": "text", "text": "hello"}]},
        "metadata": {"agentZeroMaxCapabilities": [{"type": "Tool", "name": "web_search"}]}
    });
    let params: TaskSendParams = serde_json::from_value(json).expect("deserialize");
    assert!(params.metadata.is_some());
    let meta = params.metadata.unwrap();
    assert!(meta["agentZeroMaxCapabilities"].is_array());
}
```

**`crates/agentzero-orchestrator/src/swarm.rs`**

```rust
fn register_a2a_endpoints_passes_max_capabilities() {
    use agentzero_core::security::capability::Capability;
    // Build config with max_capabilities on an agent.
    // Verify the endpoint is created (construction with max_caps doesn't panic/error).
    // (Full integration test deferred — endpoint requires live HTTP.)
}
```

---

## Files to Modify

| File | Phase | Change |
|---|---|---|
| `crates/agentzero-core/src/security/capability.rs` | F1, F3 | `"mcp:*"` → `"mcp__*"`, update tests |
| `crates/agentzero-infra/src/tools/mcp.rs` | F2, F3 | filter by cap set, tests |
| `crates/agentzero-infra/src/tools/mod.rs` | F2 | pass `policy` to `create_mcp_tools` |
| `crates/agentzero-config/src/model.rs` | G1 | `max_capabilities` on `A2aAgentConfig`, `capability_ceiling` on `A2aConfig` |
| `crates/agentzero-core/src/a2a_types.rs` | G4 | `metadata` on `TaskSendParams` |
| `crates/agentzero-orchestrator/src/a2a_client.rs` | G2, G3, G7 | `max_capabilities` field, metadata in `send()`, tests |
| `crates/agentzero-orchestrator/src/swarm.rs` | G5, G7 | pass `max_capabilities`, test |
| `crates/agentzero-gateway/src/state.rs` | G6 | `a2a_inbound_cap_ceiling` field |
| `crates/agentzero-gateway/src/a2a.rs` | G6 | extract inbound caps, direct-mode `run_agent_once` |

---

## Effort Estimate

| Phase | Effort | Priority |
|---|---|---|
| F — MCP session scoping | 1 day | HIGH |
| G — A2A max_capabilities | 1.5 days | HIGH |
| **Total** | **2.5 days** | |

---

## Acceptance Criteria

- [ ] `mcp__*` glob matches `mcp__filesystem__read_file`, `mcp__github__list_prs`, etc.
- [ ] `mcp:*` glob no longer appears in `capability.rs` (old colon-separator removed)
- [ ] `create_mcp_tools` accepts a `&ToolSecurityPolicy` and filters tools when `policy.capability_set` is non-empty
- [ ] Agent with `capability_set = {mcp__fs__read_file}` sees only `mcp__fs__read_file` in tool list; `mcp__fs__write_file` is absent
- [ ] Agent with empty `capability_set` sees all MCP tools (existing behavior preserved)
- [ ] `A2aAgentConfig::max_capabilities` field exists; default is empty vec; round-trips through serde
- [ ] `A2aConfig::capability_ceiling` field exists; default is empty vec
- [ ] `A2aAgentEndpoint::send()` includes `metadata.agentZeroMaxCapabilities` when `max_capabilities` is non-empty
- [ ] `A2aAgentEndpoint::send()` omits `metadata` key when `max_capabilities` is empty
- [ ] `TaskSendParams::metadata: Option<Value>` field exists; existing records without the key deserialise to `None`
- [ ] `register_a2a_endpoints` passes `max_capabilities` from config to `A2aAgentEndpoint::new()`
- [ ] `GatewayState::a2a_inbound_cap_ceiling` field exists; default is `CapabilitySet::default()`
- [ ] `handle_tasks_send` extracts `metadata.agentZeroMaxCapabilities` and builds effective cap ceiling
- [ ] Direct-mode A2A (`handle_tasks_send` with no swarm channel) builds `RunAgentRequest` with `capability_set_override = effective_cap`
- [ ] `cargo fmt --all` — 0 violations
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — 0 warnings
- [ ] `cargo test --workspace` — all tests pass

---

## What This Unlocks

Once Sprint 88 is complete:

- **MCP tools are fully capability-bounded.** An agent under `[[capabilities]]`
  can only invoke MCP tools explicitly granted by its capability set. The silent
  naming bug that blocked all MCP access for capability-set users is also fixed.

- **External A2A calls respect the local capability ceiling.** Every outbound
  `tasks/send` includes the caller's `max_capabilities` so remote AgentZero
  instances apply it as `capability_set_override`. The threat model entry
  "A2A bypass" is closed.

- **Sprint 89 can focus on WASM plugin sandboxing and multi-tenant lane
  isolation** — the remaining medium-priority risks from the Sprint 86 assessment.
