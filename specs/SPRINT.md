# AgentZero Sprint Plan

## Sprint 25: Privacy End-to-End

**Goal:** Close privacy enforcement gaps so boundaries are enforced from channel input through memory storage and back. Memory entries carry privacy boundaries, channels propagate them, IK fast-reconnect works client-side, and `az privacy test` validates the full stack.

**Baseline:** 16-crate workspace, 1,431 tests passing, 0 clippy warnings, privacy system production-ready (Noise Protocol, sealed envelopes, per-component boundaries, key rotation).

Previous sprints archived to `specs/sprints/23-24-production-readiness-privacy.md`.

---

### Completed

- [x] **Memory privacy boundaries** — `MemoryEntry` carries `privacy_boundary` and `source_channel` fields. `recent_for_boundary()` filters by boundary. SQLite schema migrated with backward-compatible defaults.
- [x] **Channel privacy boundaries** — `ChannelMessage.privacy_boundary` field. `dispatch_with_boundary()` blocks `local_only` → non-local channels. Per-channel boundary config in TOML. `is_local_channel()` helper (CLI + transcription = local).
- [x] **Noise IK client handshake** — `NoiseClientHandshake::new_ik()` constructor. Gateway `/v1/noise/handshake/ik` endpoint. `auto_noise_handshake()` selects IK when server key cached, XX fallback. `PrivacyInfo.supported_patterns` advertises available patterns.
- [x] **Privacy CLI test command** — `az privacy test [--json]` runs 8 diagnostic checks: config validation, boundary resolution, memory isolation, sealed envelope round-trip, Noise XX/IK handshakes, channel locality, encrypted store round-trip.
- [x] **Integration wiring & hardening** — Runtime populates `ToolContext.privacy_boundary` from config. Agent propagates `source_channel` from `ToolContext` into memory writes and `recent_for_boundary()` queries. Config validation rejects `encrypted` mode without `noise.enabled`. Leak guard `check_boundary()` blocks `local_only` content to non-local channels.

### Acceptance Criteria

