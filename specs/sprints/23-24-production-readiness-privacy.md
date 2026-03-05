# Sprints 23–24: Production Readiness, Observability & Private AI

Archived from SPRINT.md. Both sprints completed.

---

## Sprint 23: Production Readiness & Observability ✅

**Goal:** Make the gateway deployable with real metrics, harden WebSocket handling, fill critical test coverage gaps in storage and tools, add provider tracing, and update site documentation.

**Baseline:** 16-crate workspace, 1,595 tests passing, 0 clippy warnings, clean `main` branch.

### Phase 1: Gateway Production Readiness (P0) ✅

- [x] Real Prometheus metrics — `gateway_metrics.rs` with counters, histogram, gauge; `request_metrics` middleware
- [x] Dynamic `/v1/models` — Uses `supported_providers()` + `find_models_for_provider()` from catalog
- [x] WebSocket hardening — heartbeat ping, pong timeout, idle timeout; binary frame rejection
- [x] Structured error types — `GatewayError` enum (8 variants) with JSON error responses
- [x] Default rate limit — Changed to 600 req/min
- [x] Tests (11 new): 67 gateway tests total

### Phase 2: Test Coverage Expansion (P0) ✅

- [x] agentzero-storage: 19 → 46 tests (symmetric, key, store, queue, sqlite)
- [x] agentzero-tools: task_plan +3, cron_store +3, agents_ipc +2

### Phase 3: Provider Observability (P1) ✅

- [x] Tracing spans on all 8 provider methods
- [x] Transport logging upgrade
- [x] Circuit breaker visibility
- [x] Tests (+4): 134 provider tests total

### Phase 4: Site Documentation Updates (P1) ✅

- [x] Gateway docs, architecture docs, threat model, provider guide

### Phase 5: Config Cleanup (P2) ✅

- [x] Config audit + validation tests (+4): 100 config tests total

---

## Sprint 24: Private AI Production-Readiness ✅

**Goal:** Make privacy features production-ready: wire gateway, fix security gaps, add Noise client, per-component privacy boundaries, metrics, integration tests, and documentation.

**Baseline:** Privacy Phases 1-6 complete (60 tests across 4 crates), gateway wiring not connected, no client-side encryption, no per-component boundaries.

### Phase 1: Critical Security Fixes (P0) ✅

- [x] Fix "full" mode semantics — only "local_only" blocks cloud
- [x] Remove Serialize from IdentityKeyPair — prevent secret key leaks
- [x] Wire privacy into gateway startup — NoiseSessionStore, RelayMailbox, key rotation task
- [x] Key rotation lifecycle — force_rotate(), persist on rotate, --force CLI flag

### Phase 2: Noise Protocol Client + Key Distribution (P0) ✅

- [x] Client-side Noise handshake — noise_client.rs with NoiseClientHandshake + NoiseClientSession
- [x] GET /v1/privacy/info endpoint — capability discovery
- [x] HTTP Noise transport — NoiseHttpTransport wraps reqwest with encrypt/decrypt

### Phase 3: Security Hardening (P0) ✅

- [x] Sealed envelope replay protection — DashMap nonce dedup, HTTP 409 on replay
- [x] Local provider URL enforcement — reject non-localhost in local_only
- [x] Network-level tool enforcement — disable network tools in local_only
- [x] Plugin network isolation — WASM allow_network=false in local_only

### Phase 4: Per-Component Privacy Boundaries (P1) ✅

- [x] PrivacyBoundary enum with resolve() logic (11 tests)
- [x] String-based privacy helpers in common/ (8 tests)
- [x] Agent boundaries, tool boundaries, thread context
- [x] Enforcement: provider selection, tool execution, plugin isolation
- [x] Config validation (4 new tests)

### Phase 5: Metrics + Integration Tests (P1) ✅

- [x] 6 Prometheus privacy metrics wired into handshake, relay, rotation, noise middleware
- [x] E2E encrypted request/response test (handshake → encrypt → decrypt)
- [x] Noise round-trip, relay round-trip, privacy info, per-component enforcement tests
- [x] 102 gateway tests total

### Phase 6: Polish + Documentation (P2) ✅

- [x] Privacy guide, config reference, threat model update, AGENTS.md principles

### Acceptance Criteria (all met)

- Gateway starts with noise sessions, relay, and key rotation
- Client can perform Noise handshake and send encrypted requests
- local_only mode blocks all outbound network
- Per-component boundaries resolve correctly
- Sealed envelope replay rejected
- Prometheus /metrics includes privacy metrics
- E2E encryption round-trip tested
- 1,338 total workspace tests passing, 0 clippy warnings
