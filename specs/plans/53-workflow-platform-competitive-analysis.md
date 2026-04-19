# Workflow Platform Competitive Analysis

## Context

Analysis of a production open-source Rust workflow automation platform (TM9657, ~600 GitHub stars) that shares significant DNA with AgentZero — both are Rust-based, both use DAG execution, both have WASM plugin systems, and both target AI agent orchestration. The target platform has made several architectural choices that are more mature in specific areas. This analysis identifies concrete patterns we can borrow.

---

## 1. Sink Pattern — Unified Event Ingress (Most Relevant to Your Question)

### What the Target Platform Does

Their `packages/sinks` crate abstracts **all external trigger sources** behind a single `SinkTrait`:

```
SinkTrait
├── register(ctx, registration)      — set up the listener
├── unregister(ctx, registration)    — tear it down
├── handle_trigger(ctx, reg, payload) — fire when event arrives
├── validate_config(config)          — config validation
└── sink_type() → SinkType           — self-identification
```

Every event source — HTTP endpoints, webhooks, cron, MQTT, GitHub webhooks, RSS feeds, Discord bots, file watchers, NFC, geolocation, keyboard shortcuts — is a `SinkTrait` implementor. The `SinkRegistration` struct is the universal "subscription record" that holds:
- Which flow event to trigger (`event_id`, `board_id`, `app_id`)
- Sink-specific config (as `serde_json::Value`)
- Where it runs (`SinkExecution`: Local / Remote / Hybrid)
- Auth tokens, default payloads, cron expressions

**Key insight**: the `Executor` trait decouples sinks from execution — sinks just call `executor.execute_event(app_id, board_id, event_id, payload)`. They don't know or care about how flows run.

### What AgentZero Does Today

Our channel trigger system in `handlers.rs:trigger_workflows_for_channel()` scans all saved workflows looking for matching Channel trigger nodes. This is:
- **Tightly coupled** to the gateway handler
- **Channel-specific** — no abstraction for other trigger sources
- **Missing a registration model** — we scan on every inbound message

Our cron lane in `orchestrator/lanes.rs` exists but isn't connected to workflow Schedule nodes.

### What We Should Borrow

**Create a `TriggerSource` trait** (our version of `SinkTrait`) in `agentzero-core`:

```rust
#[async_trait]
pub trait TriggerSource: Send + Sync {
    fn source_type(&self) -> TriggerType;
    fn validate_config(&self, config: &serde_json::Value) -> Result<()>;
    async fn register(&self, registration: &TriggerRegistration) -> Result<()>;
    async fn unregister(&self, registration: &TriggerRegistration) -> Result<()>;
    async fn handle_event(&self, registration: &TriggerRegistration, payload: Option<Value>) -> Result<TriggerResponse>;
}
```

This would unify: Channel triggers, Schedule/Cron triggers, HTTP webhook triggers, and future sources (file watch, MQTT, etc.) under one abstraction. The `TriggerRegistration` becomes the link between "something happened externally" and "run this workflow."

**Platform availability tagging** (`SinkAvailability: Local | Remote | Both`) is clever — we should adopt this for our embedded vs. gateway deployment modes.

---

## 2. Scheduler Backend Abstraction

### What the Target Platform Does

Their `packages/sinks/src/scheduler/` has a `SchedulerBackend` trait with three implementations:
- `InMemoryScheduler` — for local/Docker Compose (cron parsing + polling)
- `AwsEventBridgeScheduler` — for AWS deployments
- `KubernetesScheduler` — for K8s CronJobs

Feature-gated: `aws = ["dep:aws-sdk-scheduler"]`, `kubernetes = ["dep:kube"]`.

The in-memory scheduler has `get_due_schedules()` + `mark_triggered()` + `sync_schedules()` — a clean polling model.

### What We Should Borrow

Our cron lane already exists but lacks this backend abstraction. We should:
1. Extract a `SchedulerBackend` trait from our existing cron lane
2. Connect it to workflow `Schedule` nodes via `TriggerRegistration`
3. Start with `InMemoryScheduler` (we already have the cron infra)

---

## 3. Compilation-as-a-Service for WASM Nodes

### What the Target Platform Does

Their `packages/compiler` is a **standalone WASM compilation service**:
- Receives compilation jobs with presigned download/upload URLs
- Downloads `.wasm` → verifies blake3 hash → precompiles per target platform → uploads `.cwasm`
- Extracts node definitions by instantiating WASM and calling `get_nodes()`
- JWT-authenticated callback to report results
- Parallel target compilation with configurable parallelism limits

### What We Should Borrow

Our WASM plugin system loads and instantiates at runtime. For a catalog/marketplace model, we'd want:
- **AOT compilation**: precompile WASM to native for each target (wasmtime `precompile()`)
- **Node discovery from WASM**: their pattern of instantiating WASM just to extract node metadata is smart — we could do this for our `az_tool_name()` ABI

---

## 4. Catalog Builder Pattern

### What the Target Platform Does

Their catalog system has a `CatalogBuilder`:
```rust
CatalogBuilder::new()
    .exclude_packages(&[CatalogPackage::Onnx])
    .only_nodes(&["control_branch", "bool_or"])
    .with_custom_nodes(my_custom_nodes())
    .build();
```

