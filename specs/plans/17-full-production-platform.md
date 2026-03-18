# Plan 10: Full Production Platform (Sprint 39)

## Context

Sprint 38 closed all CRITICAL/HIGH production gaps. Sprint 39 covers every remaining item from the production gap analysis plus strategic platform features requested by the user. This is a large sprint spanning 12 phases — expect to break it into sub-branches or parallel workstreams.

**Key constraint:** No external service dependencies (no Redis, no external message broker). The distributed event bus must be fully embedded.

## Phases & Priority

| Phase | Feature | Priority | Est. Complexity |
|-------|---------|----------|-----------------|
| A | Embedded Distributed Event Bus | HIGH | Large |
| B | Request Body Schema Validation | MEDIUM | Small |
| C | Circuit Breaker Transparent Wiring | MEDIUM | Small |
| D | Liveness Probe | MEDIUM | Tiny |
| E | Turso Migrations | MEDIUM | Small |
| F | Multi-Tenancy Deepening | HIGH | Medium |
| G | AI-Based Tool Selection | HIGH | Medium |
| H | Lightweight Orchestrator Mode | HIGH | Large |
| I | Examples Directory | MEDIUM | Medium |
| J | CI/CD Hardening | MEDIUM | Small |
| K | Fuzzing | LOW | Small |
| L | Operational Runbooks | LOW | Small (docs only) |

## Architecture Decisions

### Distributed Event Bus (Phase A) — No Redis

**Decision:** SQLite WAL + tokio broadcast + TCP gossip mesh.

**Why not Redis:**
- Adds operational complexity (another service to deploy, monitor, secure)
- Contradicts "runs anywhere, even a Raspberry Pi" lightweight story
- SQLite WAL handles concurrent reads with single writer efficiently
- For multi-instance, a thin TCP gossip layer is simpler than a Redis cluster

**Design:**

```
┌─────────────────────────────────────────────┐
│ Node A                                       │
│  ┌──────────┐    ┌───────────────┐          │
│  │ Producer  │───▶│ SqliteEventBus│──WAL──▶ DB│
│  └──────────┘    │  + broadcast  │          │
│                  └───────┬───────┘          │
│                          │ TCP gossip       │
└──────────────────────────┼──────────────────┘
                           │
┌──────────────────────────┼──────────────────┐
│ Node B                   │                   │
│                  ┌───────▼───────┐          │
│                  │ SqliteEventBus│──WAL──▶ DB│
│  ┌──────────┐   │  + broadcast  │          │
│  │ Consumer  │◀──│               │          │
│  └──────────┘   └───────────────┘          │
└─────────────────────────────────────────────┘
```

- Each node has its own SQLite event store
- In-process delivery via `tokio::sync::broadcast` (zero-latency for same-node)
- Cross-node delivery via TCP gossip (bincode frames, LRU dedup)
- Eventual consistency — events may arrive out of order across nodes, but each node's local log is ordered
- No leader election, no consensus — each node is autonomous

### AI Tool Selection (Phase G)

**Decision:** Two-tier selection with caching.

1. **Fast path (keyword):** TF-IDF style matching on tool name + description against the user message. Runs in <1ms. Selects top-K tools (configurable, default 10).
2. **Smart path (AI):** Lightweight LLM call that receives tool names + descriptions and returns a ranked subset. Cached per task-hash for the conversation session. Uses cheapest available model.
3. **Config:** `tool_selection = "all"` (default, backward compat) | `"keyword"` | `"ai"`

### Lightweight Mode (Phase H)

**Decision:** Separate binary target, not a feature flag on the main binary.

- `bin/agentzero-lite/` depends on a minimal crate subset
- Tools are replaced with HTTP stubs that call a full node's `/v1/tool-execute`
- Gateway still runs for API access
- Orchestrator + delegation + event bus all work
- Target: <10 MB release binary on Linux

## Files Most Likely Modified

### Phase A (Event Bus)
- `crates/agentzero-core/src/events.rs` (new) — EventBus trait, Event struct
- `crates/agentzero-core/src/lib.rs` — re-export events module
- `crates/agentzero-storage/src/event_bus/` (new) — SqliteEventBus impl
- `crates/agentzero-infra/src/gossip.rs` (new) — TCP gossip layer
- `crates/agentzero-infra/src/runtime.rs` — wire event bus
- `crates/agentzero-config/src/model.rs` — event bus config fields

### Phase B (Schema Validation)
- `crates/agentzero-gateway/src/models.rs` — typed request structs
- `crates/agentzero-gateway/src/handlers.rs` — replace `Json<Value>` with typed extractors

### Phase C (Circuit Breaker)
- `crates/agentzero-providers/src/transport.rs` — transparent wrapping

### Phase D (Liveness)
- `crates/agentzero-gateway/src/handlers.rs` — `/health/live` handler
- `crates/agentzero-gateway/src/router.rs` — add route

### Phase E (Turso)
- `crates/agentzero-storage/src/memory/turso.rs` — migration versioning

### Phase F (Multi-Tenancy)
- `crates/agentzero-gateway/src/api_keys.rs` — org_id on ApiKey
- `crates/agentzero-infra/src/job_store.rs` — org-scoped queries
- `crates/agentzero-storage/src/memory/pooled.rs` — org-scoped memory
- `crates/agentzero-cli/src/` — auth api-key commands

### Phase G (Tool Selection)
- `crates/agentzero-core/src/tool_selector.rs` (new) — trait + keyword impl
- `crates/agentzero-infra/src/tool_selector_ai.rs` (new) — AI impl
- `crates/agentzero-infra/src/agent.rs` — wire tool selection before provider call

### Phase H (Lightweight Mode)
- `bin/agentzero-lite/` (new) — minimal binary
- `bin/agentzero-lite/Cargo.toml` — minimal dependencies
- `crates/agentzero-gateway/src/handlers.rs` — `/v1/tool-execute` endpoint

### Phase I (Examples)
- `examples/chatbot/` (new)
- `examples/multi-agent-team/` (new)
- `examples/edge-deployment/` (new)
- `examples/research-pipeline/README.md` — update
- `examples/business-office/README.md` — update

### Phase J-K (CI/CD, Fuzzing)
- `.github/workflows/ci.yml` — Trivy, SBOM steps
- `docker-compose.yml` — secrets section
- `fuzz/` (new) — fuzz targets

### Phase L (Runbooks)
- `docs/runbooks/` (new) — 4 markdown files

## Verification

- `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings)
- `cargo test --workspace` (all existing + new tests pass)
- `cargo build --release -p agentzero-lite` (under 10 MB)
- Event bus: 2 instances communicate via gossip on localhost
- Multi-tenancy: org A cannot see org B's jobs/memory
- Tool selection: AI mode reduces tool count in provider call
- Examples: each `examples/*/` has README + working config
- CI: Trivy scan runs, SBOM generated
- Fuzz: targets compile and run for 10s without crash

## Suggested Execution Order

1. **B, C, D** (small, independent — knock out quickly)
2. **A** (large, foundational — enables H and scales the platform)
3. **E, F** (data layer — Turso + multi-tenancy)
4. **G** (AI tool selection — improves agent quality)
5. **H** (lightweight mode — depends on A for event bus)
6. **I** (examples — demonstrates all the above)
7. **J, K** (CI hardening — can run in parallel with anything)
8. **L** (runbooks — last, captures everything built)
