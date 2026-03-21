# Workflow Graph V2 — Production Design

**Status**: Planned
**Depends on**: Sprint 60 (current), specs/plans/27-blender-style-node-rendering.md

## Design References

1. **chaiNNer** — Canvas-based node editor with inline previews, dropdowns, sliders, image thumbnails inside nodes. Categorized palette with favorites/search.
2. **LangChain/Langflow** — Agent workflow with User→Agent→Model→Researcher chain. Prompt fields, role dropdowns, tool counts, response preview inside nodes. Card-based flow templates.

## Key Features to Implement

### Multi-Select & Grouping (workflow-graph WASM)
- [ ] **Shift+click multi-select** — already supported, needs visual group highlight
- [ ] **Drag-select rectangle** — click empty space + drag draws selection rectangle
- [ ] **Group into compound node** — select multiple nodes → right-click → "Group" → creates a single node that contains the sub-graph
- [ ] **Expand/collapse groups** — click to expand a compound node to see its contents
- [ ] **Copy/paste nodes** — Cmd+C/Cmd+V for selected nodes

### Inline Node Content (workflow-graph WASM + React overlay)
- [ ] **Text fields in nodes** — editable prompt/config fields rendered inside nodes
- [ ] **Dropdown selectors** — model picker, role selector inside nodes
- [ ] **Sliders** — numeric value controls (temperature, etc.)
- [ ] **Preview thumbnails** — show output previews inside nodes
- [ ] **Code blocks** — syntax-highlighted code editor inside nodes
- [ ] **Badges/counters** — "2 added" tool count badges

### Node Card Design (workflow-graph WASM renderer)
- [ ] **Colored header bar** — type-specific color (blue=agent, violet=tool, pink=channel)
- [ ] **Icon in header** — node type icon (bot, wrench, radio, etc.)
- [ ] **Dynamic height** — node height adjusts to content
- [ ] **Rounded corners** — larger radius, subtle shadow
- [ ] **Status indicator** — colored dot or badge (running, success, error)
- [ ] **Port labels with type indicators** — "Image 896x896 RGBA" style labels

### Flow Templates (UI)
- [ ] **Template cards** — pre-built workflow templates (Content Search, Code Debugger, API Integration, Basic Agent, Doc Assistant)
- [ ] **Template gallery** — browsable grid with descriptions and previews
- [ ] **One-click deploy** — click template → instantiates as a workflow

### LangChain-style Features
- [ ] **User node** — represents the human in the workflow (input/output)
- [ ] **Response preview** — shows agent response text inline in the node
- [ ] **"Responding..." animation** — live status during execution
- [ ] **Model node** — standalone model selector with provider logo
- [ ] **Tool count badge** — "2 added" on tool ports

## Implementation Phases

### Phase A: Multi-select + Grouping (workflow-graph)
1. Drag-select rectangle in WASM
2. Group/ungroup compound nodes
3. Copy/paste

### Phase B: Inline Content (hybrid WASM + React)
1. HTML overlay system — React components positioned over canvas nodes
2. Text input overlay for prompt fields
3. Dropdown overlay for model/role selection
4. Code editor overlay (Monaco or CodeMirror)

### Phase C: Visual Polish (workflow-graph renderer)
1. Colored headers, icons, dynamic height
2. Port type labels with data previews
3. Shadows, rounded corners, status badges

### Phase D: Templates + LangChain UX
1. Template gallery page
2. User/Response/Model node types
3. Live execution status overlay

## Key Files
- `workflow-graph/crates/web/src/render.rs` — node rendering
- `workflow-graph/crates/web/src/lib.rs` — interaction handling
- `workflow-graph/shared/src/lib.rs` — data model (compound nodes)
- `agentzero/ui/src/components/workflows/` — React overlays
- `agentzero/ui/src/components/dashboard/WorkflowTopology.tsx` — main component