Nodes are grouped into domain sub-crates (`catalog-core`, `catalog-data`, `catalog-web`, `catalog-llm`, `catalog-ml`, `catalog-automation`). Heavy deps are feature-gated with `--features execute`.

### What We Should Borrow

Our `default_tools()` in `agentzero-infra` is a flat list. A builder pattern would let us:
- Exclude tool categories for embedded/lightweight deployments (the size reduction project)
- Feature-gate heavy tools (browser automation, ML) behind cargo features
- Let WASM plugins register into the same catalog

---

## 5. Batched Event Callback System

### What the Target Platform Does

Their executor uses a `BufferedInterComHandler` + `run_callback_batcher`:
- Events are sent via `mpsc::UnboundedSender`
- A background task batches them by time interval OR max batch size
- Batches are POSTed to a callback URL with retry logic
- Event types: `Log`, `Progress`, `Output`, `Error`, `Chunk`, `NodeStart`, `NodeEnd`

### What We Should Borrow

Our `StatusUpdate` channel via `mpsc::Sender` is similar but simpler. Their batching pattern would be valuable for:
- Reducing HTTP overhead in our SSE streaming (`/workflows/runs/:run_id/stream`)
- Adding `NodeStart`/`NodeEnd` events for better observability
- The `BufferedInterComHandler` pattern (buffer N events, flush on interval or threshold) is reusable

---

## 6. Feature Gating for Heavy Dependencies

### What the Target Platform Does

Every catalog node with heavy deps uses:
```rust
#[cfg(feature = "execute")]
async fn run(&self, context: &mut ExecutionContext) -> Result<()> { /* real impl */ }

#[cfg(not(feature = "execute"))]
async fn run(&self, _context: &mut ExecutionContext) -> Result<()> {
    Err(anyhow!("Requires 'execute' feature"))
}
```

`get_node()` (metadata) is never gated — only `run()` (execution).

### What We Should Borrow

This directly supports our embedded binary size reduction project. We could gate heavy tool implementations behind features while keeping metadata/registration lightweight.

---

## 7. Platform-Aware Execution

### What the Target Platform Does

- `SinkExecution: Local | Remote | Hybrid` — controls where sinks run
- `SinkAvailability` — declares what's possible per platform
- Desktop-only nodes gated with `#[cfg(not(any(target_os = "ios", target_os = "android")))]`
- Nodes declare quality scores: `privacy`, `security`, `performance`, `governance`, `reliability`, `cost` (0-10)

### What We Should Borrow

The quality score system on nodes is interesting for our security/capability model. We already have `ToolSecurityPolicy` but adding per-tool `privacy_score` and `cost_score` could feed into our autonomy/approval system.

---

## Summary: Priority Borrowing Order

| Priority | Pattern | Effort | Impact | Files Affected |
|----------|---------|--------|--------|----------------|
| **1** | Sink/TriggerSource trait | Medium | High | `core/`, `infra/`, `gateway/` |
| **2** | Scheduler backend abstraction | Low | Medium | `orchestrator/`, `tools/` |
| **3** | Feature-gated heavy tools | Low | High | `tools/Cargo.toml`, tool impls |
| **4** | Catalog builder pattern | Medium | Medium | `infra/src/tools/mod.rs` |
| **5** | Batched event callbacks | Low | Medium | `gateway/` |
| **6** | WASM AOT compilation | High | Medium | `plugins/` |
| **7** | Node quality scores | Low | Low | `core/` |

---

## Specifically for the Event Bus Question

The sink pattern is exactly the right model for an event bus. The key architectural decisions:

1. **Sinks are passive subscribers** — they don't poll, they register and wait to be triggered
2. **The `Executor` trait is the only coupling point** — sinks call `execute_event()`, never touch flow internals
3. **Registration is persistent** — `SinkRegistration` is stored in DB, survives restarts
4. **Config is typed but flexible** — each sink type has its own config struct, stored as `serde_json::Value` in the registration
5. **Platform routing** — `Local | Remote | Hybrid` decides where the handler runs, which is directly applicable to our embedded vs. gateway split

We could implement this as a new `agentzero-triggers` crate (or extend `agentzero-core` with the trait) and migrate our existing channel trigger scanning + cron lane into it.

---

## 8. Multi-Backend Deployment Architecture

### What the Target Platform Does

Their `apps/backend/` has **four complete deployment targets**, each with its own binary/config:

```
apps/backend/
├── aws/           — AWS-native: Lambda executors, ECS compiler, EventBridge scheduler, SQS queues
│   ├── api/
│   ├── compiler-ecs/
│   ├── compiler-lambda/
│   ├── event-bridge/
│   ├── executor/
│   ├── executor-async/
│   ├── executor-ecs/
│   ├── file-tracker/
│   └── media-transformer/
├── kubernetes/    — K8s: Helm charts, CronJob scheduler, sink-trigger service
│   ├── api/
│   ├── compiler/
│   ├── executor/
│   ├── helm/
│   ├── migration/
│   ├── sink-trigger/
│   └── web/
├── docker-compose/ — Self-hosted: docker-compose.yml with monitoring, sink-services
│   ├── api/
│   ├── compiler/
│   ├── monitoring/
│   ├── runtime/
│   ├── sink-services/
│   └── web/
└── local/         — Desktop: embedded everything, no external deps
    ├── api/
    └── runtime/
```

