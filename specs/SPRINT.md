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