- [x] Memory entries tagged with `privacy_boundary` and `source_channel`; `recent_for_boundary()` filters correctly
- [x] Two agents sharing same store with different boundaries see isolated history
- [x] Existing databases seamlessly migrated (new columns with defaults, old JSON deserializes)
- [x] Channel messages carry privacy_boundary; dispatch blocks `local_only` → non-local channels
- [x] IK handshake completes in 1 HTTP round-trip; auto-select based on cached key
- [x] `az privacy test` runs 8 diagnostic checks, reports pass/fail
- [x] Leak guard blocks local_only memory content from non-local channel responses
- [x] Config validation catches encrypted-without-noise and boundary escalation
- [x] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test --workspace`
- [x] Test count: 1,724 total (baseline 1,431 + ~293 new across all phases)

---

### Backlog (candidates for Sprint 26)

- [x] **Remaining unchecked tools** — All 17 tools verified complete: each implements `input_schema()`, has tests, and is registered in `default_tools()`
- [x] **FFI bindings update** — Expose privacy types through UniFFI (Swift/Kotlin) and napi-rs (Node)
- [x] **Benchmarks** — Noise handshake latency, encrypt/decrypt throughput, relay mailbox performance
- [x] **Timing jitter** — Configurable randomized delays on sealed envelope relay submit/poll responses (10–100ms / 20–200ms defaults), config fields in `SealedEnvelopeConfig`, 7 new tests

---

## Sprint 26: Hardening & Polish

**Goal:** Close remaining backlog items from Sprint 25 and harden the relay and privacy stack.

**Baseline:** 16-crate workspace, 1,731+ tests passing, 0 clippy warnings, timing jitter shipped, all tools verified complete.

### Completed   

- [x] **Timing jitter for sealed envelope relay** — `JitterConfig` struct with configurable min/max delays for submit (10–100 ms) and poll (20–200 ms). Config wired through `SealedEnvelopeConfig` → `RelayMailbox::with_jitter()`. 7 new tests. Docs updated (threat model, config reference, privacy guide).
- [x] **Stale backlog cleanup** — Verified all 17 "unchecked" tools are fully implemented with `input_schema()`, tests, and registration.
- [x] **Privacy benchmarks** — Criterion 0.5 benchmarks for Noise keypair generation, XX/IK handshakes, encrypt/decrypt at 64B/1KB/64KB, sealed envelope seal+open, routing ID computation. 11 benchmark functions in `agentzero-bench` behind `privacy` feature flag.
- [x] **FFI privacy bindings** — `PrivacyBoundary`, `PrivacyInfo`, `PrivacyStatus` types exposed through UniFFI (Swift/Kotlin) and napi-rs (Node). Read-only query types for inspecting privacy state from mobile/Node apps. 6 tests.

### Backlog (candidates for Sprint 27)

- [x] **Conversation branching** — Forking and branching conversation histories *(Sprint 29)*
- [x] **Multi-modal input** — Image (not audio) across Anthropic and OpenAI providers *(Sprint 29)*

---

## Sprint 27: Event-Driven Multi-Agent Platform

**Goal:** Transform AgentZero into a full autonomous multi-agent platform where AI agents communicate via an event bus. Agents subscribe to topics, produce outputs that go back on the bus, and other agents react. The orchestrator handles routing, chaining, and channel dispatch.

**Baseline:** 16-crate workspace, 1,731+ tests passing, 0 clippy warnings, privacy stack complete.

**Outcome:** Orchestrator extracted to `agentzero-orchestrator` crate (17 workspace members). All implementation complete; integration tests carried to Sprint 28.

### Phase A: Foundation (always-on, improves single-agent mode)

- [x] **Publishing simplification** — Added `publish = false` to 14 internal crates. Only `agentzero-core` and `agentzero-plugin-sdk` are publishable. Removed `version` from internal path deps in workspace `Cargo.toml`.
- [x] **EventBus trait + InMemoryBus** — `Event` struct with `correlation_id` for chain tracing, `EventBus`/`EventSubscriber` traits, `InMemoryBus` backed by `tokio::sync::broadcast`. Privacy boundary helpers (`is_boundary_compatible`, `topic_matches`). Always-on (not feature-gated).
- [x] **ToolContext bus fields** — Added `event_bus: Option<Arc<dyn EventBus>>` and `agent_id: Option<String>` to `ToolContext` (serde-skipped).
- [x] **IPC rewrite to use bus** — `agents_ipc.rs` uses EventBus pub/sub when available (`ipc.message.{to}` topics), falls back to `EncryptedJsonStore` for backward compatibility. 8 tests (4 file-based + 4 bus-based).

### Phase B: Orchestrator (extracted to `agentzero-orchestrator`)

- [x] **SwarmConfig + AgentDescriptor** — Config model in `agentzero-config` for swarm settings. `AgentDescriptor` with `subscribes_to`/`produces` topics, `privacy_boundary`. Pipeline definitions with trigger (keywords, regex), step timeout, error strategy.
- [x] **AI AgentRouter** — LLM-based message classification via `Provider::complete()`. Keyword fallback when AI unavailable. 5 unit tests.
- [x] **Coordinator** — Three concurrent loops: channel ingestion (publishes to bus), AI router (classifies and dispatches), response/chain handler (chains or dispatches to channel). Shutdown signal via `watch::Receiver<bool>`.
- [x] **Agent worker loop** — Each agent runs in `tokio::spawn`, receives `TaskMessage` via `mpsc`, outputs published on bus per `produces` topics.
- [x] **Response/chain handler** — Subscribes to agent output events. Routes to subscribing agents (chaining) or dispatches to originating channel (terminal detection via `correlation_id`). Privacy boundary check on each chain hop.
- [x] **Pipeline executor** — Sequential pipelines with `ErrorStrategy` (Abort/Skip/Retry). Step timeout with `tokio::time::timeout`. `channel_reply` sends final result to originating channel. 3 unit tests.

### Phase C: Tool Wiring

- [x] **SubAgent tool wiring** — `SubAgentSpawnTool`, `SubAgentListTool`, `SubAgentManageTool` implemented in `agentzero-tools` and registered in `default_tools()`. 4 tests.
- [x] **Runtime integration** — `build_swarm()` in `agentzero-orchestrator` creates `InMemoryBus`, `AgentRouter`, agent workers, and `Coordinator`. Gateway calls `agentzero_orchestrator::build_swarm()`.

### Phase D: Tests & Verification

- [x] **Unit tests** — 29 tests across EventBus (9), AgentRouter (5), Coordinator (3), IPC (8), SubAgent tools (4). Covers pub/sub, filtered recv, topic matching, boundary compatibility, keyword routing, error strategies.
- [x] **Integration tests** — Agent chain (A→B→C→channel), privacy routing, graceful shutdown, error propagation. *(Completed in Sprint 28 Phase A.)*

### Acceptance Criteria

- [x] Event bus pub/sub works with multiple subscribers, filtered recv, and lagged consumer handling
- [x] Agent chaining: Agent A output triggers Agent B which triggers Agent C (via topic subscriptions) *(Sprint 28)*
- [x] AI router classifies messages and picks best agent by description; falls back to keywords
- [x] Privacy boundaries enforced: `local_only` events only route to `local_only` agents *(Sprint 28)*
- [x] `correlation_id` traces full chain back to original channel message *(Sprint 28)*
- [x] Terminal detection: when no agent subscribes to an output and correlation traces to channel, response is dispatched *(Sprint 28)*
- [x] Explicit pipelines execute sequential steps with error strategies (abort/skip/retry)
- [x] Graceful shutdown: in-flight chains complete within grace period *(Sprint 28)*
- [x] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test --workspace`