**Key insight**: The executor is a **separate binary** from the API. The API dispatches work via `DispatchPayload` (a canonical wire format in `packages/types/src/dispatch.rs`). This means:
- AWS can use Lambda for burst execution
- K8s can use Jobs for isolation
- Docker Compose runs them co-located
- Local embeds everything

The `DispatchPayload` / `DispatchPayloadRef` pattern is particularly clever — when the payload exceeds queue size limits (~256KB for SQS), it auto-stages to object storage and sends a `Remote { remote_url }` reference instead.

### What We Should Borrow

Our gateway embeds everything. For production deployments, separating the executor into its own binary with a `DispatchPayload` wire format would let us:
- Run workflow execution in isolated processes/containers
- Scale executors independently of the API
- Support Lambda-style burst scaling

---

## 9. Secrets Management Abstraction

### What the Target Platform Does

Their `packages/secrets` crate has a `SecretStore` with pluggable provider backends:

- **Providers**: AWS Secrets Manager, AWS Parameter Store, Azure Key Vault, GCP Secret Manager, Environment variables, File-based
- **Caching**: LRU cache with TTL, negative cache for misses, configurable capacity
- **Resolution**: Providers have a priority order; `SecretRef` can target a specific provider or fall through all
- **Security**: Uses the `secrecy` crate (`SecretString`, `SecretBox`) — values are zeroized on drop
- **Env override**: Any secret can be overridden by env var (for local dev)
- Retry with exponential backoff (3 retries, 100ms initial)

```rust
let store = SecretStore::new(config)?;
let value = store.get_secret(&SecretRef::new("database/password")).await?;
```

### What We Should Borrow

Our auth crate handles API keys and OAuth but doesn't have a unified secret resolution layer. The `SecretRef` pattern (a typed reference that resolves to a value through multiple backends) is cleaner than scattering env var reads. The `secrecy` crate integration for zeroize-on-drop is important for our PII safety goal.

---

## 10. Rich Human-in-the-Loop Interaction System

### What the Target Platform Does

Their `packages/types/src/interaction.rs` defines a full **structured interaction model**:

```rust
enum InteractionType {
    SingleChoice { options, allow_freeform },
    MultipleChoice { options, min/max_selections },
    Form { schema, fields },
}
```

With:
- `InteractionRequest` — sent from a running flow to the UI (via SSE)
- `InteractionResponse` — sent back from the user
- `InteractionPollResult` — `Pending | Responded | Expired | Cancelled`
- TTL-based expiry with automatic cleanup
- **Remote interactions via SSE**: `create_remote_interaction_stream()` opens an SSE connection, receives a `responder_jwt`, and waits for the user to respond — all streaming

### What We Should Borrow

Our Gate node has binary approved/denied. Their system supports:
- **Structured responses** — not just yes/no, but forms, selections, freeform text
- **JWT-scoped response tokens** — the responder_jwt limits who can respond to what
- **SSE-based waiting** — the executor opens SSE and blocks until response comes back, vs our oneshot channel approach

This would make our Gate nodes dramatically more useful — imagine an approval gate that asks "Which environment?" with options, or a form gate that collects deployment parameters.

---

## 11. InterCom / Buffered Event Bus

### What the Target Platform Does

`packages/types/src/intercom.rs` defines a `BufferedInterComHandler` that's used **everywhere** for inter-component communication:

- Uses `DashMap<String, Vec<InterComEvent>>` for lock-free concurrent buffering by event type
- Background task flushes on interval (default 20ms) OR when buffer hits capacity (default 200 events)
- `Weak<Self>` reference in the background task — auto-stops when handler is dropped
- Converts to `InterComCallback` for node-level event reporting
- Thread-safe, Clone-able, Drop-safe (flushes on drop)

**This is the core of their observability** — every node execution streams events through this handler, which batches them for efficient delivery to the API/UI.

### What We Should Borrow

This is directly relevant to our event bus question. The `BufferedInterComHandler` pattern:
1. **DashMap for concurrent lock-free access** — better than our `Mutex<HashMap>` for high-throughput event routing
2. **Event type bucketing** — events grouped by type before flush, so consumers only get what they care about
3. **Auto-flush on drop** — guarantees no event loss at shutdown
4. **Weak reference cleanup** — no leaked background tasks

---

## 12. Typed Pin System (vs Our JSON Ports)

### What the Target Platform Does

Their node connection model uses **strongly typed pins**:

```rust
struct Pin {
    pin_type: PinType,      // Input | Output
    data_type: VariableType, // Int, Float, String, Bool, Struct, Enum, ...
    value_type: ValueType,   // Normal | Array | HashMap | HashSet
    schema: Option<String>,  // JSON Schema for complex types
    options: PinOptions,     // valid_values, range, step, sensitive, enforce_schema
    default_value: Option<Vec<u8>>,
    connected_to: BTreeSet<String>,
}
```

Connections are validated at **compile time** — you can't connect an Int output to a String input. The UI shows type mismatches before execution.

### What We Should Borrow

Our workflow connections are untyped (everything is JSON). Their pin system prevents a class of runtime errors. For our workflow builder, adding type metadata to ports would:
- Catch invalid connections in the UI
- Enable automatic type coercion (Int → Float)
- Show meaningful tooltips ("expects JSON array of strings")

