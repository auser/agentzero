# Plan 04: API Polish (OpenAPI, Auth Hardening, Health Probes)

## Problem

The gateway has a solid REST API surface (OpenAI-compatible chat completions, WebSocket streaming, Prometheus metrics, webhook dispatch) but lacks professional-grade polish:

1. **No OpenAPI specification** — No machine-readable API docs. Clients must read markdown docs or reverse-engineer endpoints.
2. **Timing-vulnerable auth** — Bearer token comparison uses `==` (line 48 of `auth.rs`), which is theoretically vulnerable to timing side-channel attacks.
3. **Single health endpoint** — Only `GET /health`. Kubernetes needs separate liveness (process alive) and readiness (can serve traffic) probes for rolling deployments.
4. **No pagination** — No pagination on any endpoint.
5. **No API versioning strategy** — `/v1/` prefix exists but no versioning middleware.

## Current State

### API endpoints (`crates/agentzero-gateway/src/handlers.rs` + routing)
- `GET /health` — returns `{"status":"ok"}`, no auth
- `GET /metrics` — Prometheus format, no auth
- `POST /pair` — pairing code exchange
- `POST /v1/ping` — bearer auth
- `POST /v1/webhook/:channel` — bearer auth
- `POST /api/chat` — simple chat, bearer auth
- `POST /v1/chat/completions` — OpenAI-compatible, streaming SSE, bearer auth
- `GET /v1/models` — OpenAI-compatible model list
- `GET /ws/chat` — WebSocket streaming
- Privacy: `/v1/privacy/info`, `/v1/noise/handshake/*`, `/v1/relay/*`

### Auth (`crates/agentzero-gateway/src/auth.rs`)
```rust
// Line ~48 — timing-vulnerable string comparison
if expected.as_str() == token {
    return true;
}
```

### Health (`crates/agentzero-gateway/src/handlers.rs`)
```rust
pub async fn health_handler() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}
```

## Implementation

### Phase 1: Constant-Time Token Comparison

**Add dep:** `subtle = "2"` to `crates/agentzero-gateway/Cargo.toml`

**Fix in `crates/agentzero-gateway/src/auth.rs`:**
```rust
use subtle::ConstantTimeEq;

fn verify_token(expected: &str, provided: &str) -> bool {
    if expected.len() != provided.len() {
        return false;  // length mismatch is not timing-sensitive
    }
    expected.as_bytes().ct_eq(provided.as_bytes()).into()
}
```

Also apply to pairing code verification if it uses `==`.

### Phase 2: Liveness / Readiness Probes

**New endpoints in `crates/agentzero-gateway/src/handlers.rs`:**

```rust
/// Liveness probe: is the process alive?
/// Returns 200 always. Used by k8s livenessProbe.
pub async fn liveness_handler() -> impl IntoResponse {
    Json(json!({"status": "alive"}))
}

/// Readiness probe: can we serve traffic?
/// Checks DB connectivity and optionally LLM provider reachability.
pub async fn readiness_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let db_ok = state.memory_store.health_check().await.is_ok();
    let status = if db_ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(json!({
        "status": if db_ok { "ready" } else { "not_ready" },
        "checks": {
            "database": db_ok,
        }
    })))
}
```

**Routes:**
- `GET /health/live` → `liveness_handler` (no auth)
- `GET /health/ready` → `readiness_handler` (no auth)
- `GET /health` → alias for `/health/ready` (backward compat)

**MemoryStore trait addition:**
```rust
async fn health_check(&self) -> Result<()>;
```

SQLite implementation: `SELECT 1` query.

### Phase 3: OpenAPI Specification

**Add dep:** `utoipa = "5"` and `utoipa-axum = "0.2"` to `crates/agentzero-gateway/Cargo.toml`

Feature-gated behind `api-docs` to keep binary small when not needed.

**Annotate handlers:**
```rust
#[utoipa::path(
    post,
    path = "/v1/chat/completions",
    request_body = ChatCompletionRequest,
    responses(
        (status = 200, description = "Chat completion", body = ChatCompletionResponse),
        (status = 401, description = "Unauthorized"),
    ),
    security(("bearer" = [])),
)]
pub async fn chat_completions_handler(...) { ... }
```

**Serve spec:**
- `GET /openapi.json` — raw OpenAPI 3.1 spec
- `GET /docs` — Swagger UI (optional, via `utoipa-swagger-ui`)

**Derive `ToSchema` on request/response types:**
- `ChatCompletionRequest`, `ChatCompletionResponse`
- `PairRequest`, `PairResponse`
- `HealthResponse`, `ErrorResponse`
- `PrivacyInfo`

### Phase 4: Structured Error Responses

Ensure all error responses follow a consistent schema:

```json
{
    "error": {
        "type": "authentication_error",
        "message": "Invalid or missing bearer token",
        "request_id": "abc-123-def"
    }
}
```

Review `GatewayError` enum (already has 8 variants) — ensure all variants produce this format and include request ID from Phase 2 of Plan 02.

## Files to Create/Modify

| File | Action |
|------|--------|
| `crates/agentzero-gateway/Cargo.toml` | Add subtle, utoipa deps |
| `crates/agentzero-gateway/src/auth.rs` | Constant-time comparison |
| `crates/agentzero-gateway/src/handlers.rs` | Liveness/readiness probes, OpenAPI annotations |
| `crates/agentzero-gateway/src/lib.rs` | Register new routes, mount OpenAPI |
| `crates/agentzero-core/src/types.rs` | Add `health_check()` to MemoryStore trait |
| `crates/agentzero-storage/src/memory/sqlite.rs` | Implement `health_check()` |

## Tests (~8 new)

1. Constant-time comparison: correct token → true, wrong token → false, empty token → false
2. Liveness probe: always returns 200
3. Readiness probe: returns 200 when DB accessible
4. Readiness probe: returns 503 when DB unavailable
5. Health endpoint backward compat: `/health` still works
6. OpenAPI spec: `/openapi.json` returns valid JSON with expected paths
7. Error response format: all error variants include `type`, `message`, `request_id`
8. Auth timing: verify `subtle::ConstantTimeEq` is used (code review test)

## Verification

1. `curl localhost:3000/health/live` → `{"status":"alive"}` (always 200)
2. `curl localhost:3000/health/ready` → `{"status":"ready","checks":{"database":true}}`
3. `curl localhost:3000/openapi.json` → valid OpenAPI 3.1 spec
4. `curl localhost:3000/docs` → Swagger UI renders
5. Auth with wrong token returns structured error with request ID
6. All existing tests pass
7. `cargo clippy` clean

## Dependencies Added

| Crate | Version | Condition |
|-------|---------|-----------|
| `subtle` | 2 | always |
| `utoipa` | 5 | `api-docs` feature |
| `utoipa-axum` | 0.2 | `api-docs` feature |
| `utoipa-swagger-ui` | 9 | `api-docs` feature |
