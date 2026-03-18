# Sprint 23: Production Readiness & Observability — Implementation Plan

## Context

Sprint 22H (Hardening, Coverage & Polish) is complete — all checkboxes checked, 1,595 tests passing, 0 clippy warnings. A codebase audit identified five focus areas for production readiness:

1. **Gateway gaps**: `/metrics` is a hardcoded stub, `/v1/models` returns 2 hardcoded models, WebSocket handler lacks heartbeat/idle timeout, all errors return generic status codes, rate limiting defaults to disabled
2. **Test coverage**: `agentzero-storage` has 19 tests across 10 files (weakest crate), `agentzero-tools` has thin coverage on several modules
3. **Provider observability**: Only 2 `trace!` calls in the entire provider layer — zero visibility into request latency, retries, or failures
4. **Site docs**: Pages exist but need updating to reflect Phase 1/3 changes
5. **Config dead code**: `QueryClassificationConfig` and `EmbeddingRoute` may be partially wired

## Phase 1: Gateway Production Readiness

### 1.1 Real Prometheus Metrics

**New crate dependencies:**
- `metrics = "0.24"` and `metrics-exporter-prometheus = "0.16"` in workspace `Cargo.toml`
- Add both to `crates/agentzero-gateway/Cargo.toml`

**New file: `crates/agentzero-gateway/src/metrics.rs`**

Define metric names as constants:
- `agentzero_gateway_requests_total` (counter, labels: method, path, status)
- `agentzero_gateway_request_duration_seconds` (histogram, labels: method, path)
- `agentzero_gateway_active_connections` (gauge)
- `agentzero_gateway_ws_connections_total` (counter)
- `agentzero_gateway_errors_total` (counter, labels: error_type)

**Modify: `crates/agentzero-gateway/src/state.rs`**
- Add `prometheus_handle: metrics_exporter_prometheus::PrometheusHandle` to `GatewayState`
- Initialize in `GatewayState::new()` via `PrometheusBuilder::new().install_recorder()`
- Update test helpers to initialize the handle

**Modify: `crates/agentzero-gateway/src/handlers.rs`**
- Replace hardcoded `metrics()` handler (line 42) with `state.prometheus_handle.render()`
- Change signature to accept `State(state): State<GatewayState>`

**Modify: `crates/agentzero-gateway/src/middleware.rs`**
- Add `request_metrics` middleware that records method, path, status, and latency

**Modify: `crates/agentzero-gateway/src/router.rs`**
- Wire `request_metrics` middleware into `build_router()`

### 1.2 Dynamic `/v1/models`

**Modify: `crates/agentzero-gateway/src/models.rs`**
- Change `ModelItem` fields `id` and `owned_by` from `&'static str` to `String`

**Modify: `crates/agentzero-gateway/src/handlers.rs`**
- In `v1_models()` (line 290): import `agentzero_providers::{supported_providers, find_models_for_provider}`
- Build model list dynamically from the full catalog
- Keep OpenAI-compatible response shape `{ object: "list", data: [...] }`

### 1.3 WebSocket Hardening

**Modify: `crates/agentzero-gateway/src/handlers.rs`** — `handle_socket()` function

- Add ping/pong heartbeat: send `Ping` every 30s via `tokio::time::interval`, close if no `Pong` within 60s
- Handle `Message::Binary`: respond with `{"type":"error","message":"binary frames not supported"}`
- Add 5min idle timeout: close WebSocket with close frame if no client message
- Use `tokio::select!` to multiplex incoming messages, heartbeat, and idle timeout
- Increment `ws_connections_total` counter on connection open

### 1.4 Structured Error Types

**Modify: `crates/agentzero-gateway/src/models.rs`**

```rust
enum GatewayError {
    AuthRequired,
    AuthFailed,
    NotFound { resource: String },
    AgentUnavailable,
    AgentExecutionFailed { message: String },
    RateLimited,
    PayloadTooLarge,
    BadRequest { message: String },
}
```

- Impl `IntoResponse`: return JSON `{"error":{"type":"auth_required","message":"..."}}` with status 401/403/404/503/500/429/413/400
- Increment `errors_total{error_type=...}` counter

**Modify: `crates/agentzero-gateway/src/handlers.rs`**
- Migrate `api_chat`, `v1_chat_completions`, `ws_chat`, `webhook` from `Result<_, StatusCode>` to `Result<_, GatewayError>`

### 1.5 Default Rate Limit

**Modify: `crates/agentzero-gateway/src/middleware.rs`**
- Change `MiddlewareConfig::default()` `rate_limit_max` from `0` to `600`
- Update `middleware_config_defaults` test

### 1.6 Tests (14 new)