---

## 13. WASM SDK for 16 Languages

### What the Target Platform Does

They have WASM SDKs for **16 programming languages**:
- **Component Model** (wasip2): Rust, Go, C++, Zig, C#, Swift, Python, TypeScript
- **Core Module** (alloc/dealloc ABI): AssemblyScript, Kotlin, Nim, Lua, Java, Grain, MoonBit

Each SDK provides host API bindings for: log, pins, vars, cache, meta, stream, storage, models, http, auth.

The WASM capability matrix tracks per-language feature parity.

### What We Should Borrow

Our WASM plugin SDK supports only Rust with a minimal ABI (`az_alloc`, `az_tool_name`, `az_tool_execute`). Their approach of:
- **WIT (WebAssembly Interface Types)** for Component Model languages
- **Host function imports** for core module languages
- Comprehensive host API surface (not just execute, but storage, HTTP, models, etc.)

...would make our plugin ecosystem much more accessible. Starting with a TypeScript SDK (via `componentize-js`) would have the highest impact.

---

## 14. Canary Deployments for Events

### What the Target Platform Does

Their `Event` struct has an optional `CanaryEvent`:
```rust
struct CanaryEvent {
    weight: f32,              // traffic percentage (0.0-1.0)
    variables: HashMap<String, Variable>,
    board_id: String,
    board_version: Option<(u32, u32, u32)>,
    node_id: String,
}
```

When an event is triggered, a percentage of traffic routes to the canary version with different variables/board version.

### What We Should Borrow

This is a production deployment pattern we don't have. For workflow rollouts:
- Test a new workflow version on 10% of traffic
- Gradually increase as confidence grows
- Roll back instantly by removing the canary

---

## 15. Embedded App (Edge Deployment)

### What the Target Platform Does

`apps/embedded/` is a **Cloudflare Workers / Wrangler** app — a lightweight edge runtime that can run flows at the network edge. Combined with their `only_offline` flag on nodes and `SinkAvailability::Local`, they have a clear story for what runs where.

### What We Should Borrow

Our embedded binary targets resource-constrained devices but lacks the edge story. The `only_offline` pattern on nodes (marking compute-intensive/hardware nodes as local-only) is directly applicable to our tool capability filtering.

---

## 16. Object Store Abstraction for Everything

### What the Target Platform Does

Their `PlatformStores` breaks storage into 6 purpose-specific stores:
```rust
struct PlatformStores {
    bits_store: Option<PlatformStore>,      // binary artifacts
    user_store: Option<PlatformStore>,      // per-user data
    app_storage_store: Option<PlatformStore>, // app content
    app_meta_store: Option<PlatformStore>,   // app metadata
    temporary_store: Option<PlatformStore>,  // ephemeral/cache
    log_store: Option<PlatformStore>,        // execution logs
}
```

Each `PlatformStore` is backed by object_store (S3/R2/GCS/Azure/local filesystem). LanceDB is used for vector-searchable logs.

### What We Should Borrow

Our storage crate has encrypted KV + SQLite. Their separation of concerns (content vs. metadata vs. logs vs. temp) is cleaner for multi-cloud deployments. The LanceDB integration for searchable execution logs is interesting for our observability story.

---

---

## 16. A2UI (Agent-to-UI) — Dynamic UI Generation from Flows

### What the Target Platform Does

Their `packages/core/src/a2ui/` is a **declarative UI component system** with 60+ component types:
- Layout: Row, Column, Grid
- Interactive: TextField, Select, Button
- Display: Image, Table, Chart (Plotly, Nivo)
- Advanced: Scene3d, Canvas2d, Dialogue, GeoMap
- Game: HealthBar, MiniMap, CharacterPortrait, InventoryGrid

Nodes can dynamically generate UI by outputting A2UI component trees. A dedicated copilot generates UI from natural language.

### What We Should Borrow

Our workflow builder has a fixed node palette. The A2UI pattern of "flows that build UI" is a different paradigm — flows aren't just data pipelines, they're full-stack apps. This connects to our planned workflow builder evolution.

---

## 17. Board Versioning + Command Pattern

### What the Target Platform Does

Boards (their workflows) have:
- **Semantic versioning**: Major.Minor.Patch
- **Execution stages**: Dev → Int → QA → PreProd → Prod
- **Execution modes**: Hybrid / Remote / Local
- **Undo/redo** via a full command pattern (`commands.rs`)
- **Node hashing** (HighwayHash) for change detection
- **Layers**: Function, Macro, Collapsed — hierarchical organization

### What We Should Borrow

Our workflows have no versioning or staging. Their approach lets you:
- Promote a workflow through environments (Dev → Prod)
- Roll back to a previous version
- Track changes at the node level via content hashing

---

## 18. Code Interpreter (WASM Python Sandbox)

### What the Target Platform Does

`libs/nodes/code-interpreter/` bundles a Python interpreter (Pyodide) as WASM:
- Zero-cold-start via AOT compilation of python.wasm
- Feature-gated: `bundled-python` for embedded builds
- Compatible with n8n and Dify code node formats

### What We Should Borrow

We have no code execution sandbox. A WASM-sandboxed Python interpreter would let workflows run user-authored scripts safely.

---

## 19. Monitoring Stack (Prometheus + Grafana + Tempo)

