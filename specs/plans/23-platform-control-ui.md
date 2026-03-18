# Plan: AgentZero Platform Control UI — Sprint 46

## Context

The platform has a powerful REST/WebSocket gateway (33 endpoints) and a specialized config-ui (TOML node-graph editor), but no unified control surface for the whole platform. Sprint 46 builds a comprehensive SPA at `ui/` that controls everything — agents, runs, chat, tools, channels, models, config, memory, cron, approvals, and real-time events — while being Tauri-embeddable in the future without code changes.

The existing `crates/agentzero-config-ui/` stays as-is. The new UI is the platform-wide control panel built from scratch against the main gateway's REST API.

**Branch:** `feat/platform-ui`

---

## Tech Stack

| Concern | Choice | Reason |
|---|---|---|
| Framework | React 19 + TypeScript | Tauri standard, large ecosystem |
| Build | Vite 6 | Tauri standard, fast HMR |
| Routing | TanStack Router v1 (file-based) | Type-safe, code-splitting |
| Data fetching | TanStack Query v5 | Caching, mutation, invalidation |
| Global state | Zustand v5 | Token + settings only, swappable for Tauri store later |
| UI primitives | shadcn/ui + Tailwind v4 | Composable, dark mode, developer aesthetic |
| Charts | Recharts v2 | Cost/metrics graphs |
| Icons | Lucide React | Consistent, tree-shaken |

Dark mode first. `@import "tailwindcss"` in `index.css` with `:root { color-scheme: dark; }`.

---

## Directory Structure

```
ui/
├── package.json
├── vite.config.ts          # Vite proxy to gateway for dev mode
├── tsconfig.json
├── tailwind.config.ts      # v4 CSS-first, minimal config
├── components.json         # shadcn/ui config
├── index.html
└── src/
    ├── main.tsx            # QueryClient + RouterProvider
    ├── router.tsx          # TanStack Router setup
    ├── index.css           # Tailwind v4 import + dark root
    │
    ├── lib/
    │   ├── api/
    │   │   ├── client.ts       # Typed fetch wrapper (auth header, base URL)
    │   │   ├── agents.ts       # /v1/agents CRUD
    │   │   ├── runs.ts         # /v1/runs CRUD
    │   │   ├── chat.ts         # /v1/chat/completions + /ws/chat
    │   │   ├── models.ts       # /v1/models
    │   │   ├── health.ts       # /health endpoints + /metrics
    │   │   ├── events.ts       # /v1/events SSE
    │   │   ├── tools.ts        # /v1/tools (new endpoint)
    │   │   ├── memory.ts       # /v1/memory (new endpoints)
    │   │   ├── cron.ts         # /v1/cron (new endpoints)
    │   │   ├── approvals.ts    # /v1/approvals (new endpoints)
    │   │   └── auth.ts         # /pair
    │   ├── sse.ts              # EventSource factory hook
    │   └── ws.ts               # WebSocket factory hook
    │
    ├── store/
    │   ├── authStore.ts        # { token, baseUrl } — persisted to localStorage
    │   └── settingsStore.ts    # { theme, sidebarCollapsed }
    │
    ├── hooks/
    │   ├── useGlobalEvents.ts  # /v1/events SSE → invalidates TanStack Query cache
    │   ├── useRunStream.ts     # /v1/runs/:id/stream SSE
    │   └── useChat.ts          # /ws/chat WebSocket with SSE fallback
    │
    ├── components/
    │   ├── layout/
    │   │   ├── Shell.tsx       # Root layout: sidebar + topbar + <Outlet>
    │   │   ├── Sidebar.tsx     # 12 nav links + estop button (collapsible)
    │   │   └── Topbar.tsx      # Connection badge, health indicator, theme toggle
    │   ├── ui/                 # shadcn/ui generated components
    │   └── shared/
    │       ├── StatusBadge.tsx
    │       ├── JsonViewer.tsx
    │       ├── ConfirmDialog.tsx
    │       └── CostDisplay.tsx
    │
    └── routes/
        ├── __root.tsx          # Shell layout + auth guard
        ├── index.tsx           # Redirect → /dashboard
        ├── login.tsx           # Token/pairing entry
        ├── dashboard/index.tsx
        ├── chat/index.tsx
        ├── agents/
        │   ├── index.tsx       # Agent table
        │   └── $agentId.tsx    # Agent detail/edit
        ├── runs/
        │   ├── index.tsx       # Runs table
        │   └── $runId.tsx      # Run detail (transcript + stream + events)
        ├── tools/index.tsx
        ├── channels/index.tsx
        ├── models/index.tsx
        ├── config/index.tsx
        ├── memory/index.tsx
        ├── schedule/index.tsx
        ├── approvals/index.tsx
        └── events/index.tsx
```

---

## Pages

### Dashboard
- Data: `GET /health`, `GET /v1/agents`, `GET /v1/runs?status=running`
- Cards: health status, agent count, active runs, cost summary
- Quick actions: New Chat, New Run, E-Stop (confirm dialog → `POST /v1/estop`)
- Recent runs table (last 5)
- Live: `useGlobalEvents()` invalidates runs/agents cache on events

### Chat
- WebSocket `/ws/chat` with SSE fallback (`POST /v1/chat/completions?stream=true`)
- Model selector (`GET /v1/models`), agent selector (`GET /v1/agents`)
- Streaming token-by-token response display

### Agents
- Table: name, status badge, model, provider, keywords, tools count, source, created_at
- Status toggle → `PATCH /v1/agents/:id`
- Create/edit sheet: name, description, provider, model, system_prompt, keywords (chips), allowed_tools (multiselect)
- Delete → confirm → `DELETE /v1/agents/:id`

