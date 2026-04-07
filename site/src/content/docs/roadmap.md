---
title: Roadmap
description: AgentZero development roadmap — completed milestones and future direction.
---

## Completed

### Foundation & Core (Phases 0-4)

- Workspace setup, CI, CLI shell with `onboard`, `agent`, `status` commands
- Core domain types and traits: `Provider`, `MemoryStore`, `Tool`, `Channel`
- OpenAI-compatible provider, SQLite memory, `read_file` and `shell` tools
- Agent loop hardening (max iterations, timeouts, event logging)
- TOML config, env overrides, secret redaction, security defaults

### Runtime Expansion

- Gateway HTTP server (Axum) with pairing auth, rate limiting, CORS
- WASM plugin sandbox with integrity verification
- Channel integrations (Telegram, Discord, Slack)
- FFI bindings (Swift, Kotlin, Python via UniFFI; Node.js via napi-rs)
- 35+ LLM provider support via OpenAI-compatible interface
- Autonomy levels, OTP approval, audit trails
- Hardware discovery, cron scheduling, skills/SOP engine

### Workspace Consolidation (Sprint 20)

- Workspace consolidated from 46 to 16 crates
- Encrypted SQLite with SQLCipher
- Plugin security hardening (path traversal fix, semver, debouncing, file locking)
- Replaced wasmtime with wasmi as default WASM runtime
- Build variant tooling (default, server, minimal)
- 1,400+ tests passing, 0 clippy warnings

### Structured Tool Use (Sprint 21)

- Provider tool definitions (`ToolDefinition`, `ToolUseRequest`, `ToolResultMessage`)
- Structured tool dispatch in agent loop with text-based fallback
- Conversation message history with `Vec<ConversationMessage>`
- Streaming tool use with `ToolCallDelta` and SSE parsing
- JSON Schema validation and `agentzero tools list/info/schema` CLI commands
- All 50+ tools implement `input_schema()`

### Streaming & Agent Wiring (Sprint 22)

- **Streaming agent loop** — `Agent::respond_streaming()` with `StreamSink` / `StreamChunk`
- **Runtime streaming channel** — `run_agent_streaming()` returning receiver + join handle
- **CLI `--stream` flag** — `agentzero agent --stream -m "hello"`
- **System prompt support** — `system_prompt` in AgentConfig, wired through all providers
- **Gateway agent wiring** — Real agent calls on `/api/chat`, `/v1/chat/completions`, `/ws/chat`
- **SSE streaming** — OpenAI-compatible SSE on `/v1/chat/completions?stream=true`
- **WebSocket streaming** — Bidirectional streaming on `/ws/chat`
- **MCP connection caching** — `McpSession` with cached subprocess connections and tool schemas
- **FFI Node.js parity** — `register_tool()`, `send_message_async()`, `registered_tool_names()`

### Hardening & Polish (Sprint 22H)