---

## Sprint 28: Integration & E2E Tests

**Goal:** Close Sprint 27's testing gaps with orchestrator integration tests, then add e2e tests against a real local LLM (Ollama) in CI. Validates the full stack: provider → agent → tools → orchestrator → channels.

**Baseline:** 17-crate workspace, ~1,730+ tests passing, 0 clippy warnings, orchestrator extracted.

### Phase A: Orchestrator Integration Tests (Sprint 27 carry-over)

Using `StaticProvider` from testkit (no real LLM needed):

- [x] **Agent chain test** — A→B→C via topic subscriptions, terminal dispatches to channel
- [x] **Privacy routing test** — `local_only` events only route to `local_only` agents
- [x] **Pipeline execution test** — 3-step pipeline with abort/skip/retry error strategies
- [x] **Graceful shutdown test** — Dispatch work, send shutdown, verify in-flight completes within grace period
- [x] **Correlation tracking test** — `correlation_id` traces full chain from channel message to terminal response

### Phase B: Local LLM Test Infrastructure

- [x] **Testkit helpers** — `LocalLlmProvider::from_env()`, `skip_without_local_llm()`, `wait_for_server(url, timeout)` in `crates/agentzero-testkit/src/lib.rs`
- [x] **Test pattern** — `#[ignore]` tests, CI runs with `cargo test -- --ignored`

### Phase C: E2E Tests with Real LLM

All `#[ignore]` in `crates/agentzero-infra/tests/e2e_local_llm.rs`:

- [x] **Basic completion** — Prompt → non-empty coherent response
- [x] **Tool use** — Agent + EchoTool, verify tool call round-trip
- [x] **Streaming** — `run_agent_streaming()`, collect chunks, verify reassembly
- [x] **Multi-turn memory** — Two `respond()` calls, second references first
- [x] **Orchestrator routing** — AgentRouter with real LLM classification

### Phase D: CI Workflow

- [x] **e2e-tests job** in `.github/workflows/ci.yml` — `ubuntu-latest`, Ollama + `tinyllama` (cached), `cargo test -- --ignored`, `continue-on-error: true`

### Acceptance Criteria

- [x] Orchestrator integration tests prove: agent chaining, privacy routing, pipeline execution, graceful shutdown, correlation tracking
- [x] E2e tests pass locally with `ollama serve` + `tinyllama`
- [x] E2e tests run in CI (non-blocking initially)
- [x] `cargo test --workspace` passes without Ollama (ignored tests skip)
- [x] All quality gates: fmt, clippy, test

---

## Sprint 29: Conversation Branching, Multi-Modal Input, Plugin Registry Refresh

**Goal:** Add conversation identity so memory entries belong to named conversations that can be listed, switched, and forked. Enable image content in user messages across Anthropic and OpenAI providers. Add plugin registry refresh command.

**Baseline:** 17-crate workspace, ~1,750 tests passing, 0 clippy warnings, orchestrator + e2e tests complete.

### Phase A: Conversation Branching

- [x] **Core types** — `MemoryEntry.conversation_id` (String, serde default), `ToolContext.conversation_id` (Option<String>, serde default). `MemoryStore` trait gains `recent_for_conversation()`, `fork_conversation()`, `list_conversations()` with default implementations.
- [x] **SQLite storage** — `migrate_conversation_column()` following existing migration pattern. `append()` stores conversation_id. Optimized SQL overrides for all three new trait methods. `fork_conversation()` uses `INSERT...SELECT`.
- [x] **Agent loop** — `respond_inner()` uses `recent_for_conversation()` when conversation_id set. `write_to_memory()` threads conversation_id.
- [x] **Runtime wiring** — `RuntimeExecution.conversation_id` and `RunAgentRequest.conversation_id` fields. Populated into `ToolContext` at both construction sites.
- [x] **CLI commands** — `az conversation list`, `az conversation fork`, `az conversation switch`. Active conversation stored in `{data_dir}/active_conversation`. Agent command reads and passes conversation_id.

### Phase B: Multi-Modal Input (Image)

- [x] **Core types** — `ContentPart` enum (`Text { text }`, `Image { media_type, data }`). `ConversationMessage::User` gains `parts: Vec<ContentPart>` (serde skip_serializing_if empty). Constructors: `user()`, `user_with_parts()`.
- [x] **Anthropic provider** — Maps `ContentPart::Image` to existing `InputContentBlock::Image` + `ImageSource`. Uses `MessageContent::Blocks` when parts non-empty.
- [x] **OpenAI provider** — `Message.content` changed to `Option<Value>` for content array format. `image_url` entries with `data:{media_type};base64,{data}` URLs.
- [x] **Image markers wiring** — `load_image_refs()` reads local files/data URIs/remote URLs to `Vec<ContentPart>`. `build_user_message()` parses markers, builds multi-modal message or strips markers when vision disabled.

