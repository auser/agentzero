# AgentZero Sprint Plan

## Sprint 24: Private AI Production-Readiness

**Goal:** Make privacy features production-ready: wire gateway, fix security gaps, add Noise client, per-component privacy boundaries, metrics, integration tests, and documentation.

**Baseline:** Privacy Phases 1-6 complete (60 tests across 4 crates), gateway wiring not connected, no client-side encryption, no per-component boundaries.

---

### Phase 1: Critical Security Fixes (P0)

- [x] **Fix "full" mode semantics** — Change `"full"` to allow cloud providers through encrypted transport; only `"local_only"` blocks cloud. Update `build_provider_with_privacy()` condition, runtime provider selection, config doc comments, and tests.
- [x] **Remove Serialize from IdentityKeyPair** — Prevent accidental secret key serialization to JSON. Keep Deserialize for CLI reconstruction. Rewrite serialization test to test-only deserialization.
- [x] **Wire privacy into gateway startup** — In `run()`: read privacy config, create `NoiseSessionStore` + `NoiseKeypair`, create `RelayMailbox`, spawn key rotation task. Mode acts as preset (`encrypted` → noise auto-on; `full` → noise + sealed + rotation). Remove all `#[allow(dead_code)]` from privacy modules.
- [x] **Key rotation lifecycle** — Add `force_rotate()` and `next_rotation_at()` to `PrivacyKeyRing`. Add `--force` flag to CLI `rotate-keys`. Persist keyring to `KeyRingStore` after each rotation in the background task.

### Phase 2: Noise Protocol Client + Key Distribution (P0)

- [x] **Client-side Noise handshake** — New `noise_client.rs` in `agentzero-core/src/privacy/`: transport-agnostic `NoiseClientHandshake` + `NoiseClientSession` (6 tests including full client/server round-trip).
- [x] **GET /v1/privacy/info endpoint** — Returns gateway privacy capabilities (noise enabled, handshake pattern, public key, key fingerprint, sealed envelopes, relay mode).
- [x] **HTTP Noise transport** — New `noise_transport.rs` in `agentzero-providers`: `perform_noise_handshake()` for HTTP-level handshake, `NoiseHttpTransport` wraps reqwest with encrypt/decrypt, `build_provider_with_noise()` creates encrypted OpenAI-compatible provider.

### Phase 3: Security Hardening (P0)

- [x] **Sealed envelope replay protection** — `DashMap<[u8; 24], u64>` seen-nonces in `RelayMailbox`. Rejects duplicate nonces (HTTP 409). GC stale nonces alongside envelope GC. 3 new tests.
- [x] **Local provider URL enforcement** — In config `validate()`, rejects non-localhost base_urls when `mode = "local_only"` or `enforce_local_provider = true` (promoted warning to error). 1 new test.
- [x] **Network-level tool enforcement** — `load_tool_security_policy()` disables `http_request`, `web_fetch`, `web_search`, `composio` in `local_only` mode; enforces localhost-only domain allowlist. 1 new test.
- [x] **Plugin network isolation** — WASM plugin isolation forces `allow_network = false` when network tools are disabled (local_only mode).

### Phase 4: Per-Component Privacy Boundaries (P1) ✅

- [x] **`PrivacyBoundary` enum** — `boundary.rs` in `agentzero-core/src/privacy/` with `Inherit`, `LocalOnly`, `EncryptedOnly`, `Any` variants. `resolve()`, `allows_provider()`, `allows_network()`, `is_at_least_as_strict_as()`, `Display`. 11 tests.
- [x] **String-based privacy helpers** — `privacy_helpers.rs` in `common/` (no feature gate): `boundary_allows_provider()`, `boundary_allows_network()`, `resolve_boundary()`, `is_network_tool()`. 8 tests.
- [x] **Agent boundaries** — `privacy_boundary`, `allowed_providers`, `blocked_providers` on `DelegateAgentConfig`. `privacy_boundary` on `DelegateConfig` (core). Resolved against global mode in `build_delegate_agents()`. Provider kind enforcement in `validate_delegation()`. 4 tests.
- [x] **Tool boundaries** — `tool_boundaries: HashMap<String, String>` on `SecurityConfig`. Threaded to `AgentConfig`. In `execute_tool()`, resolves tool boundary against agent boundary and blocks network tools under `local_only`.
- [x] **Thread context** — `privacy_boundary` + `source_channel` on `ToolContext`. `privacy_boundary` + `tool_boundaries` on `AgentConfig`. Wired from config in `build_runtime_execution()`.
- [x] **Enforcement** — Provider selection (delegate validation), tool execution (agent.rs boundary check), plugin isolation (WASM network=false when local_only).
- [x] **Config validation** — Agent boundary values validated. Agent boundary can't be more permissive than global mode. Tool boundary values validated. 4 new config tests.

