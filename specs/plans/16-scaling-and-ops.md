# Plan 09: Scaling & Operational Readiness (Sprint 38)

## Context

AgentZero is at v0.5.6+ with 16 workspace crates, 2,132 tests, 0 clippy warnings, and 37 sprints of development. Sprint 37 closed all CRITICAL and HIGH security/reliability gaps. The system is now suitable for single-instance external-facing deployments.

This sprint focuses on **scaling** (multi-instance readiness, per-identity rate limiting, provider resilience) and **operational tooling** (backup/restore, production config validation, OpenAPI spec, Docker hardening).

**Current overall readiness: ~80%** — secure and well-tested for single-instance use, needs scaling and ops tooling for production at scale.

---

## Gap Summary

| Gap | Severity | Current State | What's Needed |
|-----|----------|---------------|---------------|
| Per-identity rate limiting | HIGH | Global sliding window (`AtomicU64`) | Per-API-key/org rate limit buckets |
| Provider fallback chain | HIGH | Circuit breaker only, single provider per route | Try alternate providers on failure/circuit-open |
| No OpenAPI spec | MEDIUM | Routes defined manually, no auto-generated spec | `utoipa` for OpenAPI 3.0 generation |
| No data backup/restore | MEDIUM | Binary rollback only (`updater.rs`) | `backup export` / `backup restore` CLI commands |
| No prod config validation | MEDIUM | Same validation for dev and prod | Strict mode when `AGENTZERO_ENV=production` |
| Docker resource limits | MEDIUM | No memory/CPU limits in docker-compose | Add `deploy.resources` constraints |
| No container image scanning | LOW | Dockerfile exists, no Trivy/Grype in CI | Add image scan step to CI pipeline |

---

## Implementation Phases

### Phase A: Per-Identity Rate Limiting (HIGH)

Replace the global `AtomicU64` rate limiter with per-identity buckets. Each API key (or org) gets its own sliding window counter. Global rate limiting remains as a fallback for unauthenticated requests.

**Tasks:**
1. Refactor `RateLimiter` to use `DashMap<String, SlidingWindowCounter>` for per-identity tracking
2. Extract identity key from request: API key → `key_id`, bearer token → `"bearer"`, unauthenticated → `"global"`
3. Add configurable per-identity rate limit (`rate_limit_per_identity`) alongside global limit
4. Add GC for expired identity buckets (periodic or on access)
5. Add `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` response headers
6. Tests: per-key isolation, key A at limit doesn't block key B, global fallback, header values

**Files:**
- `crates/agentzero-gateway/src/middleware.rs` — refactored `RateLimiter`
- `crates/agentzero-gateway/src/middleware.rs` — rate limit headers
- `crates/agentzero-config/src/model.rs` — `rate_limit_per_identity` config field
- `crates/agentzero-gateway/Cargo.toml` — `dashmap` dependency

### Phase B: Provider Fallback Chain (HIGH)

When the primary provider fails (circuit breaker open or persistent errors), automatically try the next provider in a configured fallback chain.

**Tasks:**
7. Add `fallback_providers` field to provider config (ordered list of provider names)
8. Create `FallbackProvider` wrapper that wraps a chain of providers, trying each in order
9. On primary failure (circuit open, 5xx, timeout), try next provider in chain
10. Emit `provider_fallback_total` metric with `from` and `to` labels
11. Tests: primary fails → fallback succeeds, all fail → error, circuit open triggers fallback

**Files:**
- `crates/agentzero-providers/src/fallback.rs` — new `FallbackProvider` implementation
- `crates/agentzero-providers/src/lib.rs` — module registration
- `crates/agentzero-config/src/model.rs` — `fallback_providers` config field
- `crates/agentzero-infra/src/runtime.rs` — wire fallback chain at startup

### Phase C: OpenAPI Spec Generation (MEDIUM)

Auto-generate OpenAPI 3.0 specification from handler types, served at `/v1/openapi.json`.

