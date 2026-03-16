# Plan: Persistent Agent Management — CLI + Config UI + LLM Tool

## Context

The tweet describes a workflow where users can create specialized, persistent AI agents through natural language:

> "Create a new persistent agent named [Name] for [specific task]. Set [Model] as primary. Use [Name] for all [task type]."

AgentZero already has robust infrastructure for persistent agents (`AgentStore`, `AgentRouter`, `Coordinator`, `DelegateTool`), but lacks three surfaces for managing them:

1. **LLM tool** — so agents can create/manage other agents during conversation
2. **CLI subcommands** — human-facing CRUD from the terminal
3. **Config UI** — visual browser-based management via the React Flow node graph editor

## What Already Exists (reuse, don't rebuild)

| Component | Location | Status |
|-----------|----------|--------|
| `AgentStore` (full CRUD + encrypted persistence) | `crates/agentzero-orchestrator/src/agent_store.rs` | Complete |
| `AgentRecord`, `AgentUpdate`, `AgentStatus` | Same file | Complete |
| `AgentRouter` (AI + keyword routing) | `crates/agentzero-orchestrator/src/agent_router.rs` | Complete |
| `Coordinator.register_dynamic_agent_from_record()` | `crates/agentzero-orchestrator/src/coordinator.rs:278` | Complete |
| `to_swarm_config()` / `to_descriptor()` | `agent_store.rs:86-106` | Complete |
| `ToolSecurityPolicy` (boolean gates) | `crates/agentzero-tools/src/lib.rs:114-145` | Complete |
| `default_tools()` | `crates/agentzero-infra/src/tools/mod.rs:31` | Complete |
| Config UI Axum server + React Flow frontend | `crates/agentzero-config-ui/` | Complete (TOML agents only) |
| Agent node descriptor (schema, properties, ports) | `crates/agentzero-config-ui/src/schema.rs:290-433` | Complete |
| TOML ↔ Graph bridge for agents | `crates/agentzero-config-ui/src/toml_bridge.rs` | Complete |

## Implementation

### Step 1: Add `enable_agent_manage` to `ToolSecurityPolicy`

**File:** `crates/agentzero-tools/src/lib.rs`

- Add `pub enable_agent_manage: bool` field to `ToolSecurityPolicy` struct (after `enable_autopilot`)
- Default to `false` in `default_for_workspace()`

### Step 2: Create `AgentManageTool`

**File:** New file `crates/agentzero-infra/src/tools/agent_manage.rs`

Place it in `agentzero-infra` (not `agentzero-tools`) to avoid circular deps — `agentzero-infra` already depends on both `agentzero-tools` and `agentzero-orchestrator`.

**Tool design:**
- Name: `"agent_manage"`
- Single tool with `action` discriminator: `create`, `list`, `get`, `update`, `delete`, `set_status`
- Takes `Arc<AgentStore>` in constructor
- Parameters:
  - `action` (required): enum of CRUD operations
  - `name`: agent name (required for create)
  - `agent_id`: agent ID (required for get/update/delete/set_status)
  - `description`: what the agent does
  - `model`: model ID (e.g. `claude-sonnet-4-20250514`)
  - `provider`: provider name (e.g. `anthropic`, `openai`)
  - `system_prompt`: the agent's persona/instructions
  - `keywords`: routing keywords (powers "Use X for all Y" — the `AgentRouter` already matches on these)
  - `allowed_tools`: tool allowlist
  - `status`: `active`/`stopped` (for set_status)
- Returns human-readable text output (not JSON) so the LLM can naturally reference it

**How routing works:** When the LLM parses "Use Aria for all travel queries", it sets `keywords: ["travel"]`. The existing `AgentRouter.route_by_keywords()` already matches incoming messages against agent keywords. No new routing code needed.

### Step 3: Wire `AgentManageTool` into `default_tools()`

**File:** `crates/agentzero-infra/src/tools/mod.rs`

- Add `mod agent_manage;` at top
- Add new parameter `agent_store: Option<Arc<AgentStore>>` to `default_tools()` and `default_tools_with_depth()`
- Behind `policy.enable_agent_manage`, push `AgentManageTool::new(store)`
- Update all call sites of `default_tools()`:
  - `crates/agentzero-infra/src/tools/mod.rs` (the DelegateTool builder closure at line 217)
  - `crates/agentzero-infra/src/runtime.rs`
  - Test functions in `mod.rs`

