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

- [ ] **Remaining unchecked tools** — cli_discovery, composio, delegate_coordination_status, hardware_*, model_routing_config, proxy_config, pushover, schedule, sop_* (5), wasm_module, wasm_tool
- [ ] **FFI bindings update** — Expose privacy types through UniFFI (Swift/Kotlin) and napi-rs (Node)
- [ ] **Benchmarks** — Noise handshake latency, encrypt/decrypt throughput, relay mailbox performance
- [ ] **Timing jitter** — Add randomized delays to sealed envelope relay to mitigate traffic analysis
