# Blender-Style Node Rendering

**Status**: Planned
**Depends on**: Sprint 60 (visual workflow builder MVP)

## Context

The current workflow-graph node renderer draws basic rectangular nodes with ports. The goal is to match Blender's Shader Editor visual style for a professional node-graph experience.

## Reference

Blender Shader Editor nodes have:
- Colored header bar per node type (red, blue, green, purple)
- Collapsible sections within nodes (e.g., "Diffuse", "Subsurface")
- Inline value editors on ports (sliders, text fields, dropdowns)
- Port dots precisely on the node border edge
- Port colors matching data type (yellow=float, purple=vector, green=shader, gray=unconnected)
- Thicker bezier edges colored to match port type
- Compact vertical spacing between ports
- Dropdown selectors on nodes (e.g., interpolation mode)

## Implementation Plan

### Phase 1: Visual Node Redesign (workflow-graph WASM)

**File**: `crates/web/src/render.rs`

1. **Colored header bar** — top 24px of node filled with type-specific color
   - Agent: blue (#3b82f6), Tool: violet (#8b5cf6), Channel: pink (#ec4899)
   - Schedule: yellow (#eab308), Gate: red (#ef4444), SubAgent: green (#22c55e)
2. **Port dot positioning** — dots exactly on the node border (currently slightly offset)
3. **Port type colors** — match Blender: text=yellow, json=purple, event=green, config=gray, tool_call=orange
4. **Thicker edge curves** — 2.5px width, colored to match source port type
5. **Compact port spacing** — reduce vertical gap between ports
6. **Node shadow** — subtle drop shadow for depth

### Phase 2: Inline Values (workflow-graph WASM + React)

1. **Value display on ports** — show current value next to port label (e.g., "Roughness: 0.500")
2. **Port metadata** — `Port` struct gains `value: Option<Value>` field
3. **Render values** — right-aligned text for input ports, left-aligned for output ports

### Phase 3: Collapsible Sections (workflow-graph WASM)

1. **Section headers** — clickable group labels within nodes
2. **Expand/collapse state** — toggle visibility of port groups
3. **Dynamic node height** — node height adjusts when sections collapse

### Phase 4: Inline Editors (React overlay)

1. **Click port value → editable** — overlay an HTML input on the port position
2. **Slider for numeric values** — horizontal slider within the port row
3. **Dropdown for enums** — select element for port type choices

## Key Files

- `workflow-graph/crates/web/src/render.rs` — draw_node, draw_ports, draw_edge
- `workflow-graph/crates/web/src/theme.rs` — ThemeConfig, node type colors
- `workflow-graph/shared/src/lib.rs` — Port struct (value field)
- `agentzero/ui/src/components/workflows/WorkflowCanvas.ts` — port definitions
- `agentzero/ui/src/components/workflows/NodeRenderer.ts` — custom render callback (if used)
