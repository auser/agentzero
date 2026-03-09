# Sprints 25–32: Privacy E2E, Multi-Agent Platform, Production Readiness

Archived from `specs/SPRINT.md` on 2026-03-08.

---

## Sprint 25: Privacy End-to-End

**Goal:** Close privacy enforcement gaps so boundaries are enforced from channel input through memory storage and back.

**Baseline:** 16-crate workspace, 1,431 tests passing, 0 clippy warnings.

### Completed

- [x] **Memory privacy boundaries** — `MemoryEntry` carries `privacy_boundary` and `source_channel` fields. `recent_for_boundary()` filters by boundary. SQLite schema migrated with backward-compatible defaults.
- [x] **Channel privacy boundaries** — `ChannelMessage.privacy_boundary` field. `dispatch_with_boundary()` blocks `local_only` → non-local channels. Per-channel boundary config in TOML. `is_local_channel()` helper.
- [x] **Noise IK client handshake** — `NoiseClientHandshake::new_ik()` constructor. Gateway `/v1/noise/handshake/ik` endpoint. Auto-select IK when server key cached, XX fallback.
- [x] **Privacy CLI test command** — `az privacy test [--json]` runs 8 diagnostic checks.
- [x] **Integration wiring & hardening** — Runtime populates `ToolContext.privacy_boundary`. Leak guard `check_boundary()` blocks local_only content to non-local channels.

---

## Sprint 26: Hardening & Polish

**Goal:** Close remaining backlog items from Sprint 25.

**Baseline:** 16-crate workspace, 1,731+ tests passing.

### Completed

- [x] **Timing jitter for sealed envelope relay** — `JitterConfig` struct with configurable delays. 7 new tests.
- [x] **Stale backlog cleanup** — Verified all 17 "unchecked" tools are fully implemented.
- [x] **Privacy benchmarks** — Criterion 0.5 benchmarks for Noise handshakes, encrypt/decrypt, sealed envelopes.
- [x] **FFI privacy bindings** — `PrivacyBoundary`, `PrivacyInfo`, `PrivacyStatus` exposed via UniFFI and napi-rs.

---

## Sprint 27: Event-Driven Multi-Agent Platform

**Goal:** Transform AgentZero into a full autonomous multi-agent platform. Orchestrator extracted to `agentzero-orchestrator` crate (17 workspace members).

### Completed

- [x] **EventBus trait + InMemoryBus** — `Event` struct with `correlation_id`, `InMemoryBus` via `tokio::sync::broadcast`.
- [x] **ToolContext bus fields** — `event_bus` and `agent_id` added.
- [x] **IPC rewrite to use bus** — Bus-backed with file-based fallback.
- [x] **SwarmConfig + AgentDescriptor** — Config model with topics, pipelines, error strategies.
- [x] **AI AgentRouter** — LLM-based message classification with keyword fallback.
- [x] **Coordinator** — Three concurrent loops: ingestion, routing, response/chain handling.
- [x] **Agent worker loop** — Each agent in `tokio::spawn` with `mpsc` task channel.
- [x] **Pipeline executor** — Sequential pipelines with Abort/Skip/Retry error strategies.
- [x] **SubAgent tool wiring** — Spawn/List/Manage tools registered in `default_tools()`.
- [x] **Runtime integration** — `build_swarm()` creates full orchestration stack.

---

## Sprint 28: Integration & E2E Tests

**Goal:** Orchestrator integration tests + e2e tests against real local LLM (Ollama).

### Completed

- [x] **Orchestrator integration tests** — Agent chain, privacy routing, pipeline execution, graceful shutdown, correlation tracking.
- [x] **Testkit helpers** — `LocalLlmProvider::from_env()`, `skip_without_local_llm()`.
- [x] **E2E tests** — Basic completion, tool use, streaming, multi-turn memory, orchestrator routing.
- [x] **CI workflow** — Ollama + tinyllama in CI (`continue-on-error: true`).

---

## Sprint 29: Conversation Branching, Multi-Modal Input, Plugin Registry Refresh

### Completed

- [x] **Conversation branching** — `conversation_id` on MemoryEntry, fork/list/switch CLI commands, SQLite migration.
- [x] **Multi-modal input (image)** — `ContentPart` enum, Anthropic + OpenAI provider mapping, `load_image_refs()`.
- [x] **Plugin registry refresh** — `registry_url` config, `az plugin refresh` command.

---

## Sprint 30: Audio Input, HTTP Registry Fetch, Plugin Deps → v0.4.0

### Completed

- [x] **HTTP registry fetch** — `ureq` HTTP fetch for plugin install and registry refresh.
- [x] **Plugin dependency resolution** — `PluginDependency` type, transitive dep installation, cycle detection.
- [x] **Audio input (Whisper)** — `AudioConfig`, `[AUDIO:path]` markers transcribed before LLM sees them.
- [x] **Version bump** — v0.4.0 release.

---

## Sprint 31: Anthropic Browser Login

### Completed

- [x] **Anthropic OAuth browser flow** — `auth login --provider anthropic` with PKCE, callback capture, token exchange.
- [x] **Token refresh** — `auth refresh --provider anthropic`.
- [x] **Paste-redirect fallback** — Manual code paste for failed callbacks.

---

## Sprint 32: Structured Logging & Container CI

### Completed

- [x] **LoggingConfig** — `LogFormat` enum, `[logging]` TOML section, env var support.
- [x] **JSON tracing subscriber** — `tracing_subscriber::fmt::layer().json()` when `Json` format.
- [x] **Per-module log levels** — `[logging.modules]` HashMap.
- [x] **Container image CI** — Multi-arch Docker images to `ghcr.io` in release workflow.