- JSON schema validation wired into tool dispatch (`prepare_tool_input()`)
- Config validation for `gateway.port`, `gateway.host`, `autonomy.level`, `max_cost_per_day_cents`
- Unsafe `unwrap()` calls replaced with safe alternatives
- `model_supports_tool_use` defaults to `false` (unknown models don't assume tool support)
- Full test coverage: wasm_bridge, parse_hook_mode, gateway TCP integration, full-loop agent with tool calls

### Production Readiness & Observability (Sprint 23)

- Real Prometheus metrics (counters, histograms, gauges) with request metrics middleware
- Dynamic `/v1/models` from provider catalog
- WebSocket hardening (heartbeat ping/pong, idle timeout, binary frame rejection)
- Structured error types (`GatewayError` with 8 variants, JSON error responses)
- Storage test expansion (19 → 46 tests), provider tracing spans, config audit
- Site documentation: gateway docs, architecture docs, threat model, provider guide

### Private AI Production-Readiness (Sprint 24)

- Gateway privacy wiring: NoiseSessionStore, RelayMailbox, key rotation task on startup
- Client-side Noise handshake (`NoiseClientHandshake`, `NoiseClientSession`, `NoiseHttpTransport`)
- `GET /v1/privacy/info` endpoint for capability discovery
- Security hardening: sealed envelope replay protection (nonce dedup, HTTP 409), local provider URL enforcement, network-level tool enforcement, plugin network isolation
- Per-component privacy boundaries (`PrivacyBoundary` enum with `resolve()`, agent/tool/channel boundaries)
- 6 Prometheus privacy metrics, E2E encryption integration tests
- Key rotation lifecycle (`force_rotate()`, `--force` CLI flag, persist on rotate)
- `Serialize` removed from `IdentityKeyPair` (prevent secret key leaks)

### Privacy End-to-End (Sprint 25)

- Memory privacy boundaries: `MemoryEntry` carries `privacy_boundary` and `source_channel`, `recent_for_boundary()` filters by boundary, SQLite schema migrated
- Channel privacy boundaries: `ChannelMessage.privacy_boundary`, `dispatch_with_boundary()` blocks `local_only` → non-local channels, per-channel boundary config
- Noise IK client handshake: 1 round-trip fast reconnect when server key is cached, `auto_noise_handshake()` selects IK vs XX
- `agentzero privacy test` command: 8 diagnostic checks (config, boundaries, memory, envelopes, Noise XX/IK, channels, encrypted store)
- Integration wiring: `ToolContext.privacy_boundary`, leak guard `check_boundary()`, config validation for encrypted-without-noise
- 1,724 tests passing, 0 clippy warnings

### Plugin Registry & Audio Input (Sprint 30)

- **HTTP registry fetch** — `az plugin install --url <https://...>` and `az plugin refresh --registry-url` accept `https://` and `http://` URLs
- **Plugin dependency resolution** — `PluginManifest` gains `dependencies: Vec<PluginDependency>` with semver `version_req`; `az plugin install --registry-url` resolves and installs transitive deps; circular deps detected
- **Audio input** — `[AUDIO:/path]` markers in user messages are transcribed before reaching the LLM via a configurable OpenAI-compatible endpoint (default: Groq Whisper); new `[audio]` config section
- Workspace version bumped to 0.4.0

### Self-Running AI Company — Autopilot Engine (Sprint 44)

- **Autopilot crate** — New `agentzero-autopilot` crate with autonomous company loop: proposals, cap gates, missions, triggers, reaction matrices, stale recovery
- **Core types** — `Proposal`, `Mission`, `MissionStep`, `AutopilotEvent`, `TriggerRule`, `ReactionRule` with status enums, serde, Display impls
- **Cap gates** — Resource constraint enforcement: daily spend, concurrent missions, proposals/hour, missions/agent/day
- **Supabase client** — PostgREST client for proposals, missions, events, content (service_role auth)
- **Trigger engine** — Event-driven + cron-based trigger rules with cooldown enforcement
- **Reaction matrix** — JSON-configurable probabilistic inter-agent interactions with wildcard patterns
- **Stale recovery** — Heartbeat monitoring for stuck missions with configurable threshold
- **Autopilot tools** — `proposal_create`, `proposal_vote`, `mission_status`, `trigger_fire`
- **Supabase schema** — SQL migration with 8 tables, RLS policies, indexes, realtime, helper views
- **Company templates** — Content Agency, Dev Agency, SaaS Product (TOML config + reaction matrix JSON)
- 38 tests, 0 clippy warnings
- Workspace version 0.6.0

### Persistent Agent Management (Sprint 45)

- **`agent_manage` LLM tool** — Agents can create/manage other agents during conversation
- **CLI `agentzero agents`** — Full CRUD subcommands (create, list, get, update, delete, status)
- **Config UI agents panel** — Browser-based persistent agent management with status toggles
- **Coordinator store sync** — Hot-loading newly created agents without restart
- 2,311 tests, 0 clippy warnings

### Platform Control UI (Sprint 46)

- **Full web SPA** at `ui/` — React 19 + TanStack Router/Query + Tailwind v4 + Recharts
- **12 pages** — Dashboard, Chat (WebSocket streaming), Agents CRUD, Runs (with detail panel), Tools, Channels, Models, Config editor, Memory, Schedule (cron CRUD), Approvals, Events (SSE stream viewer)
- **Gateway static serving** — `embedded-ui` feature embeds the SPA via `rust-embed`
- **Playwright e2e tests** covering all pages

### Multi-Agent Dashboard & Observability (Sprint 47)

- **Agent topology graph** — Canvas-based live DAG visualization of agents and delegation links
- **`GET /v1/topology`** — Live agent topology snapshot (nodes + edges)
- **`GET /v1/agents/:id/stats`** — Per-agent metrics (runs, cost, tokens, tool usage frequency)
- **Delegation tree view** — Runs page flat/tree toggle showing parent-child run hierarchy
- **Per-agent cost charts** — Recharts bar chart of tool usage + summary cards
- **Tool call timeline** — Color-coded sequential timeline in run detail panel
- **Regression detection** — `FileModificationTracker` detects when agents modify the same file in a delegation tree; warnings surface via event bus and dashboard banner

### Autonomous Agent Swarms (Sprint 72)

- **Parallel execution** — Ready-queue executor with `tokio::JoinSet` replaces level-based batching
- **Sandboxed isolation** — `WorktreeSandbox` (git worktree per agent), `ContainerSandbox` (Docker), `MicroVmSandbox` (Firecracker)
- **Cross-agent awareness** — `SwarmContext` injects sibling task descriptions and file scopes
- **Dead agent recovery** — `RecoveryMonitor` with heartbeat timeout and automatic re-dispatch
- **Goal decomposition** — `GoalPlanner` types and `SwarmSupervisor` for orchestrating planned workflows
- **CLI + Gateway** — `agentzero swarm "goal"` and `POST /v1/swarm`

### Self-Evolving Agent System (Sprint 73)

- **NL goal decomposition** — `GoalPlanner::plan()` calls LLM with tool catalog, produces multi-agent DAGs with per-node `tool_hints`
- **Dynamic tools** — Runtime-created tools (shell, HTTP, LLM, composite strategies) that persist encrypted across sessions. Export/import for sharing.
- **`tool_create` tool** — LLM-callable tool for creating dynamic tools mid-session from natural language descriptions
- **NL agent definitions** — `create_from_description` action derives name, system prompt, keywords, and allowed tools from plain English
- **Tool catalog learning** — `RecipeStore` records successful tool combos, boosts them on matching future goals via `HintedToolSelector`
- **`ToolSource` trait** — Mid-session tool discovery so newly created tools are visible without restart
- **Persistence** — `.agentzero/dynamic-tools.json`, `.agentzero/agents.json`, `.agentzero/tool-recipes.json` (all encrypted at rest)

### Candle Metal GPU + KV Cache Reuse (Sprints 77–78)

- **Apple Silicon GPU acceleration** — Bumped Candle 0.9 → 0.10, uncommented Metal feature gate, wired auto-detect with CPU fallback
- **KV cache reuse across turns** — Track cached token sequence in `LoadedModel`, skip reprocessing common prompt prefix on subsequent calls (saves 2–4k tokens of recomputation per turn in multi-turn conversations)

### Runtime Enhancements + `#[tool_fn]` + WASM Codegen (Sprints 79–80)

- **Monotonic audit events** — `seq` + `session_id` on every `AuditEvent`, gateway endpoint `GET /v1/runs/:id/events?since_seq=N` for incremental polling
- **Agent-agnostic instruction injection** — `InstructionMethod::{SystemPrompt, ToolDefinition, Custom}` for heterogeneous delegation
- **WASM plugin CLI shim bridge** — Per-execution bearer token, host tool calls via local HTTP, auto-shutdown
- **CoW overlay filesystem** for sandboxed plugin filesystem access
- **`#[tool_fn]` proc macro** — Function-level macro that collapses tool boilerplate from ~60 lines to ~10
- **Codegen dynamic tool strategy** — LLM writes Rust → compile to WASM → hot-load via existing plugin system, no restart required

### Event Bus Production Hardening (Sprint 81)

- **Multi-axis subscriber filtering** — `recv_with_filter(EventFilter { source, topic_prefix })` replaces topic-only filtering
- **Publish delivery feedback** — `publish()` returns `PublishResult { delivered: usize }` instead of `()`
- **`Arc<str>` event payloads** — broadcast fan-out is now a pointer copy, not a String clone
- Deleted 361 lines of dead orchestrator event-bus code; one unified event bus hierarchy across the workspace

### Retrieval Quality Upgrade (Sprint 82)

- **Tantivy BM25 RAG index** — Replaces case-insensitive substring matching with a proper inverted index. `RagQueryMatch` carries a `score: f32` relevance field. Cold-start rebuild from the encrypted JSON store; legacy JSONL still migrates transparently.
- **HNSW vector index for `MemoryStore::semantic_recall`** — Opt-in via `enable_hnsw_index(dir, dim)`. Replaces O(n) cosine scan with approximate nearest neighbor lookup. Mirror writes to disk every 100 inserts; cold-start rebuilds from SQLite when the index is missing.
- **Hybrid retrieval with reciprocal rank fusion** — `MemoryStore::hybrid_recall(query_text, query_embedding, limit)` runs semantic + keyword in parallel and fuses with RRF (k=60). `SemanticRecallTool` exposes `mode: "hybrid"`.
- See [Retrieval & Memory](/architecture/retrieval/) for the full design.

### Device Detection + Compile-Time Feature Guards (Sprint 83 Phases A & B)

- **`agentzero_core::device` capability detection** — `HardwareCapabilities` struct with `GpuType { Metal, Cuda, Vulkan, None }`, `NpuType { CoreML, Nnapi, None }`, thermal state, detection confidence. Cross-platform CPU/memory probe via `sysinfo`. Apple, Linux, and Android probes that don't link against CUDA or Metal at compile time.
- **Wired into Candle backend selection** — `select_device_auto()` consults the capability profile before attempting any GPU init; logs the probe result so you can see what was detected and why.
- **Wired into hardware tool surface** — `discover_boards()` prepends a `live-host` entry built from the live device probe alongside the existing simulator stubs.
- **Compile-time feature guards** — `compile_error!` blocks for `candle-cuda` on macOS, `candle-metal` off-Apple, `candle-cuda` + `candle-metal` simultaneously, `candle` or `local-model` on `wasm32`, and `storage-encrypted` + `storage-plain` simultaneously. Each error includes both the *reason* and the *fix*.

### Production-Readiness Pass — Load Testing (Sprint 84 Phase A)

- **Pure-Rust gateway load harness** — In-process gateway spawn with `--no-auth`, hammers cheap endpoints, reports RPS + p50/p95/p99 latencies. See [Load Testing](/reference/load-testing/) for invocation and baseline numbers (~68k RPS for cheap endpoints on a dev MacBook, with graceful degradation under 8x concurrency contention).

## Planned

### Registry Repo, Audio Streaming & Image Generation (Sprint 31)

- Hosted plugin registry repository with automated PR-based publishing workflow
- Streaming audio transcription for low-latency voice input
- Image generation tool (via OpenAI-compatible `/v1/images/generations` endpoint)
- `[IMAGE:...]` output markers rendered in supported frontends

### Medium-Term

- iOS XCFramework packaging for Swift FFI
- Android AAR packaging for Kotlin FFI
- TUI dashboard enhancement (live runs, agents, events in terminal)
- Lightweight orchestrator binary (sub-10MB edge deployment)

### Long-Term

- Fleet mode with Firecracker microVM isolation
- Multi-node distributed orchestration
- Self-hosted model fine-tuning integration
- Enterprise audit and compliance features

## Work Rules

- Add one capability per PR
- Every feature needs: tests, docs, and one explicit non-goal
- All tools must implement `input_schema()` for structured tool-use compatibility