### Phase C: Plugin Registry Refresh

- [x] **Config** — `registry_url: Option<String>` added to `PluginConfig`.
- [x] **Refresh command** — `refresh_registry_index()` function bypasses cache, reads from URL, saves to cache. `az plugin refresh --registry-url <url>` CLI command.

### Acceptance Criteria

- [x] Memory entries tagged with `conversation_id`; `recent_for_conversation()` filters correctly
- [x] `fork_conversation()` copies entries to new conversation; `list_conversations()` returns distinct IDs
- [x] `az conversation list/fork/switch` round-trip works
- [x] Existing databases seamlessly migrated (new column with default)
- [x] `ContentPart` serde backward compatible (empty parts = text-only)
- [x] Anthropic maps images to `InputContentBlock::Image`; OpenAI maps to content array
- [x] `az plugin refresh` force-refreshes cached registry index
- [x] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test --workspace`

### Backlog (candidates for Sprint 30)

- [x] **Audio input** — Whisper integration for audio across providers *(Sprint 30)*
- [x] **HTTP registry fetch** — Full HTTP support in `load_registry_index()` / `refresh_registry_index()` *(Sprint 30)*
- [x] **Plugin dependency resolution** — Plugins declare dependencies on other plugins *(Sprint 30)*

---

## Sprint 30: Audio Input, HTTP Registry Fetch, Plugin Dependency Resolution → v0.4.0

**Goal:** Ship the three Sprint 29 backlog items and cut a 0.4.0 minor release.

**Baseline:** 17-crate workspace, ~1,800+ tests passing, 0 clippy warnings, Sprint 29 complete.

### Phase A: HTTP Registry Fetch

- [x] **ureq HTTP fetch** — `install_from_url()` handles `https://` and `http://` URLs via `ureq`. `fetch_registry_from_url()` private helper used by `load_registry_index()` and `refresh_registry_index()`. `ureq` chosen over `reqwest::blocking` to avoid panicking inside async CLI context.

### Phase B: Plugin Dependency Resolution

- [x] **`PluginDependency` type** — `{ id: String, version_req: String }` with `semver::VersionReq` validation in `PluginManifest::validate()`.
- [x] **`dependencies` field** — Added with `#[serde(default)]` to `PluginManifest` and `RegistryVersionEntry`.
- [x] **`install_with_dependencies()`** — Resolves and installs transitive deps from registry. Cycle detection via `HashSet<String>`. Missing deps return a clear error.
- [x] **CLI wiring** — `az plugin install --registry-url <url>` loads registry, calls `install_with_dependencies()`. Prints all installed plugins including deps.

### Phase C: Audio Input (Whisper)

- [x] **`AudioConfig`** — `{ api_url, api_key, language, model }` in `AgentZeroConfig`. Defaults to Groq `/audio/transcriptions` + `whisper-large-v3`.
- [x] **`audio_markers.rs`** in `agentzero-infra` — `parse_audio_markers()`, `strip_audio_markers()`, `process_audio_markers()`. Transcribe-first pattern: `[AUDIO:path]` markers replaced with `[Transcription of audio]: <text>` before the LLM sees the message.
- [x] **HTTP transcription** — `transcribe_audio_async()` uses `reqwest` async multipart POST to Whisper endpoint. No marker config or no API key → markers stripped with a warning.
- [x] **Runtime wiring** — `RuntimeExecution.audio_config` populated from config in `build_runtime_execution()`. `process_audio_markers()` called at the top of `run_agent_with_runtime()`.

### Phase D: Release

- [x] **Version bump** — Workspace version `0.3.0` → `0.4.0`
- [x] **SPRINT.md** — Sprint 30 section added; Sprint 29 backlog items marked complete

### Acceptance Criteria

- [x] `az plugin install https://…` downloads and installs plugin with deps resolved
- [x] `az plugin refresh --registry-url https://…` fetches registry over HTTP
- [x] `[AUDIO:path.wav]` in user message → transcript text in LLM message; markers stripped when no API key
- [x] No provider changes needed for audio (transcribe-first)
- [x] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test --workspace`

### Backlog (candidates for Sprint 31)

- [ ] **Plugin registry repo** — New `agentzero-plugins` git repo with example plugins, manifest schema, and CI publishing
- [ ] **Audio streaming** — Real-time transcription in `TranscriptionChannel::listen()` (hardware mic input → agent)
- [ ] **Image generation** — `[IMAGE_GEN:prompt]` markers that call a DALL-E / Stable Diffusion compatible endpoint