### Step 4: Add `Agents` CLI subcommand (plural, to avoid breaking existing `Agent`)

**Files:**
- `crates/agentzero-cli/src/cli.rs` — Add `AgentsCommands` enum and `Agents` variant to `Commands`
- New file `crates/agentzero-cli/src/commands/agents.rs` — Handler implementation
- `crates/agentzero-cli/src/commands/mod.rs` — Add `pub mod agents;`
- `crates/agentzero-cli/src/lib.rs` — Add dispatch match arm + command name

**Subcommands:**
```
agentzero agents create --name Aria --description "Travel planner" --model claude-sonnet-4-20250514 --provider anthropic --keywords travel,booking
agentzero agents list [--json]
agentzero agents get --id <agent_id> [--json]
agentzero agents update --id <agent_id> [--name ...] [--model ...] [--keywords ...]
agentzero agents delete --id <agent_id>
agentzero agents status --id <agent_id> --active/--stopped
```

Handler pattern: instantiate `AgentStore::persistent(&ctx.data_dir)?` and call methods. Follow `cron.rs` pattern.

### Step 5: Config UI — Backend API for Persistent Agents

The config UI currently only manages TOML-based delegate agents (the `[agents.name]` config section). We need to add a separate set of REST endpoints for managing `AgentStore`-backed persistent agents — these are runtime-created agents that live in the encrypted `agents.json` store.

**File:** `crates/agentzero-config-ui/src/server.rs` — Add routes

New routes under `/api/agents`:
```
GET    /api/agents          — list all persistent agents
POST   /api/agents          — create a new persistent agent
GET    /api/agents/:id      — get agent by ID
PUT    /api/agents/:id      — update agent fields
DELETE /api/agents/:id      — delete agent
PUT    /api/agents/:id/status — set active/stopped
```

**File:** New file `crates/agentzero-config-ui/src/agents_api.rs` — Endpoint handlers

- Server needs shared `AgentStore` state via Axum's `State<Arc<AgentStore>>`
- Endpoint handlers map HTTP requests to `AgentStore` methods
- Request/response types: `CreateAgentRequest`, `UpdateAgentRequest`, `AgentResponse` (serialized `AgentRecord`)
- All operations return JSON

**File:** `crates/agentzero-config-ui/src/lib.rs` — Wire `AgentStore`

- `start_config_ui()` needs a `data_dir: &Path` parameter to construct `AgentStore::persistent(data_dir)`
- Pass `Arc<AgentStore>` to the router via `Router::with_state()`

**File:** `crates/agentzero-config-ui/Cargo.toml` — Add dependency

- Add `agentzero-orchestrator` dependency (for `AgentStore`, `AgentRecord`, etc.)
- Add `agentzero-storage` if needed (for `EncryptedJsonStore` transitive dep)

### Step 6: Config UI — Frontend Agent Management Panel

**File:** New file `crates/agentzero-config-ui/ui/src/panels/AgentsPanel.tsx`

A dedicated panel (tab in the bottom panel, alongside TOML Preview and Validation) for managing persistent agents:
- Table/list view of all persistent agents with columns: Name, Model, Status, Keywords, Created
- "Create Agent" button → modal/form with fields: name, description, provider, model, system_prompt, keywords, allowed_tools
- Inline edit for each agent row (click to expand properties)
- Status toggle (Active/Stopped)
- Delete button with confirmation
- Auto-refresh on mutations

**File:** `crates/agentzero-config-ui/ui/src/App.tsx` — Add Agents tab

- Add "Agents" tab alongside existing "TOML Preview" and "Validation" tabs in the bottom panel

**File:** New file `crates/agentzero-config-ui/ui/src/api/agents.ts` — API client

```typescript
export async function listAgents(): Promise<AgentRecord[]>
export async function createAgent(req: CreateAgentRequest): Promise<AgentRecord>
export async function getAgent(id: string): Promise<AgentRecord>
export async function updateAgent(id: string, req: UpdateAgentRequest): Promise<AgentRecord>
export async function deleteAgent(id: string): Promise<void>
export async function setAgentStatus(id: string, active: boolean): Promise<void>
```

**File:** `crates/agentzero-config-ui/ui/src/types.ts` — Add types