### What the Target Platform Does

`apps/backend/docker-compose/monitoring/` includes a complete observability stack:
- **Prometheus** — metrics collection
- **Grafana** — dashboards
- **Tempo** — distributed tracing
- Redis for job queue state, PostgreSQL for persistence

### What We Should Borrow

We have tracing via the `tracing` crate but no metrics collection or dashboards. For production workflows, Prometheus metrics on execution counts/latency/error rates are table stakes.

---

## 20. Content-Addressed Blob Offloading (Dexie-Tauri Adapter)

### What the Target Platform Does

`packages/dexie-tauri-adapter/` transparently offloads large values from IndexedDB to native filesystem:
- Blake3 content-addressed hashing with HMAC verification
- Only small references (hash + MAC) stored in IndexedDB
- Solves IndexedDB size limits for desktop apps

### What We Should Borrow

Interesting pattern for our Tauri/desktop UI if we ever hit storage limits. The content-addressed approach with integrity verification is solid.

---

## AgentZero: Disconnected Implementations Found

**This is critical** — we have several complete subsystems that are built but not wired into the runtime:

### ORPHANED: LaneManager (Cron Lane)
**Location**: `crates/agentzero-orchestrator/src/lanes.rs`
- Complete 3-lane work queuing (Main/Cron/SubAgent) with concurrency control
- `cron_rx` receiver is **never consumed** in production
- Only instantiated in unit tests

### ORPHANED: TriggerEngine (Autopilot)
**Location**: `crates/agentzero-autopilot/src/trigger.rs`
- Well-designed trigger evaluation: EventMatch, Cron, MetricThreshold conditions
- Cooldown tracking, rule toggling
- **Never instantiated at runtime** — no code loads trigger rules or calls `engine.evaluate()`
- The Cron condition explicitly says "handled by cron scheduler" — but that scheduler doesn't exist

### ORPHANED: Autopilot Mission/Proposal System
**Location**: `crates/agentzero-autopilot/src/store.rs`, `turso_store.rs`, `tools/`
- Full SQLite + Turso store implementations
- Tools for proposal creation, voting, mission status
- **No mission executor**, no proposal approval loop, no scheduler to advance states
- Never instantiated by coordinator

### ORPHANED: WASM Cron Plugins
**Location**: `plugins/agentzero-plugin-cron/`
- Two WASM plugins (schedule parser, cron manager)
- **Duplicate** the core schedule.rs logic
- Use separate storage namespace (`.agentzero/plugin-cron-tasks.json` vs `cron-tasks.json`)

### FRAGMENTED: Schedule vs Cron Tools
- `schedule.rs` — unified tool with natural language support
- `cron_tools.rs` — individual CRUD tools
- Both wrap `CronStore` but with different interfaces
- Natural language parsing duplicated in plugin too

### THE GAP: No Cron Execution Loop
**All scheduling code stores tasks but nothing fires them.** The missing piece is a loop that:
1. Reads `cron-tasks.json`
2. Checks which tasks are due
3. Executes them
4. Publishes results to event bus

### FULLY OPERATIONAL: Event Bus
The event bus (core + storage) is the one system that's fully wired:
- `InMemoryBus`, `SqliteEventBus`, `FileBackedBus`
- Agents, channels, router, coordinator all connected
- Privacy boundaries enforced

**Everything else (cron, triggers, missions, lanes) is designed to be event-driven but never subscribes to the bus.**

---

## Updated Summary: All Borrowable Patterns

| # | Pattern | Effort | Impact | Relevance |
|---|---------|--------|--------|-----------|
| **1** | Sink/TriggerSource trait (event bus) | Medium | **High** | Core workflow feature |
| **2** | BufferedInterComHandler (DashMap event bus) | Low | **High** | Directly requested |
| **3** | Feature-gated heavy tools | Low | **High** | Binary size reduction |
| **4** | Structured human-in-the-loop interactions | Medium | **High** | Gate node upgrade |
| **5** | Scheduler backend abstraction | Low | Medium | Schedule node completion |
| **6** | Catalog builder pattern | Medium | Medium | Tool management |
| **7** | Multi-backend deployment (DispatchPayload) | High | Medium | Production scaling |
| **8** | Secrets management abstraction | Medium | Medium | Security posture |
| **9** | Typed pin system | High | Medium | Workflow correctness |
| **10** | WASM AOT compilation | High | Medium | Plugin performance |
| **11** | WASM SDK multi-language | High | Medium | Plugin ecosystem |
| **12** | Board versioning + execution stages | Medium | Medium | Workflow lifecycle |
| **13** | A2UI (dynamic UI from flows) | High | Medium | Full-stack workflows |
| **14** | Code interpreter (WASM Python) | Medium | Medium | Script execution |
| **15** | Monitoring stack (Prometheus/Grafana/Tempo) | Medium | Medium | Production observability |
| **16** | Node quality scores | Low | Low | Security/autonomy |
| **17** | Canary event deployments | Medium | Low | Production workflows |
| **18** | Object store separation | Medium | Low | Multi-cloud readiness |
| **19** | Content-addressed blob offloading | Low | Low | Desktop storage |
| **20** | Edge/embedded app pattern | Low | Low | Already in progress |

---

## 21. WASM Sandbox: Bitflag Capabilities + Resource Limits

