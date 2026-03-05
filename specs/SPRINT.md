# AgentZero Sprint Plan

## Sprint 25: TBD

**Goal:** To be determined.

**Baseline:** 16-crate workspace, 1,338 tests passing, 0 clippy warnings, privacy system production-ready (Noise Protocol, sealed envelopes, per-component boundaries, key rotation).

Previous sprints archived to `specs/sprints/23-24-production-readiness-privacy.md`.

---

### Backlog (candidates for Sprint 25)

- [ ] **Conversation privacy tagging** — Tag conversations with effective PrivacyBoundary at creation time; prevent local_only conversations from being loaded by cloud-backed agents
- [ ] **IK handshake pattern** — Implement IK (known server key) pattern for faster reconnection; skip step 1 when client already knows server's public key
- [ ] **Channel privacy enforcement** — Wire channel_privacy config into pipeline dispatch; tag incoming messages with channel's resolved boundary
- [ ] **Privacy CLI improvements** — `az privacy status` shows active mode, noise sessions, relay stats; `az privacy test` validates connectivity
- [ ] **Remaining unchecked tools** — cli_discovery, composio, delegate_coordination_status, hardware_*, model_routing_config, proxy_config, pushover, schedule, sop_* (5), wasm_module, wasm_tool
- [ ] **FFI bindings update** — Expose privacy types through UniFFI (Swift/Kotlin) and napi-rs (Node)
- [ ] **Benchmarks** — Noise handshake latency, encrypt/decrypt throughput, relay mailbox performance