```typescript
interface AgentRecord {
  agent_id: string;
  name: string;
  description: string;
  provider: string;
  model: string;
  system_prompt?: string;
  keywords: string[];
  allowed_tools: string[];
  status: 'active' | 'stopped';
  created_at: number;
  updated_at: number;
}
```

### Step 7: Config UI — Schema & Tool Summary Updates

**File:** `crates/agentzero-config-ui/src/schema.rs`

- Add `enable_agent_manage` to the security policy node descriptor's "Automation & Integrations" group
- Add `agent_manage` to `build_tool_summaries()` (gated by `enable_agent_manage`)

### Step 8: Coordinator store sync (hot-loading)

**File:** `crates/agentzero-orchestrator/src/coordinator.rs`

Add `pub async fn sync_from_store(&self, store: &AgentStore, config_path: &Path, workspace_root: &Path)`:
1. List all agents from store
2. Register any Active agents not already registered (via `register_dynamic_agent_from_record`)
3. Deregister any agents that were deleted or Stopped
4. Call this on a timer in the coordinator's main loop (e.g., every 30s)

This is MVP — event-driven sync via `mpsc::Sender<AgentEvent>` can come later.

### Step 9: Config integration

**File:** `crates/agentzero-config/src/model.rs`

Add `enable_agent_manage: bool` to the TOML agent config section, wired through to `ToolSecurityPolicy`. Follow existing pattern for `enable_cron`, `enable_web_search`, etc.

## Files to modify (summary)

| File | Change |
|------|--------|
| `crates/agentzero-tools/src/lib.rs` | Add `enable_agent_manage` to `ToolSecurityPolicy` |
| `crates/agentzero-infra/src/tools/mod.rs` | Add `mod agent_manage`, wire into `default_tools()` |
| `crates/agentzero-infra/src/tools/agent_manage.rs` | **NEW** — `AgentManageTool` implementation |
| `crates/agentzero-cli/src/cli.rs` | Add `AgentsCommands` enum + `Agents` variant |
| `crates/agentzero-cli/src/commands/agents.rs` | **NEW** — CLI handler |
| `crates/agentzero-cli/src/commands/mod.rs` | Add `pub mod agents;` |
| `crates/agentzero-cli/src/lib.rs` | Add dispatch + command name |
| `crates/agentzero-config-ui/src/server.rs` | Add `/api/agents` routes |
| `crates/agentzero-config-ui/src/agents_api.rs` | **NEW** — REST endpoint handlers |
| `crates/agentzero-config-ui/src/lib.rs` | Add `data_dir` param, wire `AgentStore` state |
| `crates/agentzero-config-ui/src/schema.rs` | Add `enable_agent_manage` + tool summary |
| `crates/agentzero-config-ui/Cargo.toml` | Add `agentzero-orchestrator` dep |
| `crates/agentzero-config-ui/ui/src/panels/AgentsPanel.tsx` | **NEW** — Agent management UI |
| `crates/agentzero-config-ui/ui/src/api/agents.ts` | **NEW** — API client functions |
| `crates/agentzero-config-ui/ui/src/types.ts` | Add `AgentRecord` interface |
| `crates/agentzero-config-ui/ui/src/App.tsx` | Add Agents tab to bottom panel |
| `crates/agentzero-orchestrator/src/coordinator.rs` | Add `sync_from_store()` |
| `crates/agentzero-config/src/model.rs` | Add `enable_agent_manage` config field |
| `crates/agentzero-infra/src/runtime.rs` | Pass `AgentStore` to `default_tools()` |

## Verification

1. **Unit tests:** Test `AgentManageTool` execute with each action (create, list, get, update, delete, set_status) using in-memory `AgentStore`
2. **CLI tests:** Add parse tests for `agentzero agents create/list/get/update/delete/status` in `lib.rs` tests
3. **Config UI backend tests:** Test each `/api/agents` endpoint returns correct status codes and JSON
4. **Config UI frontend:** `cd crates/agentzero-config-ui/ui && npm run build` succeeds, Agents tab renders agent list
5. **Integration:** `cargo test --workspace` — all 1,426+ tests pass
6. **Clippy:** `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings
7. **Manual CLI:** Run `agentzero agents create --name Aria --description "Travel planner" --model claude-sonnet-4-20250514 --provider anthropic --keywords travel,booking` → verify persists → `agentzero agents list` shows it
8. **Manual Config UI:** Run `agentzero config-ui` → open browser → Agents tab → create agent → verify it appears in list and persists across restarts
