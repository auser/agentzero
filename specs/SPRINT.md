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

- [ ] **Conversation branching** — Forking and branching conversation histories
- [ ] **Multi-modal input** — Image and audio across all providers

---

## Sprint 27: Event-Driven Multi-Agent Platform

**Goal:** Transform AgentZero into a full autonomous multi-agent platform where AI agents communicate via an event bus. Agents subscribe to topics, produce outputs that go back on the bus, and other agents react. The gateway orchestrates routing, chaining, and channel dispatch.

**Baseline:** 16-crate workspace, 1,731+ tests passing, 0 clippy warnings, privacy stack complete.

### Phase A: Foundation (always-on, improves single-agent mode)

- [x] **Publishing simplification** — Added `publish = false` to 14 internal crates. Only `agentzero-core` and `agentzero-plugin-sdk` are publishable. Removed `version` from internal path deps in workspace `Cargo.toml`.
- [x] **EventBus trait + InMemoryBus** — `Event` struct with `correlation_id` for chain tracing, `EventBus`/`EventSubscriber` traits, `InMemoryBus` backed by `tokio::sync::broadcast`. Privacy boundary helpers (`is_boundary_compatible`, `topic_matches`). Always-on (not feature-gated).
- [x] **ToolContext bus fields** — Added `event_bus: Option<Arc<dyn EventBus>>` and `agent_id: Option<String>` to `ToolContext` (serde-skipped).
- [ ] **IPC rewrite to use bus** — Replace file-based `EncryptedJsonStore` IPC in `agents_ipc.rs` with event bus pub/sub.

### Phase B: Gateway Coordinator (feature-gated: `swarm`)

- [ ] **SwarmConfig + AgentDescriptor** — Config model for swarm settings, agent descriptors with `subscribes_to`/`produces` topics, pipeline definitions.
- [ ] **AI AgentRouter** — LLM-based message classification to pick the best agent by name/description. Keyword fallback when AI router fails.
- [ ] **Coordinator** — Three concurrent loops: channel ingestion (publishes channel messages to bus), AI router (routes to agents), response/chain handler (chains agent outputs or dispatches to channels). Dynamic number of agent workers.
- [ ] **Agent worker loop** — Each agent runs in `tokio::spawn`, receives tasks via `mpsc`, outputs go back on the bus.
- [ ] **Response/chain handler** — Subscribes to agent output events. Routes to subscribing agents (chaining) or dispatches to originating channel (terminal detection via `correlation_id`).
- [ ] **Pipeline executor** — Optional explicit sequential pipelines for common workflows. Checked before topic-based routing.

### Phase C: Tool Wiring

- [ ] **SubAgent tool wiring** — Wire `SubAgentSpawnTool`, `SubAgentListTool`, `SubAgentManageTool` to coordinator via event bus.
- [ ] **Runtime integration** — `build_swarm()` in runtime creates bus, router, agent workers, and coordinator.

### Phase D: Tests & Verification

- [ ] **Unit tests** — EventBus pub/sub, filtered recv, correlation_id propagation, topic matching, boundary compatibility, AI routing with mock provider.
- [ ] **Integration tests** — Agent chain (A→B→C→channel), privacy routing, graceful shutdown, error propagation.

### Acceptance Criteria

- [ ] Event bus pub/sub works with multiple subscribers, filtered recv, and lagged consumer handling
- [ ] Agent chaining: Agent A output triggers Agent B which triggers Agent C (via topic subscriptions)
- [ ] AI router classifies messages and picks best agent by description; falls back to keywords
- [ ] Privacy boundaries enforced: `local_only` events only route to `local_only` agents
- [ ] `correlation_id` traces full chain back to original channel message
- [ ] Terminal detection: when no agent subscribes to an output and correlation traces to channel, response is dispatched
- [ ] Explicit pipelines execute sequential steps with error strategies (abort/skip/retry)
- [ ] Graceful shutdown: in-flight chains complete within grace period
- [ ] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test --workspace`