### What the Target Platform Does

Their `packages/wasm/src/limits.rs` defines a comprehensive WASM security model:

**Resource Limits** (`WasmLimits`):
- Memory: 16MB (restrictive) → 256MB (permissive)
- Timeout: 10s → 300s
- Fuel: 1B instructions (~1s) → 100B (~100s)
- Stack depth, table count, memory count, instance count limits

**Capabilities** (`WasmCapabilities` — bitflags):
```
STORAGE_READ | STORAGE_WRITE | STORAGE_DELETE
HTTP_GET | HTTP_WRITE | WEBSOCKET | TCP | UDP | DNS
VARIABLES_READ | VARIABLES_WRITE
CACHE_READ | CACHE_WRITE
OAUTH | TOKEN | STREAMING | A2UI | MODELS | FUNCTIONS
```

Compound presets: `STANDARD` (read-only storage + HTTP GET + cache), `ALL` (everything), `NONE` (sandboxed).

**Critical pattern**: `WasmSecurityConfig::from_node_permissions()` maps node-declared `NodePermission` enum values to bitflag capabilities — the bridge between "what the node says it needs" and "what the sandbox allows."

### What We Should Borrow

Our WASM capability filtering (Sprint 50) uses `ToolSecurityPolicy` booleans. Their `bitflags` approach is more expressive and composable. The tiered presets (`restrictive` / `standard` / `permissive`) and the `from_node_permissions()` bridge pattern are worth adopting.

---

## 22. WASM Host Function Surface

### What the Target Platform Does

Their `packages/wasm/src/host_functions/` exposes 13 host function modules to WASM:

| Module | Functions |
|--------|-----------|
| `pins.rs` | Read/write typed pin values |
| `variables.rs` | Get/set flow variables |
| `cache.rs` | Key-value cache operations |
| `storage.rs` | Object store read/write/delete/list |
| `http.rs` | HTTP requests (proxied through host) |
| `websocket.rs` | WebSocket connections |
| `logging.rs` | Structured logging |
| `metadata.rs` | Node/execution metadata |
| `streaming.rs` | Stream chunks to client |
| `auth.rs` | OAuth token access |
| `schema.rs` | JSON schema validation |
| `linker.rs` | WIT/WASI linker setup |

Each function checks `WasmCapabilities` before executing — e.g., `storage_read` checks `caps.has(STORAGE_READ)`.

### What We Should Borrow

Our WASM ABI is minimal (`az_alloc`, `az_tool_name`, `az_tool_execute`). Their host function surface gives plugins real power while maintaining security boundaries. The capability-checked host functions pattern is the right model for expanding our plugin API.

---

## 23. Browser Automation + RPA Nodes

### What the Target Platform Does

`packages/catalog/automation/` has two major modules:

**Browser** (13 files): auth, capture, context, extract, files, input, interact, navigation, observe, page, snapshot, storage, wait — full Playwright-style browser control as flow nodes.

**RPA** (14 files): act, assert, checkpoint, diagnose, error_handler, locate, log, metrics, retry, session, snapshot, timeout, wait_for — desktop automation with retry/checkpoint/error recovery.

Both are `#[cfg(not(target_os = "ios"))]` gated — desktop/server only.

### What We Should Borrow

We don't have browser automation nodes. Their decomposition into atomic operations (navigate, click, extract, wait, assert) with built-in retry/checkpoint is the right granularity for workflow nodes.

---

## 24. Multi-Cloud Credentials Abstraction

### What the Target Platform Does

`packages/core/src/credentials.rs` defines a `SharedCredentialsTrait` with implementations for:
- `AwsSharedCredentials` — S3 + DynamoDB + SQS
- `AzureSharedCredentials` — Blob Storage + Cosmos DB
- `GcpSharedCredentials` — GCS + Firestore
- `MixedSharedCredentials` — Different providers for different store types

Each can produce:
- `PlatformStore` (object store) ��� for content, meta, or logs
- `ConnectBuilder` (LanceDB) — for vector databases
- `LogsDbBuilder` — for execution logs

### What We Should Borrow

Our credential system is focused on API keys and OAuth. Their approach of credentials that can produce typed stores (content vs. meta vs. logs) is cleaner for multi-deployment scenarios.

---

## 25. 20+ LLM Provider Integrations via Rig

### What the Target Platform Does

`packages/model-provider/` wraps the `rig` crate with 20+ provider backends:
OpenAI, Anthropic, Gemini, Cohere, Perplexity, Groq, Together, OpenRouter, DeepSeek, Mistral, VoyageAI, Ollama, Hyperbolic, Moonshot, Galadriel, Mira, Mozilla, XAI, LlamaCpp, LMStudio

Plus embedding models with proxy/remote execution support, text splitters (markdown/character/tokenizer/tiktoken), and a `ModelLogic` trait for swap-in/swap-out model backends.

### What We Should Borrow

We use the `rig` crate too (via our providers crate). Their `ModelProviderConfiguration` struct that holds all provider configs in one place, with env-var-based initialization, is cleaner than our current per-provider setup.

---

## 26. Hub Architecture (App Marketplace)

### What the Target Platform Does

