# AgentZero Sprint Plan

## Sprint 23: Production Readiness & Observability

**Goal:** Make the gateway deployable with real metrics, harden WebSocket handling, fill critical test coverage gaps in storage and tools, add provider tracing, and update site documentation.

**Baseline:** 16-crate workspace, 1,595 tests passing, 0 clippy warnings, clean `main` branch.

Previous sprints archived to `specs/sprints/21-22-structured-tool-use-streaming-hardening.md`.

---

### Phase 1: Gateway Production Readiness (P0)

- [ ] **Real Prometheus metrics** — Add `metrics` + `metrics-exporter-prometheus` crates; create `gateway/src/metrics.rs` with counters (`requests_total{method,path,status}`, `errors_total{error_type}`, `ws_connections_total`) and histogram (`request_duration_seconds{method,path}`); add `PrometheusHandle` to `GatewayState`; replace hardcoded `/metrics` stub in `handlers.rs:42` with `handle.render()`; add `request_metrics` middleware; wire into `build_router()`
- [ ] **Dynamic `/v1/models`** — Replace hardcoded 2-model list in `handlers.rs:296-310` with `supported_providers()` + `find_models_for_provider()` from `agentzero-providers` catalog; change `ModelItem` fields from `&'static str` to `String`
- [ ] **WebSocket hardening** — In `handlers.rs` `handle_socket()`: add ping/pong heartbeat (30s interval, 60s timeout), reject `Message::Binary` with error frame, add 5min idle timeout via `tokio::select!`
- [ ] **Structured error types** — Define `GatewayError` enum in `models.rs` (`AuthRequired`, `AuthFailed`, `AgentUnavailable`, `AgentExecutionFailed`, `RateLimited`, `PayloadTooLarge`, `BadRequest`); impl `IntoResponse` returning JSON `{"error":{"type":"...","message":"..."}}` with distinct status codes; migrate handlers from `Result<_, StatusCode>` to `Result<_, GatewayError>`
- [ ] **Default rate limit** — Change `MiddlewareConfig::default()` `rate_limit_max` from `0` to `600` (10 req/s over 60s window); users can set `0` in config to disable
- [ ] **Tests** (14 new): metrics returns real counters after requests, request duration histogram populated, error counter increments on 4xx/5xx, all expected metric names present, catalog models appear in `/v1/models`, model IDs match catalog, binary WebSocket frame gets error response, heartbeat ping sent, idle timeout fires, `AgentUnavailable` → 503 JSON, `AuthRequired` → 401 JSON, `BadRequest` → 400 JSON, `AgentExecutionFailed` → 500 JSON, default rate limit 600 then reject

### Phase 2: Test Coverage Expansion (P0)

**agentzero-storage** (19 → 45+ tests):

- [ ] **Crypto tests** (`crypto/symmetric.rs`): empty plaintext round-trip, truncated ciphertext error, modified nonce error, invalid JSON envelope error, two encryptions produce different ciphertexts (nonce uniqueness)
- [ ] **Key tests** (`crypto/key.rs`): valid 64-char hex accepted, empty string rejected, wrong-length rejected, `from_config_dir` creates key file if missing
- [ ] **Store tests** (`store.rs`): save creates parent dirs, save+delete makes load return `None`, `load_or_default` with missing file, corrupt file returns error, concurrent save doesn't corrupt
- [ ] **Queue tests** (`queue.rs`): dequeue nonexistent ID is no-op, `len()` correct after enqueue/dequeue, complex payload round-trips through drain
- [ ] **SQLite memory tests** (`memory/sqlite.rs`): `recent(0)` returns empty, large limit returns all, 10KB content round-trips, Unicode/emoji content round-trips

**agentzero-tools** (targeted gaps):

- [ ] **task_plan.rs**: empty plan creation, multi-step serialize/deserialize, status transitions, invalid input error
- [ ] **cron_tools.rs**: valid cron expression parsing, invalid expression rejected, list when empty, remove nonexistent job is no-op
- [ ] **agents_ipc.rs**: empty body error, unknown agent error, serialization round-trip
- [ ] **cron_store.rs**: persistence round-trip, corrupted file returns error

### Phase 3: Provider Observability (P1)

- [ ] **Tracing spans** — Add `#[tracing::instrument(skip(self), fields(provider, model))]` to `chat()` and `chat_streaming()` in `anthropic.rs` and `openai.rs`; `info!` on request start/completion with latency_ms and token counts; `warn!` on retry; `error!` on final failure
- [ ] **Transport logging upgrade** — In `transport.rs`, upgrade `trace!`/`debug!` to `info!`/`warn!` for request/response/retry logging
- [ ] **Circuit breaker visibility** — Add `info!` log on state transitions (closed→open, open→half-open, half-open→closed); expose `CircuitBreakerStatus` struct with `state_label()` and `failure_count()`
- [ ] **Tests** (4 new): provider instrument compiles, transport log fields correct, circuit breaker status reflects state, state label correct after transitions

### Phase 4: Site Documentation Updates (P1)

- [ ] **Gateway docs** (`site/src/content/docs/reference/gateway.md`): add metrics endpoint section (counters, histograms, Prometheus scrape config), update `/v1/models` to note dynamic catalog, add WebSocket heartbeat/idle timeout docs, add structured error response format with examples, note default rate limit
- [ ] **Architecture docs** (`site/src/content/docs/architecture.md`): update crate count to 16, add observability section (tracing spans, metrics, circuit breaker)
- [ ] **Threat model** (`site/src/content/docs/security/threat-model.md`): add WebSocket abuse entry (binary injection, connection flooding), add rate limiting/DOS entry, update mitigations to reference Prometheus metrics
- [ ] **Provider guide** (`site/src/content/docs/guides/providers.md`): ensure all catalog providers listed, document transport config (timeout, retries, circuit breaker)

### Phase 5: Config Cleanup (P2)

- [ ] **Dead config audit** — Trace `QueryClassificationConfig` through `runtime.rs` → agent loop to confirm enforcement; trace `EmbeddingRoute` through `routing.rs` → runtime; add `tracing::warn!` when classification enabled with no rules, or when routes reference uncatalogued providers
- [ ] **Config validation tests** (4 new): `QueryClassificationConfig` deserializes from TOML with rules, default has `enabled: false`, `EmbeddingRoute` deserializes with optional fields, validation warns on empty rules

---

### Acceptance Criteria

- [ ] `/metrics` returns real Prometheus counters that change with gateway traffic
- [ ] `/v1/models` returns all models from the provider catalog
- [ ] WebSocket handler sends ping, handles pong, rejects binary frames, times out idle connections
- [ ] Gateway error responses are structured JSON with `type`/`message` fields
- [ ] Default rate limit is 600 req/min (configurable to 0 to disable)
- [ ] `agentzero-storage` has 45+ tests (up from 19)
- [ ] `agentzero-tools` targeted modules have 3-4x their current test count
- [ ] Provider `chat()`/`chat_streaming()` produce tracing spans with provider, model, latency, token counts
- [ ] Circuit breaker state transitions are logged at `info!` level
- [ ] Site docs updated for gateway metrics, error types, WebSocket behavior, rate limits
- [ ] All quality gates pass: `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace`
- [ ] Total test count: 1,660+ (up from 1,595)
