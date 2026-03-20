# Visual Workflow Builder (LangChain Fleet-style)

**Decisions**: WASM-level node creation API in workflow-graph. Full MVP scope (Phases 1-3).

**Plan file**: `specs/plans/26-visual-workflow-builder.md`
**Branch**: `feat/visual-workflow-builder`

---

## Phase 0: Project Setup

- [x] Checkout branch `feat/visual-workflow-builder` from `main`
- [x] Save this plan to `specs/plans/26-visual-workflow-builder.md`
- [ ] Add sprint entry to `specs/SPRINT.md`
- [ ] Keep `specs/SPRINT.md` updated with task completion status throughout implementation

**Tauri compatible**: WASM + Canvas2D rendering works natively in Tauri's WebView. The React UI bundle (`ui/dist/`) can be served via `rust_embed` or Tauri's asset protocol. Gateway API runs on localhost — Tauri IPC can optionally bypass HTTP later for lower latency, but REST works out of the box.

---

## Context

AgentZero needs a visual workflow builder similar to [LangChain Fleet](https://www.langchain.com/langsmith/fleet) — a drag-and-drop UI for composing agent workflows with tools, sub-agents, channels, schedules, and approval gates. The user's own [workflow-graph](https://github.com/auser/workflow-graph) library (Rust+WASM Canvas2D renderer with React wrapper) provides the graph visualization foundation but needs extensions for **new node types** beyond its current "Job" model.

## Approach: Extend workflow-graph + Build Adapter Layer

1. **Upstream**: Add a `metadata` field to workflow-graph's `Job` struct so custom renderers can distinguish node types without polluting the core library with domain-specific types
2. **AgentZero UI**: Build a React adapter layer that maps AgentZero's domain model (agents, tools, channels, schedules) to workflow-graph nodes and back
3. **Rendering**: Use the existing `onRenderNode` callback for type-specific visuals (different shapes/colors/icons per node type)

---

## Phase 1: workflow-graph Extension (upstream)

**Goal**: Enable creating new node types via metadata.

### Changes in `workflow-graph` repo

1. **Add `metadata` to `Job` struct** in `shared/src/lib.rs`:
   ```rust
   #[serde(default)]
   pub metadata: HashMap<String, serde_json::Value>,
   ```
   Backward-compatible, non-breaking. Carries `node_type`, `description`, `icon`, `approval_required`, etc.

2. **Add WASM-level node CRUD API** — the canvas owns the graph state:
   - `add_node(job: Job) -> Result<()>` — insert node, trigger re-layout
   - `remove_node(id: &str) -> Result<()>` — remove node + connected edges, re-layout
   - `update_node(id: &str, partial: JsValue) -> Result<()>` — patch node properties (name, status, metadata)
   - `add_edge(from_id: &str, to_id: &str, metadata: JsValue) -> Result<()>` — create edge with optional metadata
   - `remove_edge(from_id: &str, to_id: &str) -> Result<()>` — delete edge, re-layout
   - `get_nodes() -> JsValue` — return current node list (for JS store sync)
   - `get_edges() -> JsValue` — return current edge list
   - These methods live in `crates/web/src/lib.rs` on the `WorkflowGraphController` struct
   - Each mutation triggers incremental re-layout + re-render (not full rebuild)

3. **Expose edge metadata** — `Edge` struct gets `metadata: HashMap<String, Value>` for conditional labels (approval, subscription)

4. **React wrapper updates** — `@auser/workflow-graph-react` exposes imperative handle:
   ```tsx
   ref.current.addNode(job)
   ref.current.removeNode(id)
   ref.current.addEdge(from, to, metadata)
   ref.current.removeEdge(from, to)
   ref.current.updateNode(id, partial)
   ```

5. **Publish** as v0.5.0

### Key file
- `workflow-graph/shared/src/lib.rs` — Job struct

---

## Phase 2: Read-Only Visualization

**Goal**: Replace the simple `TopologyGraph` with an interactive workflow-graph-based view.

### Files to create
| File | Purpose |
|------|---------|
| `ui/src/routes/workflows/index.tsx` | Workflow list/visualization page |
| `ui/src/components/workflows/WorkflowCanvas.tsx` | Wraps `WorkflowGraphComponent`, handles WASM init |
| `ui/src/components/workflows/NodeRenderer.ts` | `onRenderNode` callback — draws distinct visuals per `metadata.node_type` |

### Files to modify
| File | Change |
|------|--------|
| `ui/package.json` | Add `@auser/workflow-graph-react`, `@auser/workflow-graph-web` |
| `ui/src/components/layout/Sidebar.tsx` | Add "Workflows" nav entry (GitBranch icon, after Agents) |
| `crates/agentzero-gateway/src/router.rs` | Add `*.wasm` to embedded-ui include list |

### Data flow
`GET /v1/topology` + `GET /v1/config` → convert to workflow-graph `Workflow` → render

---

## Phase 3: Visual Builder MVP

**Goal**: Drag-and-drop workflow composition that deploys to the live swarm.

### Node types

| Type | Maps to | Config fields |
|------|---------|---------------|
| Agent | `SwarmAgentConfig` | name, description, system_prompt, model, provider, allowed_tools |
| Tool | Entry in agent's `allowed_tools` | tool_name, require_approval |
| SubAgent | `DelegateAgentConfig` | agent_id or inline config |
| Channel | Channel config | channel_type (slack/discord/telegram), connect status |
| Schedule | Cron job | cron expression, message |
| Gate | Approval flag | gate_type (manual/threshold), timeout |

### Files to create

| File | Purpose |
|------|---------|
| `ui/src/store/workflowBuilderStore.ts` | Zustand store: nodes, edges, selection, dirty flag, serialization |
| `ui/src/routes/workflows/builder.tsx` | Builder page composing canvas + palette + inspector + popover |
| `ui/src/components/workflows/NodePalette.tsx` | Left sidebar with draggable node types |
| `ui/src/components/workflows/NodeInspector.tsx` | Right panel — full property editor (slide-out sheet, Radix Sheet) |
| `ui/src/components/workflows/NodePopover.tsx` | Inline popover anchored to clicked node — quick summary + edit key fields without leaving the canvas. Shows node name, status, 2-3 key fields, and "Open full editor" link to NodeInspector. Uses Radix Popover. |
| `ui/src/components/workflows/AgentNodeForm.tsx` | Agent config form (name, prompt, model, tools) |
| `ui/src/components/workflows/ToolNodeForm.tsx` | Tool selection + approval toggle |
| `ui/src/components/workflows/ChannelNodeForm.tsx` | Channel picker |
| `ui/src/components/workflows/ScheduleNodeForm.tsx` | Cron expression input |
| `ui/src/components/workflows/GateNodeForm.tsx` | Approval gate config |
| `ui/src/components/workflows/SubAgentNodeForm.tsx` | Sub-agent reference |
| `ui/src/components/workflows/WorkflowToolbar.tsx` | Save, Deploy, Export TOML, Import, Auto-layout, Zoom |
| `ui/src/components/workflows/QuickCreateWizard.tsx` | Step-by-step wizard dialog for creating a basic workflow (see below) |
| `ui/src/lib/api/workflows.ts` | API client: deploy via `PUT /v1/config`, fetch current config |

### Node editing UX (two-tier)

1. **Click node → NodePopover** (inline, anchored to node position on canvas)
   - Shows: node name, type badge, 2-3 key fields (e.g., model for Agent, cron for Schedule)
   - Quick-edit inline fields without leaving the canvas context
   - "Open full editor →" button opens NodeInspector
   - Dismiss by clicking elsewhere on canvas

2. **Double-click node or "Open full editor" → NodeInspector** (right-side Sheet)
   - Full property form for the node type (AgentNodeForm, ToolNodeForm, etc.)
   - All fields, system prompt textarea, tool multi-select, etc.
   - Stays open while you interact with the canvas (non-modal)

### Quick-Create Wizard

**File**: `ui/src/components/workflows/QuickCreateWizard.tsx`

Accessible from: "New Workflow" button on `/workflows` list page + empty state on builder page.

**Steps** (Radix Dialog with step indicator):
1. **Name & describe** — workflow name, one-line description
2. **Pick an agent** — select existing agent or create new (name + model + system prompt)
3. **Attach tools** — multi-select from available tools, toggle approval per tool
4. **Connect a channel** — optional: pick Slack/Discord/Telegram/Chat or skip
5. **Set a schedule** — optional: cron expression or "manual trigger only"
6. **Review & create** — summary card showing the workflow graph preview, "Create" button

On submit: populates the builder canvas with the configured nodes + edges, user can further edit visually before deploying.

### Serialization

**Builder → SwarmConfig** (`toSwarmConfig()`):
- Agent nodes → `swarm.agents` map entries
- Tool nodes connected to Agent → agent's `allowed_tools`
- Edges between Agents → `subscribes_to`/`produces` topics or pipeline steps
- Schedule nodes → cron job entries
- Gate connections → `approval_required_tools` list on agent

**SwarmConfig → Builder** (`loadFromSwarmConfig()`):
- Parse `swarm.agents` into Agent nodes
- Parse `swarm.pipelines` into ordered edge chains
- Infer Tool/Channel/Schedule nodes from agent configs
- Auto-layout via workflow-graph

### Backend: Zero new endpoints for MVP
- Deploy: `PUT /v1/config` with swarm section
- Fetch: `GET /v1/config` → extract swarm
- Tools list: `GET /v1/tools`
- Agent CRUD: existing `GET/POST/PATCH/DELETE /v1/agents`
- Cron: existing `GET/POST /v1/cron`

### Config model addition
Add to `SwarmAgentConfig` in `crates/agentzero-config/src/model.rs`:
```rust
#[serde(default)]
pub approval_required_tools: Vec<String>,
```

---

## Phase 4: Advanced Features

- Pipeline mode with sequential step ordering + fan-out visualization
- Edge styling (dashed=subscription, colored=data flow, red=approval gate)
- Undo/redo in builder store
- Workflow draft persistence to backend (`/v1/workflows` CRUD)
- Live status overlay (merge builder view with topology data)
- Validation overlay (missing fields, circular deps)
- Dark theme alignment
- Keyboard shortcuts (Delete, Ctrl+S, Ctrl+Z)
- E2E tests (Playwright)

---

## Verification

1. **workflow-graph extension**: Run `cargo test` in workflow-graph repo, verify metadata roundtrips through WASM
2. **Read-only view**: Start gateway, visit `/workflows`, verify topology renders with correct node types
3. **Builder**: Create a workflow with 2 agents + tools, deploy, verify `GET /v1/config` shows correct swarm section
4. **Approval gates**: Toggle approval on a tool, deploy, verify `approval_required_tools` appears in config
5. **Round-trip**: Load existing swarm config → edit in builder → deploy → reload → verify no data loss