`packages/core/src/hub.rs` defines a `Hub` ��� a central registry where:
- Apps are published with categories, visibility (Public/Private/Prototype/Offline), execution modes
- Models ("bits") are registered with search capabilities
- Mail, push notifications, alerting configured at the hub level
- OAuth provider configurations stored centrally

The Hub is the "app store" — users browse, install, and configure flows as apps.

### What We Should Borrow

We don't have an app/workflow marketplace concept. For shareable workflows (recipes, templates), a Hub-like registry with visibility/execution mode controls is the right model.

---

## 27. Protobuf Serialization (Performance)

### What the Target Platform Does

`packages/core/src/protobuf/` has protobuf definitions for all core types:
- Boards, nodes, pins, variables, events, A2UI components, app configs
- Used alongside JSON — protobuf for wire format, JSON for storage/human readability
- Generated via `prost` with `ToProto`/`FromProto` traits

### What We Should Borrow

We use JSON everywhere. For high-throughput scenarios (execution events, SSE streaming), protobuf would reduce serialization overhead. Their dual-format approach (protobuf on wire, JSON at rest) is pragmatic.

---

## 28. Execution Context: Per-Node Storage Scoping

### What the Target Platform Does

`ExecutionContextCache` provides isolated storage paths per node:
```rust
get_user_dir(node: bool)  → users/{sub}/apps/{app_id}[/{node_id}]
get_cache(node, user)     → tmp/{global|user/{sub}}/apps/{app_id}[/{node_id}]
get_storage(node)         �� {board_dir}/storage[/{node_id}]
```

Each node gets its own storage namespace — can't accidentally clobber another node's data.

### What We Should Borrow

Our tools share a flat workspace. Per-node storage scoping prevents data interference between workflow nodes and enables node-level cleanup.

---

## 29. User Execution Context (Runtime Identity)

### What the Target Platform Does

`UserExecutionContext` carries identity through execution:
- `sub` (user subject from OIDC), `role` (with bitfield permissions), `is_technical_user`, `key_id`
- `RoleContext` with `has_permission()`, custom key-value attributes
- Special contexts: `offline()` for local, `technical()` for API keys

### What We Should Borrow

Our security model has capabilities on tools but doesn't carry user identity through execution. Their `UserExecutionContext` pattern is needed for our Gate/approval nodes — "who approved this?" needs an identity.

---

## 30. LanceDB for Execution Logs + Vector Search

### What the Target Platform Does

Execution logs are stored in LanceDB (Arrow-based vector DB):
- `StoredLogMessage` serialized via `serde_arrow` into Arrow `RecordBatch`
- Supports vector search, full-text search, hybrid search, and filtered queries
- `LogMeta` tracks the run and flushes to LanceDB on completion
- Token counts, latency, cost tracked per log entry

### What We Should Borrow

Our logs go to tracing/file. LanceDB for execution logs enables "search all executions where token count > X" or "find similar error patterns" — powerful for debugging workflows at scale.

---

## Complete Summary Table

| # | Pattern | Effort | Impact | Category |
|---|---------|--------|--------|----------|
| **1** | Sink/TriggerSource trait | Medium | **High** | Event Bus |
| **2** | BufferedInterComHandler (DashMap) | Low | **High** | Event Bus |
| **3** | Feature-gated heavy tools | Low | **High** | Build |
| **4** | Structured human-in-the-loop | Medium | **High** | Workflow |
| **5** | Scheduler backend abstraction | Low | Medium | Scheduling |
| **6** | Catalog builder pattern | Medium | Medium | Tools |
| **7** | Multi-backend deployment | High | Medium | Infra |
| **8** | Secrets management abstraction | Medium | Medium | Security |
| **9** | Typed pin system | High | Medium | Workflow |
| **10** | WASM AOT compilation | High | Medium | Plugins |
| **11** | WASM SDK multi-language | High | Medium | Plugins |
| **12** | Board versioning + stages | Medium | Medium | Workflow |
| **13** | A2UI (dynamic UI from flows) | High | Medium | UI |
| **14** | Code interpreter (WASM Python) | Medium | Medium | Tools |
| **15** | Monitoring (Prometheus/Grafana) | Medium | Medium | Observability |
| **16** | Node quality scores | Low | Low | Security |
| **17** | Canary event deployments | Medium | Low | Workflow |
| **18** | Object store separation | Medium | Low | Storage |
| **19** | Content-addressed blob offload | Low | Low | Storage |
| **20** | Edge/embedded app pattern | Low | Low | Deployment |
| **21** | WASM bitflag capabilities | Low | Medium | Security |
| **22** | WASM host function surface | Medium | Medium | Plugins |
| **23** | Browser/RPA automation nodes | High | Medium | Tools |
| **24** | Multi-cloud credentials | Medium | Low | Infra |
| **25** | 20+ LLM providers via Rig | Low | Low | Providers |
| **26** | Hub/marketplace architecture | High | Low | Platform |
| **27** | Protobuf wire format | Medium | Low | Performance |
| **28** | Per-node storage scoping | Low | Medium | Security |
| **29** | User execution context | Low | Medium | Security |
| **30** | LanceDB execution logs | Medium | Medium | Observability |
| **31** | Unified copilot (Board + UI) | High | Medium | AI Assist |
| **32** | Board cleanup/compression | Medium | Medium | Workflow |

---

## Action Items: Wiring Up Our Disconnected Code