### Phase 5: Metrics + Integration Tests (P1) ✅

- [x] **Privacy Prometheus metrics** — 6 new metrics in `gateway_metrics.rs`: `noise_sessions_active` (gauge), `noise_handshakes_total{result}` (counter), `relay_mailbox_envelopes` (gauge), `relay_submit_total` (counter), `key_rotation_total{epoch}` (counter), `privacy_encrypt_duration_seconds` (histogram). Wired into noise handshake (step2), relay submit, key rotation task, and noise middleware (encrypt/decrypt timing).
- [x] **Integration test: Noise round-trip** — Full XX handshake over HTTP: step1 (client → e), step2 (client → s se), session_id returned. Plus error cases: invalid base64, unknown handshake_id, noise disabled → 503.
- [x] **Integration test: E2E encrypted request/response** — Full handshake → send GET /health with `X-Noise-Session` header → server encrypts response → client decrypts and verifies JSON content. Also tests plaintext passthrough without session header and unauthorized response for invalid session IDs.
- [x] **Integration test: relay round-trip** — Submit sealed envelope → poll → verify decoded payload. Replay rejection (duplicate nonce → 409). Relay disabled → 404. Empty mailbox → empty array.
- [x] **Integration test: privacy info** — `/v1/privacy/info` reports noise_enabled=true with public_key when configured, noise_enabled=false otherwise.
- [x] **Integration test: per-component enforcement** — Delegation validation rejects cloud provider with local_only boundary (4 core tests). Tool execution blocks network tools under local_only (agent.rs). Config validation rejects invalid/overly-permissive boundaries (4 config tests).

### Phase 6: Polish + Documentation (P2) ✅

- [x] **Update AGENTS.md** — Add product principles: delight, ease of use, security, developer experience.
- [x] **Privacy guide** — `site/src/content/docs/guides/privacy.md`: modes, per-component config, key rotation, Noise Protocol, sealed envelopes, metrics.
- [x] **Privacy config reference** — `site/src/content/docs/config/privacy.md`: all TOML options with defaults, validation rules, mode-as-preset table.
- [x] **Threat model update** — Added Privacy & Encryption Threats section: key management, transport security, sealed envelope security, boundary enforcement.

---

### Acceptance Criteria

- [x] Gateway starts with noise sessions, relay, and key rotation when `mode = "encrypted"` or `"full"`
- [x] Client can perform Noise handshake and send encrypted requests through gateway
- [x] `local_only` mode blocks all outbound network (tools, plugins, providers, URL access)
- [x] Per-component boundaries resolve correctly (child can never exceed parent)
- [x] Sealed envelope replay is rejected (duplicate nonces)
- [x] Prometheus `/metrics` includes privacy gauges and counters
- [x] Integration tests cover full encryption round-trip
- [x] All quality gates pass: `cargo test --workspace --features privacy`
- [x] Test count: 1,338 total workspace tests passing (266 core + 106 config + 102 gateway + others)

---

## Sprint 23: Production Readiness & Observability ✅

**Goal:** Make the gateway deployable with real metrics, harden WebSocket handling, fill critical test coverage gaps in storage and tools, add provider tracing, and update site documentation.

**Baseline:** 16-crate workspace, 1,595 tests passing, 0 clippy warnings, clean `main` branch.

Previous sprints archived to `specs/sprints/21-22-structured-tool-use-streaming-hardening.md`.

---

### Phase 1: Gateway Production Readiness (P0) ✅

