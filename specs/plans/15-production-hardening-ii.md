# Plan 08: Production Hardening II (Sprint 37)

## Context

AgentZero is at v0.5.6 with 16 workspace crates, 1,400+ tests, 0 clippy warnings, and 36 sprints of development. The core orchestration, delegation security, budgeting, and multi-agent coordination features are solid. This plan identifies the remaining gaps between current state and a production-ready system suitable for external-facing, multi-tenant deployments.

**Current overall readiness: ~65%** — solid for single-server internal use, needs hardening for production at scale.

---

## Gap Summary by Category

### 1. SECURITY — Critical Gaps

| Gap | Severity | Current State | What's Needed |
|-----|----------|---------------|---------------|
| API key scope enforcement not wired | CRITICAL | Scopes defined but handlers don't check them | Scope-checking middleware on all `/v1/*` routes |
| OTP secret printed to stdout | HIGH | `lib.rs` prints `otpauth://` URI | Log at DEBUG level |
| Session tokens never expire | HIGH | Paired tokens have no TTL | Configurable session TTL (default 7d) |
| API key store is in-memory only | HIGH | Keys lost on restart | Persist via `agentzero-storage` encrypted backend |
| No WebSocket message size limit | MEDIUM | Frames accepted without size check | Set max frame size in axum WebSocket config |
| No request body schema validation | MEDIUM | `webhook()` accepts untyped JSON | Channel name validation |

### 2. TLS & NETWORKING — Critical Gap

| Gap | Severity | Current State | What's Needed |
|-----|----------|---------------|---------------|
| TLS not wired into listener | CRITICAL | Feature exists, config parsed, but plain TCP only | Wire `axum_server::tls_rustls` |
| No HSTS headers | HIGH | Even if TLS added, no Strict-Transport-Security | Add HSTS middleware |

### 3. OBSERVABILITY — Moderate Gaps

| Gap | Severity | Current State | What's Needed |
|-----|----------|---------------|---------------|
| No business/provider metrics | HIGH | Gateway has Prometheus metrics, but no per-provider histograms | Add metrics in provider layer |
| No correlation IDs | HIGH | Can't trace request across services | Propagate `request_id` in tracing spans + headers |
| No structured audit log | HIGH | Auth failures logged, but no structured audit trail | Dedicated audit log |
| No `#[instrument]` usage | MEDIUM | All spans manual | Add `#[instrument]` on key async functions |

### 4. DATA & PERSISTENCE — Moderate Gaps

| Gap | Severity | Current State | What's Needed |
|-----|----------|---------------|---------------|
| No migration versioning | HIGH | Migrations run as "add column if not exists" with no tracking | Schema version table |
| Cost tracker uses ad-hoc JSON file | MEDIUM | `cost_usage.json` via `std::fs` (violates AGENTS.md rule 9) | Migrate to `agentzero-storage` |
| No per-tool execution timeout | MEDIUM | Tools can run indefinitely | `tokio::time::timeout` per tool |

### 5. TESTING — Specific Gaps

| Gap | Severity | Current State | What's Needed |
|-----|----------|---------------|---------------|
| No E2E security test suite | HIGH | Individual unit tests exist, no full auth flow | Integration test covering full auth lifecycle |
| No load/stress tests | HIGH | No sustained traffic testing | Load tests for gateway |
| No WebSocket tests | MEDIUM | `ws_relay()` has no tests | Connection, message, size limit tests |

---

## Implementation Phases

### Phase A: Security Essentials (CRITICAL) — DONE
1. Wire API key scope enforcement middleware on all `/v1/*` handler routes
2. Add session TTL to paired tokens (configurable, default 7 days)
3. Persist `ApiKeyStore` to `agentzero-storage` encrypted backend
4. Move OTP secret to DEBUG log level
5. Add `.unwrap()` prohibition to AGENTS.md

### Phase B: TLS & Input Hardening (CRITICAL/HIGH) — DONE
6. Wire `tls-rustls` into gateway listener (conditional on config)
7. Add HSTS header middleware
8. Add WebSocket frame size limit (2 MB)
9. Validate webhook channel names against pattern

### Phase C: Observability & Audit (HIGH) — DONE
10. Add per-provider metrics (request count, latency histogram, error rate, token usage) — DONE
11. Add correlation ID (`X-Request-Id`) propagation through all spans and response headers — DONE
12. Add structured audit log for security events (auth, key management, estop) — DONE
13. Add `#[instrument]` to key async paths (agent respond, tool execute, provider call) — DONE

### Phase D: Data Integrity (HIGH) — DONE
14. Add schema version table and migration tracking to SQLite/pooled stores — DONE
15. Migrate cost tracker from ad-hoc JSON to `agentzero-storage` — DONE
16. Add per-tool execution timeout (`tokio::time::timeout`) — DONE

### Phase E: Testing (HIGH) — DONE
17. E2E security integration test: create API key → scope enforcement → request flow — DONE
18. Load test: gateway under sustained concurrent requests — DONE
19. WebSocket relay tests (connect, message, size limit rejection) — DONE

---

## Files Modified

### Phase A
- `crates/agentzero-gateway/src/api_keys.rs` — persistent store via `EncryptedJsonStore`
- `crates/agentzero-gateway/src/auth.rs` — session TTL, scope enforcement
- `crates/agentzero-gateway/src/state.rs` — TTL fields, API key store
- `crates/agentzero-gateway/src/lib.rs` — OTP log level
- `AGENTS.md` — `.unwrap()` prohibition rule

### Phase B
- `crates/agentzero-gateway/src/lib.rs` — TLS listener wiring
- `crates/agentzero-gateway/src/middleware.rs` — HSTS middleware
- `crates/agentzero-gateway/src/router.rs` — HSTS layer
- `crates/agentzero-gateway/src/handlers.rs` — WS size limit, channel validation
- `crates/agentzero-gateway/Cargo.toml` — tls feature, axum-server dep
- `crates/agentzero-config/src/model.rs` — TlsConfig struct

### Phase C
- `crates/agentzero-providers/src/provider_metrics.rs` — new module
- `crates/agentzero-providers/src/anthropic.rs` — metrics injection
- `crates/agentzero-providers/src/openai.rs` — metrics injection
- `crates/agentzero-gateway/src/middleware.rs` — correlation ID middleware

### Site Docs
- `site/src/content/docs/reference/gateway.md` — all new features documented
- `site/src/content/docs/config/reference.md` — TLS config section