Before borrowing from the target platform, we should **activate what we already built**. Informed by their patterns:

### Priority 1: Create the Cron Execution Loop
- Wire `CronStore` → polling loop → event bus publish → agent execution
- The target platform's `InMemoryScheduler.get_due_schedules()` + `mark_triggered()` is the exact pattern
- Connect to existing `LaneManager.cron_rx` channel (it's already designed for this)
- Unify `schedule.rs` and `cron_tools.rs` into one interface

### Priority 2: Wire TriggerEngine to Event Bus
- Subscribe `TriggerEngine` to the event bus
- On each event, call `engine.evaluate()` and fire matching rules
- This activates the entire autopilot trigger system we already built
- The target platform's `SinkTrait.handle_trigger()` validates this pattern

### Priority 3: Activate LaneManager in Coordinator
- Instantiate `LaneManager` in the swarm coordinator startup
- Route work to appropriate lanes (main/cron/subagent)
- Consume all three receivers

### Priority 4: Connect Autopilot Mission System
- Implement mission executor loop
- Wire proposal voting to event bus
- This is lower priority but represents significant orphaned work

### Priority 5: Consolidate Duplicates
- Remove WASM cron plugins (duplicate of core)
- Merge schedule.rs + cron_tools.rs
- Single cron task storage namespace

---

## 31. Unified Copilot (Board + Frontend AI Assistant)

### What the Target Platform Does

`packages/core/src/copilot/` implements a multi-scope AI assistant:

- **CopilotScope**: `Board` (modifies flow graph), `Frontend` (generates UI), `Both` (does both)
- Accepts: user prompt, images, chat history, selected nodes/components, run context
- Returns: `BoardCommand[]` (node adds, connection changes, variable updates) + `SurfaceComponent[]` (UI components) + follow-up suggestions
- Streams tokens + scope decisions + plan steps to the client
- Uses `CatalogProvider` trait to query available nodes for context

The copilot doesn't just generate code — it produces structured commands that the board's command pattern executes with undo/redo support.

### What We Should Borrow

We don't have an AI assistant for workflow building. Their approach of structured commands (not free-form edits) is safer and undoable. The multi-scope pattern (flow logic + UI generation from one prompt) is ambitious but shows where workflow copilots are heading.

---

## 32. Board Cleanup + Compression

### What the Target Platform Does

`packages/core/src/flow/board/cleanup.rs` + compression utilities:
- Board state compressed to/from files via `compress_to_file` / `from_compressed`
- Both JSON and Protobuf serialization paths
- Cleanup logic for orphaned pins, broken connections, stale variables
- Events stored compressed with semantic versioning

### What We Should Borrow

Our workflows are stored as-is in JSON. Compression + periodic cleanup (orphaned connections after node deletion, etc.) prevents state rot in long-lived workflows.

---

## Items NOT Borrowable (platform-specific)

These are notable but not relevant to AgentZero's architecture:

- **Prisma ORM** for PostgreSQL schema management (we use SQLite/Turso)
- **Next.js frontend** (we use a different UI stack)
- **Tauri desktop shell** (we have our own)
- **Dexie IndexedDB adapter** (our desktop storage is different)
- **SQS/EventBridge** specifics (platform-level, not architectural)
- **Codacy CI** integration (tool-specific)

---

## Final Coverage Checklist

| Area | Covered | Notes |
|------|---------|-------|
| packages/core | Yes | Flow engine, copilot, A2UI, state, credentials, protobuf |
| packages/sinks | Yes | SinkTrait, scheduler backends, HTTP sink, configs |
| packages/executor | Yes | Execution dispatch, batched callbacks, WASM loading |
| packages/compiler | Yes | AOT compilation, node extraction, parallel targets |
| packages/types | Yes | InterCom, interaction, dispatch payloads |
| packages/storage | Yes | Object store, LanceDB, vector search |
| packages/model-provider | Yes | 20+ LLM providers, embeddings, splitters |
| packages/secrets | Yes | Multi-provider secrets with caching |
| packages/wasm | Yes | Limits, capabilities, host functions, security |
| packages/schema | Yes | JSON schema generation |
| packages/catalog | Yes | Builder pattern, domain sub-crates, feature gating |
| packages/api | Yes | Middleware, utoipa, permission macros |
| packages/bits | Stub | Empty crate (model metadata placeholder) |
| packages/dexie-tauri-adapter | Yes | Blob offloading for desktop |
| packages/catalog-macros | Skipped | Proc macros (implementation detail) |
| packages/catalog-build-helper | Skipped | Build script helpers |
| apps/backend/* | Yes | 4 deployment targets (AWS, K8s, Docker Compose, Local) |
| apps/desktop | Noted | Tauri + Next.js (not borrowable) |
| apps/embedded | Yes | Cloudflare Workers edge deployment |
| apps/web | Noted | Standalone web app |
| libs/wasm-sdk | Yes | 16-language WASM SDKs |
| libs/nodes/code-interpreter | Yes | WASM Python sandbox |
| libs/platform | Yes | Node.js + Python SDKs |
| templates/ | Yes | WASM node templates, capability matrix |
| tests/ | Noted | Docker-compose test infrastructure |
| .github/workflows | Noted | CI: clippy, audit, tests, FOSSA |
| CLAUDE.md | Yes | Project conventions and guidelines |