All in `crates/agentzero-gateway/src/tests.rs`:
- Metrics endpoint returns real counter values after requests
- Request duration histogram populated
- Error counter increments on 4xx/5xx
- All expected metric names present in `/metrics` output
- `/v1/models` returns catalog models (count matches, known providers appear)
- Model IDs match catalog entries
- Binary WebSocket frame receives error response
- Heartbeat ping sent (test WebSocket client over real TCP)
- Idle timeout closes connection
- `AgentUnavailable` → 503 with structured JSON
- `AuthRequired` → 401 with structured JSON
- `BadRequest` → 400 with message
- `AgentExecutionFailed` → 500 with message
- Default rate limit 600 allows then rejects

---

## Phase 2: Test Coverage Expansion

### Storage Crate (19 → 45+ tests)

**`crates/agentzero-storage/src/crypto/symmetric.rs`** (currently 2 tests):
- Empty plaintext encrypt/decrypt round-trip
- Truncated ciphertext returns error
- Modified nonce returns error
- Invalid JSON envelope returns error
- Two encryptions of same plaintext produce different ciphertexts

**`crates/agentzero-storage/src/crypto/key.rs`** (currently 2 tests):
- Valid 64-char hex accepted
- Empty string rejected
- Wrong-length rejected
- `from_config_dir` creates key file if missing

**`crates/agentzero-storage/src/store.rs`** (currently 3 tests):
- Save creates parent directories
- Save + delete makes load return None
- `load_or_default` with missing file
- Corrupt encrypted file returns error
- Concurrent save from two threads doesn't corrupt

**`crates/agentzero-storage/src/queue.rs`** (currently 4 tests):
- Dequeue nonexistent ID is no-op
- `len()` correct after enqueue/dequeue
- Complex nested payload round-trips through drain

**`crates/agentzero-storage/src/memory/sqlite.rs`** (currently 6 tests):
- `recent(0)` returns empty vec
- Large limit returns all entries
- 10KB content round-trips
- Unicode/emoji content round-trips

### Tools Crate (targeted gaps)

**`crates/agentzero-tools/src/task_plan.rs`** (3 tests / 309 lines):
- Empty plan creation succeeds
- Multi-step serialize/deserialize
- Status transitions (pending→in_progress→completed)
- Invalid input returns structured error

**`crates/agentzero-tools/src/cron_tools.rs`** (4 tests / 409 lines):
- Valid cron expression parsing
- Invalid expression rejected
- List when empty returns empty
- Remove nonexistent job is no-op

**`crates/agentzero-tools/src/agents_ipc.rs`** (2 tests / 234 lines):
- Empty body returns error
- Unknown agent returns descriptive error
- Message serialization round-trip

**`crates/agentzero-tools/src/cron_store.rs`** (2 tests / 179 lines):
- Persistence round-trip (save then load)
- Corrupted file returns error

---

## Phase 3: Provider Observability

**Modify: `crates/agentzero-providers/src/anthropic.rs`**
- Add `#[tracing::instrument(skip(self), fields(provider = "anthropic", model = %self.model))]` to `chat()` and `chat_streaming()`
- `info!` on request start/completion with latency_ms, input_tokens, output_tokens
- `warn!` on retry with attempt count and reason
- `error!` on final failure

**Modify: `crates/agentzero-providers/src/openai.rs`**
- Same pattern as anthropic.rs with `provider = "openai"`

**Modify: `crates/agentzero-providers/src/transport.rs`**
- Upgrade `trace!`/`debug!` to `info!`/`warn!` for request/response/retry
- Add `info!` on circuit breaker state transitions
- Expose `CircuitBreakerStatus { state_label: &str, failure_count: u32 }` struct

**Tests (4 new):** compilation with instrument attributes, transport log fields, circuit breaker status, state label after transitions

---

## Phase 4: Site Documentation Updates

**`site/src/content/docs/reference/gateway.md`**: metrics endpoint (counters, histograms, scrape config), dynamic `/v1/models`, WebSocket heartbeat/timeout, structured error format, default rate limit

**`site/src/content/docs/architecture.md`**: crate count 16, observability section

**`site/src/content/docs/security/threat-model.md`**: WebSocket abuse entry, rate limiting/DOS entry, Prometheus metrics for anomaly detection

**`site/src/content/docs/guides/providers.md`**: all catalog providers listed, transport config docs

---

## Phase 5: Config Cleanup

**`crates/agentzero-config/src/model.rs`** + **`crates/agentzero-infra/src/runtime.rs`**:
- Audit `QueryClassificationConfig` and `EmbeddingRoute` wiring
- Add `tracing::warn!` for enabled-but-empty classification rules
- Add `tracing::warn!` for routes referencing uncatalogued providers
- 4 new validation tests

---

## Verification

```sh
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace  # target: 1,660+ tests, 0 failures
curl http://127.0.0.1:<port>/metrics  # real counters
curl http://127.0.0.1:<port>/v1/models  # full catalog
```