**Tasks:**
12. Add `utoipa` dependency to gateway crate
13. Annotate request/response types with `#[derive(utoipa::ToSchema)]`
14. Annotate handlers with `#[utoipa::path(...)]` attributes
15. Add `GET /v1/openapi.json` endpoint serving the generated spec
16. Tests: endpoint returns valid JSON, schema includes key endpoints

**Files:**
- `crates/agentzero-gateway/Cargo.toml` — `utoipa` dependency
- `crates/agentzero-gateway/src/models.rs` — schema derives
- `crates/agentzero-gateway/src/handlers.rs` — path annotations
- `crates/agentzero-gateway/src/router.rs` — new route

### Phase D: Backup/Restore CLI (MEDIUM)

Data export and import commands for disaster recovery and migration.

**Tasks:**
17. `agentzero backup export <output-dir>` — exports encrypted stores (API keys, cost tracker, conversation memory) to a portable archive
18. `agentzero backup restore <archive-path>` — imports archive, merging or replacing existing data
19. Export format: tar.gz containing individual JSON files (decrypted for portability, re-encrypted on import)
20. Validate archive integrity before restore (checksum)
21. Tests: round-trip export → restore preserves data, corrupt archive rejected

**Files:**
- `crates/agentzero-cli/src/backup.rs` — new module
- `crates/agentzero-cli/src/cli.rs` — `backup` subcommand registration
- `crates/agentzero-storage/src/lib.rs` — export/import helpers on `EncryptedJsonStore`

### Phase E: Production Config & Docker Hardening (MEDIUM)

**Tasks:**
22. Add `AGENTZERO_ENV` environment variable support (`development`, `production`)
23. Production mode validation: require TLS or explicit `allow_insecure`, require auth (no open mode), warn on localhost bind with public-facing config
24. Add `deploy.resources` to `docker-compose.yml` (memory limit: 512MB, CPU: 1.0)
25. Add Dockerfile healthcheck using conditional HTTP/HTTPS
26. Tests: prod validation rejects insecure config, dev mode permissive

**Files:**
- `crates/agentzero-config/src/model.rs` — `RuntimeConfig.environment` field
- `crates/agentzero-config/src/loader.rs` — env-based validation
- `docker-compose.yml` — resource limits
- `Dockerfile` — healthcheck update

---

## Files Modified (Summary)

### Phase A
- `crates/agentzero-gateway/src/middleware.rs`
- `crates/agentzero-gateway/Cargo.toml`
- `crates/agentzero-config/src/model.rs`

### Phase B
- `crates/agentzero-providers/src/fallback.rs` (new)
- `crates/agentzero-providers/src/lib.rs`
- `crates/agentzero-config/src/model.rs`
- `crates/agentzero-infra/src/runtime.rs`

### Phase C
- `crates/agentzero-gateway/Cargo.toml`
- `crates/agentzero-gateway/src/models.rs`
- `crates/agentzero-gateway/src/handlers.rs`
- `crates/agentzero-gateway/src/router.rs`

### Phase D
- `crates/agentzero-cli/src/backup.rs` (new)
- `crates/agentzero-cli/src/cli.rs`
- `crates/agentzero-storage/src/lib.rs`

### Phase E
- `crates/agentzero-config/src/model.rs`
- `crates/agentzero-config/src/loader.rs`
- `docker-compose.yml`
- `Dockerfile`

### Site Docs
- `site/src/content/docs/reference/gateway.md` — rate limit headers, OpenAPI endpoint, fallback config
- `site/src/content/docs/config/reference.md` — new config fields

## Verification

- `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings)
- `cargo test --workspace` (all existing + new tests pass)
- Manual: per-identity rate limiting isolated between keys
- Manual: provider fallback triggers on primary failure
- Manual: `GET /v1/openapi.json` returns valid OpenAPI 3.0 spec
- Manual: `backup export` → `backup restore` round-trip
- Manual: production mode rejects insecure configuration