- [x] **Real Prometheus metrics** — `gateway_metrics.rs` with counters (`requests_total`, `errors_total`, `ws_connections_total`), histogram (`request_duration_seconds`), gauge (`active_connections`); `PrometheusHandle` in `GatewayState`; `request_metrics` middleware wired into router
- [x] **Dynamic `/v1/models`** — Uses `supported_providers()` + `find_models_for_provider()` from catalog; `ModelItem` fields changed to `String`
- [x] **WebSocket hardening** — `tokio::select!` with 30s heartbeat ping, 60s pong timeout, 5min idle timeout; binary frame rejection with error JSON
- [x] **Structured error types** — `GatewayError` enum (8 variants) with `IntoResponse` returning `{"error":{"type":"...","message":"..."}}`. All handlers migrated from `Result<_, StatusCode>` to `Result<_, GatewayError>`
- [x] **Default rate limit** — Changed to 600 req/min (was 0/unlimited)
- [x] **Tests** (11 new): structured error JSON tests, catalog model validation, rate limit boundary test, metrics content-type, perplexity filter structured error. 67 gateway tests total.

### Phase 2: Test Coverage Expansion (P0) ✅

**agentzero-storage** (19 → 46 tests):
- [x] `symmetric.rs` +4: empty plaintext, truncated ciphertext, invalid envelope, nonce uniqueness
- [x] `key.rs` +4: hex accepted, empty rejected, wrong length rejected, auto-create key file
- [x] `store.rs` +5: parent dirs, save+delete, load_or_default, corrupt file, concurrent save
- [x] `queue.rs` +3: dequeue nonexistent, len tracking, nested payload
- [x] `sqlite.rs` +4: recent(0), large limit, large content, unicode/emoji

**agentzero-tools** (targeted gaps filled):
- [x] `task_plan.rs` +3: empty create, status transitions, invalid input
- [x] `cron_store.rs` +3: persistence round-trip, remove nonexistent, list empty
- [x] `agents_ipc.rs` +2: recv missing, message round-trip

### Phase 3: Provider Observability (P1) ✅

- [x] **Tracing spans** — `info_span!` with `Instrument` on all 8 provider methods (4 Anthropic, 4 OpenAI) with `provider` and `model` fields
- [x] **Transport logging upgrade** — `log_request`/`log_response` upgraded to `info!`, `log_retry` upgraded to `warn!`
- [x] **Circuit breaker visibility** — State transitions at `info!` level; new `CircuitBreakerStatus` struct with `state` and `failure_count` fields; exported from crate
- [x] **Tests** (+4): `circuit_breaker_status_reflects_state`, `circuit_breaker_status_after_recovery`, `state_label_correct_for_all_states`, `log_helpers_accept_expected_field_types`. 134 provider tests total.

### Phase 4: Site Documentation Updates (P1) ✅

- [x] **Gateway docs** — Added: Metrics section (counters, histograms, Prometheus scrape config), Models section (dynamic catalog), Error Responses table (8 error types), WebSocket Behavior section (heartbeat, idle timeout, binary rejection), updated rate limit default
- [x] **Architecture docs** — Added Observability section (tracing spans, metrics, circuit breaker)
- [x] **Threat model** — Added WebSocket Abuse section (binary injection, connection flooding, zombie connections), Denial of Service section (request flooding, large payloads, monitoring)
- [x] **Provider guide** — Expanded Transport Configuration with retry policy, circuit breaker behavior, and observability details

### Phase 5: Config Cleanup (P2) ✅

- [x] **Config audit** — Confirmed both `QueryClassificationConfig` and `EmbeddingRoute` are actively enforced at runtime via `ModelRouter`. Added `tracing::warn!` in `validate()` when classification enabled with empty rules, or when embedding routes have empty provider/model fields.
- [x] **Config validation tests** (+4): `query_classification_deserializes_with_rules`, `query_classification_default_has_enabled_false`, `embedding_route_deserializes_with_optional_fields`, `validation_warns_on_empty_classification_rules`. 100 config tests total.

---

### Acceptance Criteria

- [x] `/metrics` returns real Prometheus counters that change with gateway traffic
- [x] `/v1/models` returns all models from the provider catalog
- [x] WebSocket handler sends ping, handles pong, rejects binary frames, times out idle connections
- [x] Gateway error responses are structured JSON with `type`/`message` fields
- [x] Default rate limit is 600 req/min (configurable to 0 to disable)
- [x] `agentzero-storage` has 46 tests (up from 19)
- [x] `agentzero-tools` targeted modules have expanded test coverage
- [x] Provider methods produce tracing spans with provider and model fields
- [x] Circuit breaker state transitions are logged at `info!` level
- [x] Site docs updated for gateway metrics, error types, WebSocket behavior, rate limits
- [x] All quality gates pass: `cargo fmt`, `cargo clippy -D warnings`, `cargo test`
- [x] Test counts: gateway 67, storage 46, tools 251, providers 134, config 100