### Runs
- Table with status filter (all / pending / running / completed / failed / cancelled)
- Right drawer on row click: result, transcript tab, events tab, live stream tab
- Cancel → `DELETE /v1/runs/:id`; E-Stop → `POST /v1/estop`

### Tools
- Grouped by category (file, web, execution, memory, scheduling, delegation, media, hardware)
- Enable/disable toggles (security policy booleans via `PATCH /v1/config`)
- JSON schema accordion for each tool

### Channels
- Cards for 30+ platforms (hardcoded registry)
- Connection status from SSE events
- Configure sheet → saves via `PATCH /v1/config`
- Webhook URL display

### Models
- Provider-grouped list from `GET /v1/models`
- Set default, refresh cache
- Model routing rules table with add/edit/delete

### Config
- Schema-driven accordion for 25+ TOML sections
- Import/export TOML files
- Data: new `GET /v1/config` + existing config-ui schema bridge

### Memory
- Browse entries (paginated), search, delete
- Recall panel, forget interface

### Schedule
- Cron job CRUD table with schedule expression picker

### Approvals
- Pending queue with approve/deny actions and history

### Events (Live)
- Global SSE stream with topic filter, pause/clear controls, pretty-printed JSON entries

---

## Real-Time Strategy

**`useGlobalEvents()`** — mounted once in `Shell.tsx`:
- `EventSource` to `/v1/events?token=<token>` (EventSource can't set headers)
- Invalidates TanStack Query cache by topic prefix (`job.*` → `['runs']`, `agent.*` → `['agents']`)

**`useRunStream(runId)`** — per-run SSE for the run detail drawer

**`useChat()`** — WebSocket for bidirectional streaming chat; SSE fallback when WS unavailable

**Polling**: health query uses `refetchInterval: 30_000`

---

## Gateway Additions Required

9 new endpoints — all follow existing handler patterns in `crates/agentzero-gateway/src/handlers.rs`:

| Endpoint | Method | Purpose |
|---|---|---|
| `/v1/tools` | GET | List tools with metadata, schema, security gate flags |
| `/v1/memory` | GET | Browse memory (paginated, `?q=` search) |
| `/v1/memory/:id` | DELETE | Delete a memory entry |
| `/v1/memory/recall` | POST | Query memory by text |
| `/v1/memory/forget` | POST | Forget by key/filter |
| `/v1/cron` | GET, POST, PATCH, DELETE | Cron job CRUD |
| `/v1/approvals` | GET | List pending approvals |
| `/v1/approvals/:id/approve` | POST | Approve action |
| `/v1/approvals/:id/deny` | POST | Deny action |
| `/v1/config` | GET, PATCH | Read/update config (dot-path patches) |

Also ensure `/v1/events` accepts `?token=` query param for EventSource compatibility.

---

## Gateway Static Serving

Production: `rust-embed` behind `embedded-ui` Cargo feature.

**`crates/agentzero-gateway/Cargo.toml`:**
```toml
[features]
embedded-ui = ["rust-embed", "mime_guess"]
```

**`crates/agentzero-gateway/src/handlers.rs`** — add `static_handler` (copy pattern from `crates/agentzero-config-ui/src/server.rs`):
- `#[derive(Embed)] #[folder = "../../ui/dist"] struct UiAssets;`
- Serve exact asset paths with immutable cache headers
- Fallback all unmatched paths to `index.html`

**`crates/agentzero-gateway/src/router.rs`** — replace `GET /` → `dashboard` with `.fallback(static_handler)` when feature is active.

**Dev mode**: Vite at port 5173 proxies all `/v1`, `/ws`, `/health`, `/pair`, `/api`, `/metrics` to `http://localhost:8080`.

**`crates/agentzero-gateway/build.rs`** — auto-run `npm run build` when `embedded-ui` enabled and `ui/dist` missing. `cargo:rerun-if-changed=../../ui/src`.

---

## Build Integration (Justfile additions)

```makefile
ui-install:
    cd ui && npm install

ui-build:
    cd ui && npm run build

ui-dev:
    cd ui && npm run dev

build-full: ui-build
    cargo build --release --features embedded-ui
```

---

## Tauri Compatibility

- Zustand `persist` uses `localStorage` now; swap to `@tauri-apps/plugin-store` by changing only the store file
- `authStore.baseUrl` defaults to `""` (same origin); Tauri sets it to `http://127.0.0.1:<dynamic_port>`
- No Node.js APIs in UI source
- `@vitejs/plugin-react` (not swc) for Tauri mobile compatibility

---

## Critical Files to Create/Modify

**New:**
- `ui/` — entire SPA
- `crates/agentzero-gateway/build.rs`

**Modified Rust files:**
- `crates/agentzero-gateway/src/handlers.rs` — 10 new endpoint handlers + `static_handler`
- `crates/agentzero-gateway/src/router.rs` — new routes + `.fallback(static_handler)` behind feature
- `crates/agentzero-gateway/src/models.rs` — new request/response structs
- `crates/agentzero-gateway/Cargo.toml` — `embedded-ui` feature, rust-embed, mime_guess

**Reference (replicate pattern, do not modify):**
- `crates/agentzero-config-ui/src/server.rs` — rust-embed static serving + SPA fallback

---

## Verification

1. `cd ui && npm run build` — zero TypeScript errors, clean Vite build
2. `cargo build --features embedded-ui` — compiles, clippy clean, zero warnings
3. `agentzero gateway` → `http://localhost:8080` → UI loads, no 404s
4. Dashboard: live health, agents, runs cards visible
5. Chat: send message → streaming response via WebSocket
6. Agents: create → appears in table → delete → gone
7. Runs: submit → track to completion → transcript loads
8. Events page: live SSE stream, topic filter works
9. Dev mode: `npm run dev` at 5173, proxy to gateway works
10. `cargo test --workspace` — all existing tests pass
