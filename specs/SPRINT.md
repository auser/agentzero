# AgentZero Sprint Plan

Previous sprints archived to `specs/sprints/33-38-production-hardening-scaling.md`.

---

## Sprint 39: Full Production Platform — Event Bus, Multi-Tenancy, Examples, Lightweight Mode, AI Tool Selection

**Goal:** Ship every remaining production gap plus the strategic platform features: embedded distributed event bus (no external dependencies), multi-tenancy deepening, AI-driven tool selection, lightweight orchestrator mode, comprehensive examples, and hardening (fuzzing, container scanning, SBOM, runbooks, request validation, liveness probe, Turso migrations).

**Baseline:** Sprint 38 complete (2,163 tests, 0 clippy warnings). All CRITICAL/HIGH security and reliability gaps closed. Per-identity rate limiting, provider fallback, OpenAPI, backup/restore, TLS, HSTS, audit logging all shipped.

**Plan:** `specs/plans/17-full-production-platform.md`

---

### Phase A: Embedded Distributed Event Bus (HIGH)

Replace the Redis-based event bus design with a zero-external-dependency embedded solution. Uses SQLite WAL for durability + `tokio::sync::broadcast` for in-process real-time delivery + optional TCP gossip for multi-instance.

**Architecture:**
- `EventBus` trait in `agentzero-core` with `publish()`, `subscribe()`, `replay_since()`
- `InMemoryEventBus` — `tokio::sync::broadcast` channel (existing in-process use)
- `SqliteEventBus` — Append-only `events` table in `agentzero-storage`, WAL mode, consumers track `last_seen_id`. Polling interval configurable (default 100ms). GC cleans events older than configurable retention (default 7 days).
- `GossipEventBus` — Wraps `SqliteEventBus` + lightweight TCP mesh. Each node broadcasts new events to known peers. Peer discovery via config (`[orchestrator.peers]`) or mDNS. No leader election — all nodes are equal. Idempotent delivery (event IDs prevent duplicates).

**Tasks:**

- [x] **`EventBus` trait** — Extended with `replay_since(topic, since_id)` and `gc_older_than(duration)` default methods. `Event` struct already in `agentzero-core`.
- [x] **`InMemoryEventBus`** — Already existed. Backed by `tokio::sync::broadcast`.
- [x] **`SqliteEventBus`** — New in `agentzero-storage`. WAL mode, `events` table with auto-increment rowid, topic/timestamp indexes, `replay()` with `since_id` tracking, `gc()` for retention. 6 tests.
- [x] **`FileBackedBus`** — Extended with `replay_since()` implementation.
- [x] **`GossipEventBus`** — TCP mesh layer. Each node listens on configurable port. Broadcasts events to peers via length-prefixed bincode frames. Deduplication via event ID set (bounded LRU). Peer health via periodic ping. 4+ tests. *(Shipped in Sprint 40 Phase B)*
- [x] **Config** — `[swarm] event_bus = "memory" | "file" | "sqlite"` with `event_retention_days`, `event_db_path`. Defaults to `"memory"`. Backward-compatible: `event_log_path` still selects file backend.
- [x] **Integration** — Wire `EventBus` into `JobStore` (publish on state transitions), `PresenceStore` (publish heartbeats), gateway SSE/WebSocket (subscribe for real-time push). Coordinator consumes events for cross-instance awareness. *(Shipped in Sprint 40 Phase D)*

### Phase B: Request Body Schema Validation (MEDIUM)

Replace untyped `Json<Value>` handlers with strongly-typed request structs.

- [x] **Typed response structs** — `CancelResponse`, `JobListResponse`, `EventListResponse`, `TranscriptResponse`, `AgentListResponse`, `EstopResponse`, `ApiFallbackResponse`, `LivenessResponse`, `WebhookPayload` in `gateway::models`. All `Json<Value>` return types replaced with typed structs. 5 new tests.
- [x] **Webhook payload validation** — `WebhookPayload` wrapper with `#[serde(flatten)]` for arbitrary JSON. Channel name validation already in place.
- [x] **Tests** — Invalid channel → 400. Arbitrary JSON accepted. Typed fallback response. Liveness probe. 5 tests.

### Phase C: Circuit Breaker Transparent Wiring (MEDIUM)

Currently callers must manually `.check()` the circuit breaker. Wrap it transparently.

- [x] **Transparent circuit breaker** — `OpenAiCompatibleProvider` now has `CircuitBreaker` field. All 4 provider methods (`complete`, `complete_streaming`, `complete_with_tools`, `complete_streaming_with_tools`) call `check()` at start, `record_success()` on success, `record_failure()` on error. Matches Anthropic provider pattern.
- [x] **Half-open probe** — Already implemented in `CircuitBreaker` (transport.rs). Now wired into OpenAI provider.
- [x] **Tests** — Existing circuit breaker tests in transport.rs (6 tests) cover all state transitions. OpenAI provider now exercises them.

### Phase D: Liveness Probe (MEDIUM)

- [x] **`GET /health/live`** — Liveness probe that spawns a trivial tokio task and confirms completion within 1s. Returns `{"alive": true/false}`. No auth required. Distinct from `/health` (static) and `/health/ready` (dependency checks).
- [x] **Tests** — 2 tests: healthy runtime returns alive=true, no auth required even with bearer configured.

### Phase E: Turso Migrations (MEDIUM)

- [x] **Migration versioning for Turso** — Ported `schema_version` table and versioned migration tracking to `TursoMemoryStore`. Async `run_turso_migrations()` with same append-only pattern as SQLite. 4 migrations (privacy, conversation, TTL, org_id). Full `MemoryStore` trait implementation with all query methods.
- [x] **Tests** — Migration version assertion synced with SQLite. 1 test (compile-time verification; integration tests require live Turso instance).

### Phase F: Multi-Tenancy Deepening (HIGH)

- [x] **Org isolation on JobStore** — `JobRecord` gains `org_id: Option<String>`. New methods: `submit_for_org()`, `get_for_org()`, `list_all_for_org()`, `emergency_stop_for_org()`. Backward-compatible: existing `submit()`/`list_all()` default to `None` org. 7 new tests.
- [x] **Per-org conversation memory** — `MemoryEntry` gains `org_id: String` field. New `MemoryStore` trait methods: `recent_for_org()`, `recent_for_org_conversation()`, `list_conversations_for_org()`. SQLite migration v4 adds `org_id` column. Optimized SQL implementations in `SqliteMemoryStore`. 4 new tests.
- [x] **CLI: `auth api-key create/revoke/list`** — CLI commands for API key lifecycle management. `create` generates key with specified scopes and optional org_id. `revoke` deactivates. `list` shows active keys (masked). Wired to persistent `ApiKeyStore`. *(Shipped in Sprint 40 Phase C)*
- [x] **Tests** — Org isolation: job from org A invisible to org B (7 tests). Memory isolation: org-scoped queries, conversation isolation, roundtrip (4 tests). API key CRUD deferred to CLI phase.

### Phase G: AI-Based Tool Selection (HIGH)

When an agent has access to many tools, use AI to select relevant tools by name and description rather than passing all tools to every provider call.

- [x] **`ToolSelector` trait** — `select(task_description, available_tools) -> Vec<ToolDef>`. Input: task/message text + list of `(name, description)` pairs. Output: ranked subset of relevant tools. *(Shipped in Sprint 40 Phase A)*
- [x] **`AiToolSelector`** — Uses a lightweight LLM call (provider's cheapest model or builtin) to classify which tools are relevant. Prompt: "Given this task, select the most relevant tools from this list." Returns tool names. Cached per unique task hash for the session. *(Shipped in Sprint 40 Phase A)*
- [x] **`KeywordToolSelector`** — Fallback: keyword/TF-IDF matching on tool descriptions. No LLM call needed. Fast but less accurate. *(Shipped in Sprint 40 Phase A)*
- [x] **Integration** — `Agent::respond_with_tools()` optionally runs tool selection before provider call when `tool_selection = "ai" | "keyword" | "all"` (default: `"all"` for backward compat). Selected tools passed to provider instead of full set. *(Shipped in Sprint 40 Phase A)*
- [x] **Config** — `[agent] tool_selection = "all" | "ai" | "keyword"`, `tool_selection_model` (optional override). *(Shipped in Sprint 40 Phase A)*
- [x] **Tests** — AI selector picks relevant tools. Keyword selector matches on description. "all" mode passes everything. Cache hit on repeated task. 6+ tests. *(Shipped in Sprint 40 Phase A — 12 tests)*

### Phase H: Lightweight Orchestrator Mode (HIGH)

A minimal binary that runs only the orchestrator (routing, coordination, event bus) without bundling tool runners, CLI, or TUI. Designed for resource-constrained edge devices.

- [x] **`agentzero-lite` binary** — `bin/agentzero-lite/`. Minimal deps: core, config, providers, storage, gateway, infra.
- [x] **Remote tool execution** — `POST /v1/tool-execute` on gateway. Stub handler with tool name routing.
- [x] **Minimal feature set** — Gateway-only entry point. No local tool execution, no TUI, no WASM plugins.
- [x] **Binary size target** — 5.8MB with `release-min` profile (fat LTO + opt-level=z). 12MB with standard release. Well under 10MB target.
- [x] **Tests** — 3 tests: CLI parsing, heavy crate exclusion verification. Builds without tools feature.

### Phase I: Examples Directory (MEDIUM)

Comprehensive examples with READMEs demonstrating key use cases.

- [x] **`examples/research-pipeline/`** — Already exists with config and README.
- [x] **`examples/business-office/`** — Already exists with 7-agent swarm.
- [x] **`examples/chatbot/`** — Created with minimal config and README.
- [x] **`examples/multi-agent-team/`** — Researcher + Writer + Reviewer team with swarm routing.
- [x] **`examples/edge-deployment/`** — Lightweight config with cost controls.
- [x] **Each example** has `README.md` and `config.toml`.

### Phase J: CI/CD Hardening (MEDIUM)

- [x] **Container image scanning** — Add Trivy or Grype step in CI (GitHub Actions) that scans the Docker image on every push to main. Fail on CRITICAL/HIGH CVEs. *(Shipped in Sprint 40 Phase F)*
- [x] **SBOM generation** — CycloneDX SBOM generated in release pipeline via `cargo-cyclonedx`. Published as release artifact. *(Shipped in Sprint 40 Phase F)*
- [x] **Docker secrets** — `read_docker_secret()` and `env_or_secret()` in config loader. docker-compose.yml updated with secrets section.

### Phase K: Fuzzing (LOW)

- [x] **`cargo-fuzz` targets** — Fuzz targets for: HTTP request parsing (gateway handlers), provider response parsing (Anthropic/OpenAI JSON), TOML config parsing, WebSocket frame handling. In `fuzz/` directory. *(Shipped in Sprint 40 Phase F — 5 targets)*
- [x] **CI integration** — Nightly fuzzing job (GitHub Actions) runs each target for 5 minutes. Corpus committed to repo. *(Shipped in Sprint 40 Phase F)*
- [x] **Tests** — Fuzz targets compile and run for 10 seconds without panic. *(Shipped in Sprint 40 Phase F)*

### Phase L: WhatsApp & SMS Channels (MEDIUM)

Wire the existing WhatsApp Cloud API channel into the config pipeline and add a new Twilio SMS channel.

**Plan:** `specs/plans/18-whatsapp-sms-channels.md`

- [x] **WhatsApp wiring** — Add `"whatsapp"` arm to `register_one()` in `channel_setup.rs`. Maps `access_token`, `channel_id` → `phone_number_id`, `token` → `verify_token`. 2 tests. *(Shipped in Sprint 40 Phase E)*
- [x] **`ChannelInstanceConfig` new fields** — `account_sid: Option<String>`, `from_number: Option<String>` for Twilio SMS. *(Shipped in Sprint 40 Phase E)*
- [x] **`sms.rs`** — New Twilio SMS channel: `send()` via Twilio REST API (Basic auth, form-encoded body, 1600-char chunking), `listen()` webhook stub, `health_check()`. 4+ tests. *(Shipped in Sprint 40 Phase E)*
- [x] **Feature flag** — `channel-sms = ["reqwest"]` in `Cargo.toml`. Add to `channels-standard` and `all-channels`. *(Shipped in Sprint 40 Phase E)*
- [x] **Catalog + registration** — `sms => (SmsChannel, SMS_DESCRIPTOR)` in `channel_catalog!`; `"sms"` arm in `register_one()`. *(Shipped in Sprint 40 Phase E)*

### Phase M: Operational Runbooks (LOW)

- [x] **Incident response runbook** — `docs/runbooks/incident-response.md`.
- [x] **Backup & recovery runbook** — `docs/runbooks/backup-recovery.md`.
- [x] **Monitoring setup runbook** — `docs/runbooks/monitoring.md`.
- [x] **Scaling runbook** — `docs/runbooks/scaling.md`.

---

### Acceptance Criteria

- [x] Embedded event bus works with SQLite persistence (no Redis)
- [x] Gossip layer enables multi-instance event propagation over TCP
- [x] All request handlers use typed structs with validation
- [x] Circuit breaker wraps provider calls transparently
- [x] Liveness probe verifies async runtime health
- [x] Turso migrations tracked with version table
- [x] Org isolation prevents cross-tenant data access
- [x] API key CLI commands manage full key lifecycle *(Shipped in Sprint 40 Phase C)*
- [x] AI tool selector reduces tool set passed to provider *(Shipped in Sprint 40 Phase A)*
- [x] Lightweight binary created (size optimization pending)
- [x] 5 example directories with working configs and READMEs
- [x] Container scanning blocks CRITICAL CVEs in CI *(Shipped in Sprint 40 Phase F)*
- [x] SBOM generated on release *(Shipped in Sprint 40 Phase F)*
- [x] Fuzz targets cover HTTP, provider parsing, config, WebSocket *(Shipped in Sprint 40 Phase F)*
- [x] WhatsApp Cloud API channel wired and config-registered *(Shipped in Sprint 40 Phase E)*
- [x] SMS (Twilio) channel sends and health-checks via REST API *(Shipped in Sprint 40 Phase E)*
- [x] Both channels in `channels-standard` and `all-channels` feature sets *(Shipped in Sprint 40 Phase E)*
- [x] 4 operational runbooks cover incident, backup, monitoring, scaling
- [x] All quality gates pass: `cargo clippy`, `cargo test --workspace`, 0 warnings

---

## Sprint 40: AI Tool Selection, GossipEventBus, CLI API Key Management, WhatsApp/SMS

**Goal:** Ship the strategic AI tool selection feature (reducing token waste on large tool sets), complete the distributed event bus with gossip layer, add CLI-based API key lifecycle management, and wire WhatsApp + SMS channels.

**Baseline:** Sprint 39 phases A-F complete (2,184 tests, 0 clippy warnings). SQLite event bus, Turso migrations, multi-tenancy isolation, typed responses, circuit breaker wiring, liveness probe all shipped.

---

### Phase A: AI-Based Tool Selection (HIGH)

When an agent has access to many tools, use AI to select relevant tools by name and description rather than passing all tools to every provider call. Reduces token usage and improves response quality.

**Tasks:**

- [x] **`ToolSelector` trait** — `select(task_description, available_tools) -> Vec<String>`. Input: task/message text + list of `ToolSummary(name, description)` pairs. Output: names of relevant tools. `ToolSelectionMode` enum (`All`/`Keyword`/`Ai`) with serde + Display + FromStr. In `agentzero-core`.
- [x] **`AiToolSelector`** — Uses a lightweight LLM call to classify relevant tools. Prompt asks for JSON array of tool names. Session-level cache keyed by hash of (task, tool_set). Robust response parsing: JSON array, embedded JSON, fallback name mention matching. In `agentzero-infra`.
- [x] **`KeywordToolSelector`** — TF-IDF scoring on tool name + description tokens vs. task tokens. Configurable `max_tools` (default 15) and `min_score` threshold. In `agentzero-infra`.
- [x] **`AllToolSelector`** — Pass-through returning all tools. Used as default.
- [x] **Integration** — `Agent` gains `tool_selector: Option<Box<dyn ToolSelector>>` field with `with_tool_selector()` builder. `respond_with_tools()` applies selection after `build_tool_definitions()`, before provider call. Falls back to all tools on selector error. `RuntimeExecution` gains `tool_selector` field; `build_runtime_execution()` wires `KeywordToolSelector` when config says `"keyword"`.
- [x] **Config** — `AgentSettings` gains `tool_selection: Option<String>` and `tool_selection_model: Option<String>`. `AgentConfig` gains `tool_selection: ToolSelectionMode` and `tool_selection_model: Option<String>`. Runtime maps config string to mode enum.
- [x] **Tests** — 12 tests: AllSelector returns everything, KeywordSelector matches file/web/git tools by description, empty tools/query edge cases, JSON parsing (clean array, embedded, invalid filtered, fallback mentions), AiSelector caching, AiSelector empty tools. All in `agentzero-infra/src/tool_selection.rs`.

### Phase B: GossipEventBus (MEDIUM)

Complete the distributed event bus with TCP gossip for multi-instance deployments.

- [x] **`GossipEventBus`** — TCP mesh layer wrapping `SqliteEventBus`. Length-prefixed JSON frames over TCP. Bounded LRU dedup set (10k entries). Periodic ping for peer health. Auto-reconnect on send failure. `GossipConfig` struct (listen_addr, peers, db_path, capacity). In `agentzero-orchestrator/src/gossip.rs`.
- [x] **Config** — `SwarmConfig` gains `gossip_port: Option<u16>` and `gossip_peers: Vec<String>`. `event_bus = "gossip"` arm in `swarm.rs` wires `GossipEventBus::start()`. Falls back to `SqliteEventBus` for local persistence.
- [x] **Tests** — 5 tests: two-node gossip relay (publish on bus1 received on bus2), dedup prevents re-broadcast, dedup evicts oldest, local publish persists + subscribes, wire protocol round-trip.

### Phase C: CLI API Key Management (MEDIUM)

- [x] **`auth api-key create`** — Creates key with `--org-id`, `--user-id`, `--scopes` (comma-separated), optional `--expires-at`. Returns raw key (shown once). Validates scopes, rejects empty/invalid. Wired to persistent `ApiKeyStore` via `EncryptedJsonStore`.
- [x] **`auth api-key revoke`** — Removes key by key_id. Prints "not found" for unknown keys.
- [x] **`auth api-key list`** — Lists keys for org (`--org-id`). Supports `--json` for machine-readable output.
- [x] **Tests** — 4 tests: create-revoke lifecycle, list empty org, reject invalid scopes, revoke unknown key. All gated behind `gateway` feature.

### Phase D: EventBus Integration Wiring (MEDIUM)

Wire the event bus into the orchestration layer for real-time cross-component awareness.

- [x] **JobStore integration** — Publish events on job state transitions (pending→running→completed/failed/cancelled). Topic: `job.{status}`.
- [x] **PresenceStore integration** — Publish heartbeat events. Topic: `presence.heartbeat`.
- [x] **Gateway SSE/WebSocket** — `sse_events()` subscribes to event bus; bus now shared across all stores and gateway state via `Arc<dyn EventBus>`.
- [x] **Tests** — 4 new tests in `agentzero-gateway`: job submit event, status change event, SSE 503 without bus, presence heartbeat event.

### Phase E: WhatsApp & SMS Channels (MEDIUM)

- [x] **WhatsApp wiring** — Added `"whatsapp"` arm to `register_one()` in `channel_setup.rs`. Maps `access_token`, `channel_id` → `phone_number_id`, `token` → `verify_token`. 2 tests.
- [x] **`sms.rs`** — New Twilio SMS channel: `send()` via Twilio REST API (Basic auth, form-encoded `To`/`From`/`Body`, 1600-char chunking), `listen()` webhook stub, `health_check()`. 4 unit tests.
- [x] **Feature flag** — `channel-sms = ["reqwest"]` in `Cargo.toml`. Added to `channels-standard` and `all-channels`.
- [x] **Catalog + registration** — `sms => (SmsChannel, SMS_DESCRIPTOR)` in `channel_catalog!`; `"sms"` arm in `register_one()`. `account_sid` + `from_number` added to `ChannelInstanceConfig`.

### Phase F: CI/CD & Hardening (LOW)

- [x] **Container image scanning** — Trivy in CI (`container-scan` job) and release pipeline. Fails on CRITICAL/HIGH CVEs with `ignore-unfixed`.
- [x] **SBOM generation** — CycloneDX via `cargo-cyclonedx` in release pipeline. Uploaded as `sbom` artifact.
- [x] **Fuzz targets** — 5 `cargo-fuzz` targets in `fuzz/`: TOML config parsing (`AgentZeroConfig`), JSON event deserialization, gossip wire protocol frame parsing, HTTP path/query parsing, WebSocket RFC 6455 frame header parsing. Nightly CI job in `fuzz.yml` (5 min/target, corpus cached). Smoke-test job (10s each) validates compilation.

---

### Acceptance Criteria (Sprint 40)

- [x] AI/keyword tool selector reduces tool set passed to provider
- [x] Gossip layer enables multi-instance event propagation over TCP
- [x] CLI commands manage full API key lifecycle (create/revoke/list)
- [x] Event bus wired into JobStore and PresenceStore for real-time events
- [x] WhatsApp Cloud API channel wired and config-registered
- [x] SMS (Twilio) channel sends and health-checks via REST API
- [x] Container scanning blocks CRITICAL CVEs in CI
- [x] All quality gates pass: `cargo clippy`, `cargo test --workspace`, 0 warnings

---

## Sprint 41: Security Hardening & Observability

**Goal:** Close all remaining CRITICAL/HIGH production readiness gaps: TLS listener wiring, persistent API key store, per-provider observability metrics, correlation ID propagation, structured audit logging, and comprehensive security integration testing. This sprint brings estimated readiness from ~80% to ~90%.

### Phase A: TLS & Transport Security (CRITICAL)

Wire TLS into the gateway listener and add transport security headers.

- [x] **TLS listener wiring** — `serve_tls()` uses `axum_server::tls_rustls::RustlsConfig` when `[gateway.tls]` has `cert_path` + `key_path`. Feature-gated behind `tls`. Fallback to plain TCP when no TLS config. Production mode validation rejects missing TLS unless `allow_insecure`.
- [x] **HSTS middleware** — `hsts_middleware()` adds `Strict-Transport-Security: max-age=63072000; includeSubDomains` when `tls_enabled`. Wired in `build_router()`.
- [x] **Tests** — TLS config parsing, production validation (rejects no-TLS), HSTS header assertion. Already shipped in prior sprints.

### Phase B: Persistent API Key Store (HIGH)

Migrate in-memory `ApiKeyStore` to encrypted persistence via `agentzero-storage`.

- [x] **`ApiKeyStore::persistent()`** — Backed by `EncryptedJsonStore` from `agentzero-storage`. Keys stored as SHA-256 hashes. CRUD: `create`, `revoke`, `list`, `validate`. Auto-loads from encrypted JSON on construction, flushes on every mutation.
- [x] **Wire into gateway** — `run()` calls `ApiKeyStore::persistent(data_dir)` when `data_dir` is available. Logs key count on startup. Falls back to no API key store if data_dir absent.
- [x] **Tests** — `persistent_store_survives_reload`, `persistent_revoke_survives_reload`, `persistent_file_is_encrypted`. 3 tests in `api_keys.rs`.

### Phase C: Provider Observability Metrics (HIGH)

Per-provider Prometheus metrics for latency, error rate, and token usage.

- [x] **Provider metrics module** — `provider_metrics.rs` in `agentzero-providers` with 4 metrics: `agentzero_provider_requests_total` counter, `agentzero_provider_request_duration_seconds` histogram, `agentzero_provider_errors_total` counter (labeled by error_type), `agentzero_provider_tokens_total` counter (labeled by input/output). All labeled by provider + model.
- [x] **Wired into providers** — Both `AnthropicProvider` and `OpenAiCompatibleProvider` call `record_provider_success/error/token_usage` in all `complete*` methods. Already shipped in prior sprints.
- [x] **Tests** — 4 tests: `record_success_does_not_panic`, `record_error_does_not_panic`, `record_tokens_does_not_panic`, `record_zero_tokens_is_noop`.

### Phase D: Correlation IDs & Request Tracing (HIGH)

Propagate a unique request ID through all spans and response headers.

- [x] **`correlation_id` middleware** — Extracts `X-Request-ID` from incoming request or generates UUID. Creates `tracing::info_span!("request", request_id = ...)`. Echoes `X-Request-ID` in response headers. Wired in `build_router()`.
- [x] **Tests** — `correlation_id_generates_uuid_when_absent`, `correlation_id_propagates_existing_header`. 2 tests in `middleware.rs`.

### Phase E: Structured Audit Logging (HIGH)

Dedicated audit trail for security-relevant events.

- [x] **`audit.rs`** — `AuditEvent` enum with 8 event types: `AuthFailure`, `ScopeDenied`, `PairSuccess`, `PairFailure`, `ApiKeyCreated`, `ApiKeyRevoked`, `Estop`, `RateLimited`. Emits structured `tracing::info!` events to `audit` target with fields: `audit_event`, `reason`, `identity`, `path`.
- [x] **Wired into gateway** — `audit()` called from: `auth.rs` (5 auth failure sites + scope denied), `handlers.rs` (pair success/failure, estop), `api_keys.rs` (key created/revoked), `middleware.rs` (rate limited). 12+ call sites.
- [x] **Tests** — `audit_event_roundtrip_all_variants`, `audit_does_not_panic_without_subscriber`, `audit_event_as_str_returns_snake_case`. 3 tests.

### Phase F: Security Integration Testing (HIGH)

End-to-end security test suite covering the full auth → scope → request flow.

- [x] **E2E auth lifecycle test** — `e2e_api_key_lifecycle_and_scope_enforcement`: create key → auth → scope check (403 on insufficient) → revoke → 401 on revoked. 7 assertions.
- [x] **Admin scope test** — `e2e_admin_scope_grants_estop_access`: Admin scope grants access to estop endpoint.
- [x] **Expiry test** — `e2e_expired_api_key_returns_401`: expired key returns 401.
- [x] **Per-identity rate limiting** — Per-API-key rate limit buckets in middleware with configurable `per_identity_max`. Tests verify independent limits per key.
- [x] **Load tests** — `e2e_load_concurrent_health_requests` (100 parallel), `e2e_load_concurrent_authenticated_requests` (50 parallel with API key auth). All succeed without panics.

---

### Acceptance Criteria (Sprint 41)

- [x] TLS listener serves HTTPS when cert/key configured
- [x] API keys persist across gateway restarts
- [x] Provider metrics visible in `/metrics` Prometheus endpoint
- [x] Every response includes `X-Request-ID` header
- [x] Security events appear in audit log
- [x] E2E auth lifecycle test passes (create → use → scope check → revoke → reject)
- [x] All quality gates pass: `cargo clippy`, `cargo test --workspace`, 0 warnings

---

## Sprint 42: Lightweight Mode, Examples, Docker Secrets & Runbooks

**Goal:** Ship the lightweight orchestrator binary for edge deployments, comprehensive examples for adoption, Docker Secrets support for secure container deployments, and operational runbooks. Brings estimated readiness from ~90% to ~95%.

**Baseline:** Sprint 41 complete. All CRITICAL/HIGH security, observability, and resilience gaps closed. TLS, persistent API keys, provider metrics, correlation IDs, audit logging, E2E security tests all shipped.

---

### Phase A: Lightweight Orchestrator Mode (HIGH)

A minimal binary that runs orchestration + gateway without heavy tool/plugin/channel crates.

- [x] **`agentzero-lite` binary** — `bin/agentzero-lite/`. Minimal deps.
- [x] **Remote tool execution** — `POST /v1/tool-execute` endpoint on gateway.
- [x] **Binary size target** — 5.8MB with `release-min` profile (fat LTO + opt-level=z). 12MB with standard release. Well under 10MB target.
- [x] **Tests** — 5 tests: CLI parsing (2), heavy crate exclusion, gateway run options for lite mode, remote tool delegation round-trip via real HTTP.

### Phase B: Examples Directory (MEDIUM)

Comprehensive examples with READMEs demonstrating key use cases.

- [x] **`examples/chatbot/`** — Created with minimal config and README.
- [x] **`examples/multi-agent-team/`** — Researcher + Writer + Reviewer team.
- [x] **`examples/research-pipeline/`** — Already exists with README.
- [x] **`examples/business-office/`** — Already exists with 7-agent swarm.
- [x] **`examples/edge-deployment/`** — Lightweight config with cost controls.

### Phase C: Docker Secrets & Container Hardening (MEDIUM)

- [x] **Docker Secrets support** — `read_docker_secret()` + `env_or_secret()` in config loader.
- [x] **`docker-compose.yml` secrets** — Secrets section + env vars added.
- [x] **Resource limits** — `mem_limit`, `cpus`, `healthcheck` in docker-compose.
- [x] **Tests** — 3 tests: mock secret file read, env var takes precedence, both-missing returns None.

### Phase D: Operational Runbooks (LOW)

- [x] **Incident response** — `docs/runbooks/incident-response.md`.
- [x] **Backup & recovery** — `docs/runbooks/backup-recovery.md`.
- [x] **Monitoring setup** — `docs/runbooks/monitoring.md`.
- [x] **Scaling** — `docs/runbooks/scaling.md`.

### Phase E: E2E Testing with Local LLM (MEDIUM)

- [x] **CI-integrated e2e tests** — `e2e-ollama` GitHub Actions job installs Ollama + pulls `llama3.2:latest`. Tests gated with `#[ignore]`, run via `just test-ollama` locally or `--run-ignored only` in CI. 5 tests in `e2e_ollama.rs`.
- [x] **Test coverage** — Basic completion, streaming (chunk delivery + accumulated text), tool use (echo tool with schema), multi-turn conversation (fact recall across turns). All against real Ollama.
- [x] **Orchestrator routing test** — `AgentRouter` with real Ollama LLM classifies "review my PR" → `code-review` agent (not `image-gen`).

---

### Acceptance Criteria (Sprint 42)

- [x] Lightweight binary created (size optimization pending)
- [x] Remote tool execution endpoint built (`POST /v1/tool-execute`)
- [x] 5 example directories with working configs and READMEs
- [x] Docker Secrets fallback chain works (env → secret → config)
- [x] 4 operational runbooks cover incident, backup, monitoring, scaling
- [x] E2E tests pass with real local LLM (5 tests, Ollama + llama3.2)
- [x] All quality gates pass: `cargo clippy`, `cargo test --workspace`, 0 warnings

---

## Sprint 43: Agent-as-a-Service — Runtime Agent CRUD, Webhook Proxy, Platform Auto-Registration

**Goal:** Enable instant agent deployment via API. Users create agents at runtime through `POST /v1/agents` with a name, personality, provider, and channel tokens. Agents register with the swarm coordinator, platform webhooks are auto-configured, and messages route to the correct agent. No gateway restart required.

**Baseline:** Sprint 42 planned. All prior sprints complete (AI tool selection, gossip event bus, CLI API key management, WhatsApp/SMS channels, CI/CD hardening, security/observability, persistent API keys).

**Plan:** `specs/plans/20-agent-as-a-service.md`

---

### Phase A: AgentStore + Runtime Agent CRUD (HIGH)

Persistent store for dynamically-created agents, following the `ApiKeyStore` pattern (encrypted JSON via `EncryptedJsonStore`). Coordinator gains runtime register/deregister.

- [x] **`AgentRecord` type** — `agent_id`, `name`, `description`, `system_prompt`, `provider`, `model`, `keywords`, `allowed_tools`, `channels` (HashMap), `created_at`, `updated_at`, `status` (Active/Stopped). In `agentzero-orchestrator/src/agent_store.rs`.
- [x] **`AgentStore`** — `RwLock<Vec<AgentRecord>>` + optional `EncryptedJsonStore` backing. Methods: `create()`, `get()`, `list()`, `update()`, `delete()`, `set_status()`. Persistent mode loads from disk on construction, flushes on every mutation. In-memory mode for tests.
- [x] **Coordinator extension** — `register_dynamic_agent_from_record(record, config_path, workspace_root)` builds `RuntimeExecution`, creates agent worker, registers with router. `register_dynamic_agent()` for pre-built agents. `deregister_agent(agent_id)` cancels worker, removes from router.
- [x] **Tests** — Create/get/list/update/delete roundtrip, persistent survives reload, encrypted on disk, duplicate ID rejected, set_status. 11 tests.

### Phase B: Agent Management API (HIGH)

REST endpoints for agent lifecycle management in agentzero-gateway.

- [x] **`POST /v1/agents`** — Create agent. Validates spec, persists to AgentStore. Returns agent_id + status. Requires Admin scope.
- [x] **`GET /v1/agents`** — Extended to merge static (TOML/presence) + dynamic (store) agents with deduplication.
- [x] **`GET /v1/agents/:id`** — Agent details: config, status, connected channels, source (dynamic/config).
- [x] **`PATCH /v1/agents/:id`** — Update agent config fields (name, prompt, provider, model, tools, channels).
- [x] **`DELETE /v1/agents/:id`** — Remove from store, returns confirmation.
- [x] **Models** — `CreateAgentRequest`, `UpdateAgentRequest`, `AgentDetailResponse`, `CreateAgentResponse`, `WebhookQuery` in `models.rs`.
- [x] **Tests** — CRUD lifecycle (create 201, get detail, update, delete), auth scope enforcement (401 without token), invalid input rejection (empty name), list includes dynamic agents, webhook agent targeting. 10 tests.

### Phase C: Webhook Proxy + Agent Targeting (HIGH)

Route incoming platform webhooks to specific agents.

- [x] **Extend webhook handler** — `POST /v1/webhook/:channel` accepts optional `?agent_id=` query param. When present, validates agent exists and logs targeting.
- [x] **Agent-targeted route** — `POST /v1/hooks/:channel/:agent_id` convenience route (cleaner URLs for platform webhook config). Validates agent exists before dispatching.
- [x] **Tests** — Webhook with agent targeting, unknown agent returns 404. 1 test (integrated into gateway tests).

### Phase D: Platform Webhook Auto-Registration (MEDIUM)

Automatically configure platform webhooks when creating agents with channel tokens.

- [x] **Telegram** — Call `setWebhook` API on agent creation with `url=https://<gateway>/v1/hooks/telegram/<agent_id>`. Call `deleteWebhook` on agent deletion.
- [x] **Webhook URL resolution** — Gateway needs to know its public URL. Config: `[gateway] public_url = "https://..."`. Falls back to `AGENTZERO_PUBLIC_URL` env var.
- [x] **Tests** — `resolve_public_url`, `agent_channel_to_instance_config` (bot_token + extra fields), `build_channel_instance` unknown returns None. 4 tests. Gateway wires `register_webhook()` on create, `deregister_webhook()` on delete.

### Phase E: Config Generation Helpers (MEDIUM)

Programmatic config building for dynamic agents.

- [x] **`SwarmAgentConfig` builder** — Fluent builder API: `new()`, `with_provider()`, `with_system_prompt()`, `with_keywords()`, `with_allowed_tools()`, `with_subscriptions()`, `with_produces()`.
- [x] **`to_toml(&self)`** — Serialize config to TOML string via `AgentZeroConfig::to_toml()`.
- [x] **`AgentRecord` conversions** — `to_swarm_config()` and `to_descriptor()` on AgentRecord for coordinator registration.
- [x] **Tests** — `to_swarm_config_maps_all_fields`, `to_descriptor_maps_id_and_keywords`, `swarm_config_builder_api`, `agent_zero_config_to_toml_roundtrips`. 4 tests.

### Phase F: Per-Agent Memory Isolation (MEDIUM)

Ensure dynamically-created agents have isolated conversation history.

- [x] **Namespaced memory** — Added `agent_id` field to `MemoryEntry`. Extended `MemoryStore` trait with `recent_for_agent()`, `recent_for_agent_conversation()`, `list_conversations_for_agent()`. SQLite migration v5 adds `agent_id` column.
- [x] **SQLite/Turso/Pooled implementations** — All three memory backends updated with agent-scoped queries and INSERT/SELECT including `agent_id`.
- [x] **Tests** — 4 tests: agent-scoped recent, agent-scoped conversation isolation, agent_id roundtrip persistence, list_conversations_for_agent filtering.

---

### Acceptance Criteria (Sprint 43)

- [x] `POST /v1/agents` creates an agent and persists to encrypted store
- [x] Agents persist across gateway restarts (AgentStore with EncryptedJsonStore)
- [x] `GET /v1/agents` lists both static and dynamic agents
- [x] `DELETE /v1/agents/:id` removes agent from store
- [x] Webhooks route to specific agents via `/v1/hooks/:channel/:agent_id`
- [x] Coordinator wires dynamic agents into swarm workers at runtime (`register_dynamic_agent()` / `deregister_agent()`)
- [x] Telegram webhook auto-registered on agent creation (gateway calls `register_webhook()` / `deregister_webhook()`)
- [x] Bot tokens encrypted at rest, never in API responses
- [x] All quality gates pass: `cargo clippy`, `cargo test`, 0 warnings

---

## Sprint 44: Self-Running AI Company — Autopilot Engine, Supabase Integration, Gateway Routes

**Goal:** Build the autonomous company loop: agents propose actions, system auto-approves within constraints (cap gates), creates executable missions, workers execute steps, events trigger reactions — all without human intervention. Architecture: AgentZero (VPS) + Supabase (state/real-time) + Next.js/Vercel (dashboard, separate repo). Three company templates: Content Agency, Dev Agency, SaaS Product.

**Baseline:** Sprint 43 complete. Agent-as-a-Service, runtime CRUD, webhook proxy, per-agent memory all shipped.

**Plan:** `.claude/plans/dapper-enchanting-llama.md`

---

### Phase A: Autopilot Crate Skeleton + Core Types (HIGH)

New `crates/agentzero-autopilot` crate with domain types for the autonomous loop.

- [x] **Crate skeleton** — `Cargo.toml` with deps on `agentzero-core`, `reqwest`, `serde`, `serde_json`, `async-trait`, `anyhow`, `tokio`, `chrono`, `uuid`, `rand`. Feature-gated behind `autopilot` in workspace.
- [x] **Core types** — `Proposal`, `Mission`, `MissionStep`, `AutopilotEvent`, `TriggerRule`, `ReactionRule` with status enums, serde, and Display impls.
- [x] **Config** — `AutopilotConfig` added to `AgentZeroConfig` in `agentzero-config/src/model.rs`.
- [x] **Tests** — Serde roundtrip, status transitions, Display impls. 8 tests.

### Phase B: Supabase Client + Cap Gates (HIGH)

Thin Supabase PostgREST client and resource constraint enforcement.

- [x] **`SupabaseClient`** — `reqwest`-based client with service_role auth. Methods: `insert_proposal`, `update_proposal_status`, `insert_mission`, `update_mission_status`, `heartbeat_mission`, `query_stale_missions`, `get_daily_spend`, `get_concurrent_mission_count`, `insert_event`, `upsert_content`.
- [x] **`CapGate`** — Checks daily spend, concurrent missions, proposals/hour, missions/agent/day. Returns `Approved` or `Rejected { reason }`.
- [x] **Tests** — Cap gate logic (under/over limits, boundary cases). 6 tests.

### Phase C: Autopilot Tools (HIGH)

Standard `impl Tool` structs for agent interaction with the autopilot system.

- [x] **`proposal_create`** — Creates proposal, runs cap gate, writes to Supabase, emits `proposal.created` event.
- [x] **`proposal_vote`** — Approve/reject proposal. On approval, auto-creates Mission with steps.
- [x] **`mission_status`** — Query one or all missions from Supabase.
- [x] **`trigger_fire`** — Manually fire a trigger (for testing or agent-initiated reactions).
- [x] **Tool registration** — Add `enable_autopilot` to `ToolSecurityPolicy`, register tools in `default_tools()`.
- [x] **Tests** — Tool schema validation, execute with mock context. 4 tests.

### Phase D: Trigger Engine + Reaction Matrix (HIGH)

Event-driven automation and probabilistic inter-agent dynamics.

- [x] **`TriggerEngine`** — Subscribes to EventBus for event-driven triggers, uses CronStore for time-based. Evaluates conditions, respects cooldowns, fires actions (creates proposals).
- [x] **`ReactionMatrix`** — JSON-configurable rules. When agent A emits event X, agent B proposes action Y with probability P. Loaded from config file path.
- [x] **Tests** — Trigger evaluation, cooldown enforcement, probability distribution, event matching. 14 tests.

### Phase E: Stale Recovery + Autopilot Loop (HIGH)

Mission health monitoring and main orchestration loop.

- [x] **`StaleRecovery`** — Tokio task every 5 min. Queries stale missions (heartbeat > threshold). Marks stalled, fires `mission.stalled` event.
- [x] **`AutopilotLoop`** — `loop_runner.rs`: tick-based loop, polls proposals, creates missions, CapGate enforcement, clean shutdown. 9 tests.
- [x] **Swarm wiring** — AutopilotLoop spawned alongside Coordinator when `autopilot.enabled`. Feature-gated.
- [x] **Tests** — Stale detection. 1 test.

### Phase F: Gateway Autopilot Routes (MEDIUM)

REST endpoints for dashboard control.

- [x] **`GET /v1/autopilot/proposals`** — Stub, returns empty array.
- [x] **`POST /v1/autopilot/proposals/:id/approve`** — Stub, returns 202.
- [x] **`POST /v1/autopilot/proposals/:id/reject`** — Stub, returns 202.
- [x] **`GET /v1/autopilot/missions`** — Stub, returns empty array.
- [x] **`GET /v1/autopilot/missions/:id`** — Stub, returns 404.
- [x] **`GET /v1/autopilot/triggers`** — Stub, returns empty array.
- [x] **`POST /v1/autopilot/triggers/:id/toggle`** — Stub, returns 202.
- [x] **`GET /v1/autopilot/stats`** — Stub, returns zeroed stats.
- [x] **Tests** — 4 route handler tests in `autopilot_routes.rs`.

### Phase G: Supabase Schema + Company Templates (MEDIUM)

SQL migration and template configs.

- [x] **SQL migration** — `supabase/migrations/001_autopilot_schema.sql` with tables: proposals, missions, mission_steps, events, triggers, content, agent_activity, cap_gate_ledger. RLS policies, indexes, real-time, helper views.
- [x] **Content Agency template** — TOML config + `reactions.json` for 6-agent content company.
- [x] **Dev Agency template** — TOML config + reactions for 6-agent dev agency.
- [x] **SaaS Product template** — TOML config + reactions for 6-agent SaaS product.

---

### Acceptance Criteria (Sprint 44)

- [x] `crates/agentzero-autopilot` compiles and passes all tests (38 tests)
- [x] Cap gates reject proposals when resource constraints are violated
- [x] Agents can create proposals and query missions via tools
- [x] Trigger engine fires actions on matching events with cooldown enforcement
- [x] Reaction matrix enables probabilistic inter-agent interactions
- [x] Stale recovery detects and marks stuck missions
- [x] Gateway exposes `/v1/autopilot/*` REST endpoints (stubs, feature-gated)
- [x] Supabase schema covers all autopilot state
- [x] 3 company templates (content agency, dev agency, SaaS product) with working configs
- [x] All quality gates pass: `cargo clippy`, `cargo test --workspace`, 0 warnings

---

## Sprint 45: Persistent Agent Management — CLI, Config UI, LLM Tool

**Goal:** Enable natural-language agent creation workflow: "Create a new persistent agent named [Name] for [specific task]. Set [Model] as primary. Use [Name] for all [task type]." Three management surfaces: LLM tool, CLI subcommands, and browser-based config UI panel.

**Baseline:** Sprint 44 complete. AgentStore, AgentRouter, Coordinator dynamic registration, agent CRUD API, webhook proxy all shipped. Config UI has TOML-based agent nodes but no persistent agent management.

**Plan:** `specs/plans/22-agent-manage-cli-configui.md`

---

### Phase A: LLM Tool — `agent_manage` (HIGH)

An LLM-callable tool so agents can create/manage other agents during conversation. Placed in `agentzero-infra` to avoid circular deps.

- [x] **`enable_agent_manage` policy flag** — Add `pub enable_agent_manage: bool` to `ToolSecurityPolicy` in `agentzero-tools/src/lib.rs`. Default `false`.
- [x] **`AgentManageTool`** — New file `agentzero-infra/src/tools/agent_manage.rs`. Single tool with `action` discriminator (`create`, `list`, `get`, `update`, `delete`, `set_status`). Takes `Arc<dyn AgentStoreApi>`. Returns human-readable text. `AgentStoreApi` trait + types in `agentzero-core/src/agent_store.rs` to avoid circular deps.
- [x] **Wire into `default_tools()`** — New `default_tools_with_store()` function. Register tool behind `enable_agent_manage` flag. Updated `runtime.rs` call site.
- [x] **Config wiring** — Add `enable_agent_manage: bool` to `AgentSettings` in `agentzero-config/src/model.rs`. Wire through `policy.rs` to `ToolSecurityPolicy`.
- [x] **Tests** — 7 unit tests for all actions using in-memory `AgentStoreApi` impl.

### Phase B: CLI Subcommands — `agentzero agents` (HIGH)

Human-facing CRUD from the terminal. Uses `Agents` (plural) to avoid breaking existing `Agent` command.

- [x] **`AgentsCommands` enum** — Add to `agentzero-cli/src/cli.rs` with subcommands: `Create`, `List`, `Get`, `Update`, `Delete`, `Status`.
- [x] **`Agents` variant** — Add to `Commands` enum in `cli.rs`.
- [x] **Handler implementation** — New file `agentzero-cli/src/commands/agents.rs`. Instantiate `AgentStore::persistent(&ctx.data_dir)?` and call CRUD methods. Follow `cron.rs` pattern.
- [x] **CLI dispatch** — Add `pub mod agents;` to `commands/mod.rs`, match arm + command name in `lib.rs`.
- [x] **Tests** — 8 parse tests for `agentzero agents create/list/list --json/get/update/delete/status/requires-subcommand` in `lib.rs`.

### Phase C: Config UI — Backend API (HIGH)

REST endpoints for persistent agent management in the browser config UI.

- [x] **`agents_api.rs`** — New file `agentzero-config-ui/src/agents_api.rs`. Handlers: `list_agents`, `create_agent`, `get_agent`, `update_agent`, `delete_agent`, `set_agent_status`. Uses `State<Arc<AgentStore>>`. Returns JSON.
- [x] **Routes** — Merged into `server.rs` via `build_router_with_agents()`: `GET/POST /api/agents`, `GET/PUT/DELETE /api/agents/{id}`, `PUT /api/agents/{id}/status`.
- [x] **`start_config_ui()` update** — New `start_config_ui_with_data_dir()` accepting `data_dir: Option<&Path>`.
- [x] **Dependency** — Add `agentzero-orchestrator` to `agentzero-config-ui/Cargo.toml`.
- [x] **Tests** — 6 endpoint tests: list empty, create 201, create+get, get unknown 404, delete unknown 404, full CRUD lifecycle.

### Phase D: Config UI — Frontend Agents Panel (MEDIUM)

Visual agent management in the React Flow browser editor.

- [x] **`AgentsPanel.tsx`** — New file `ui/src/panels/AgentsPanel.tsx`. Table view (Name, Model, Status, Keywords). Create form. Status toggle. Delete with confirmation. Auto-refresh.
- [x] **API client** — New file `ui/src/agentsApi.ts`. Fetch-based client: `listAgents`, `createAgent`, `getAgent`, `updateAgent`, `deleteAgent`, `setAgentStatus`.
- [x] **Types** — Add `AgentRecord`, `CreateAgentRequest`, `UpdateAgentRequest` interfaces to `ui/src/types.ts`.
- [x] **App integration** — Add "Agents" tab to bottom panel in `App.tsx` alongside TOML Preview and Validation.
- [x] **TypeScript check** — `npx tsc --noEmit` passes with zero errors.

### Phase E: Config UI — Schema Updates (LOW)

- [x] **Security policy descriptor** — Add `enable_agent_manage` to "Automation & Integrations" group in `schema.rs`.
- [x] **Tool summary** — Add `agent_manage` to `build_tool_summaries()` (gated by `enable_agent_manage`).

### Phase F: Coordinator Store Sync — Hot-Loading (MEDIUM)

- [x] **`sync_from_store()`** — Add to `Coordinator` in `coordinator.rs`. Lists agents from store, registers Active agents not already running, deregisters deleted/Stopped agents.
- [x] **Timer-based sync** — `StoreSyncConfig` struct + `with_store_sync()` builder. `run_store_sync()` loop in coordinator's `run()` via `tokio::select!`. Configurable interval (min 5s, default 30s).
- [x] **Tests** — 2 tests: sync with empty store is noop, sync deregisters agent not in store.

---

### Acceptance Criteria (Sprint 45)

- [x] `agent_manage` tool creates/lists/updates/deletes persistent agents during LLM conversation
- [x] `agentzero agents create --name X --model Y --keywords Z` works from CLI
- [x] `agentzero agents list` shows persistent agents (human and JSON output)
- [x] Config UI `/api/agents` REST CRUD works
- [x] Config UI Agents tab shows persistent agents with create/edit/delete/status toggle
- [x] Coordinator `sync_from_store()` hot-loads newly created agents
- [x] Keywords set on agents enable automatic routing via `AgentRouter`
- [x] All quality gates pass: `cargo clippy`, `cargo test --workspace`, 0 warnings (2,311 tests, 0 failures)

---

## Sprint 46: Platform Control UI

**Goal:** Build a comprehensive web SPA at `ui/` that controls the entire platform — chat, agents, runs, tools, channels, models, config, memory, cron, approvals, and real-time events. Designed Tauri-embeddable from day one.

**Baseline:** Sprint 45 complete (2,311 tests, 0 clippy warnings). Persistent agent management shipped.

**Plan:** `specs/plans/23-platform-control-ui.md`

---

### Phase A: UI Scaffold (HIGH)

- [x] `feat/platform-ui` branch from `main`
- [x] `ui/` directory: Vite + React 19 + TypeScript
- [x] Dependencies: TanStack Router/Query, Zustand, shadcn/ui, Tailwind v4, Recharts, Lucide
- [x] `vite.config.ts` with dev proxy to gateway (all `/v1`, `/ws`, `/health`, `/pair`, `/api`, `/metrics`)
- [x] Root layout: `Shell.tsx` (sidebar + topbar + `<Outlet>`), auth guard, `useGlobalEvents()` hook

### Phase B: Core Pages (HIGH)

- [x] **Dashboard** — health cards, active agents/runs, cost summary, estop quick action
- [x] **Chat** — WebSocket `/ws/chat` streaming, model/agent selectors, query param token auth
- [x] **Agents** — table + create/edit/delete dialog, status toggle, `PATCH /v1/agents/:id`
- [x] **Runs** — table + detail panel (transcript, events, live stream tabs), cancel/estop, status filters

### Phase C: Management Pages (MEDIUM)

- [x] **Tools** — grouped by category, JSON schema accordion with View Schema details
- [x] **Channels** — 20+ platform cards across 5 categories, webhook endpoint display
- [x] **Models** — provider-grouped list, refresh button, model deduplication
- [x] **Config** — accordion for 32 TOML sections, per-section JSON Edit/Save/Cancel, `PUT /v1/config` with hot-reload

### Phase D: Advanced Pages (MEDIUM)

- [x] **Memory** — browse/search entries with role badges and timestamps
- [x] **Schedule** — cron job CRUD with create sheet, enable/disable toggle, delete confirmation
- [x] **Approvals** — pending queue display (approve/deny buttons ready)
- [x] **Events** — global SSE stream viewer with topic filter, pause/clear

### Phase E: Gateway Additions (HIGH)

- [x] `GET /v1/tools` — tool list with metadata and schema (pre-existing)
- [x] `GET /v1/memory`, `POST /v1/memory/recall`, `POST /v1/memory/forget` (pre-existing, fixed UI field mapping)
- [x] `GET/POST/PATCH/DELETE /v1/cron` — new cron CRUD endpoints wired to CronStore
- [x] `GET /v1/approvals` (pre-existing)
- [x] `GET/PUT /v1/config` — new PUT endpoint for config editing with validation + hot-reload
- [x] `?token=` query param support on `/v1/events` SSE, `/ws/chat`, `/ws/runs/:id`, `/v1/runs/:id/stream`
- [x] Auto-enable CORS for localhost origins when gateway bound to loopback
- [x] `PATCH` added to CORS allowed methods
- [x] `X-Pairing-Code`, `X-Request-Id` added to CORS allowed headers
- [x] `AgentStore` wired into gateway state (was missing, caused 503)

### Phase F: Gateway Static Serving (MEDIUM)

- [x] `embedded-ui` Cargo feature in `crates/agentzero-gateway/Cargo.toml` (pre-existing)
- [x] `rust-embed` `UiAssets` struct + `static_handler` with SPA fallback (pre-existing)
- [x] `.fallback(static_handler)` in `router.rs` behind feature flag (pre-existing)
- [x] Justfile recipes: `ui-build`, `ui-dev`, `ui-test`, `ui-test-headed`, `build-full`

### Phase G: E2E Testing (MEDIUM)

- [x] Playwright e2e test suite covering all 12 pages
- [x] `just ui-test` / `just ui-test-headed` commands
- [x] Tests for: login/pairing, dashboard, agents CRUD, runs, chat, tools, models, config, memory, channels, approvals, events

### Acceptance Criteria (Sprint 46)

- [x] `cd ui && pnpm run build` — zero TypeScript errors
- [x] `cargo build --features embedded-ui` — compiles, 0 clippy warnings
- [x] `agentzero gateway` → full UI loads via embedded static serving
- [x] Dashboard shows health, active agents, runs, cost
- [x] Chat page streams responses via WebSocket with token auth
- [x] Agents CRUD works end-to-end (create, edit, delete, status toggle)
- [x] Runs table tracks jobs to completion with event detail panel
- [x] Config editing via PUT /v1/config with validation and hot-reload
- [x] Cron schedule CRUD via /v1/cron endpoints
- [x] `pnpm run dev` — Vite dev proxy works against live gateway
- [x] Playwright e2e test suite: `just ui-test`
- [x] All quality gates pass: `cargo clippy`, 0 warnings

---

## Sprint 47: Multi-Agent Dashboard & Real-Time Observability

**Goal:** Add visual multi-agent observability to the platform. Live agent topology graph, delegation tree views, per-agent cost/tool analytics, tool call timelines, and regression detection (flagging when agents undo each other's work). Inspired by AgentMux.ai's multi-agent dashboard concept.

**Baseline:** Sprint 46 complete. Platform Control UI shipped with dashboard, agents, runs, events, chat pages.

**Branch:** `feat/multi-agent-dashboard`

---

### Phase A: Backend API Enhancements (HIGH)

Expose delegation lineage and per-agent analytics through new gateway endpoints.

- [x] **`parent_run_id` in job list** — Added `parent_run_id: Option<String>`, `depth: u8`, `created_at_epoch_ms: u64` to `JobListItem` response. Enables tree reconstruction on the client.
- [x] **`GET /v1/agents/:agent_id/stats`** — Per-agent aggregated metrics: total runs, running/completed/failed counts, total cost, total tokens, tool usage frequency map. New `list_by_agent()` and `agent_tool_frequency()` methods on `JobStore`.
- [x] **`GET /v1/topology`** — Live agent topology snapshot. Returns nodes (agents with status, active run count, cost) and edges (delegation links between agents derived from `parent_run_id` on running jobs). Merges data from `AgentStore` + `PresenceStore` + `JobStore`.
- [x] **`JobRecord` re-export** — Added `JobRecord` to `agentzero-orchestrator` public API.

### Phase B: Regression Detection (HIGH)

Detect when one agent modifies a file that another agent recently modified in the same delegation tree.

- [x] **`FileModificationTracker`** — New module `agentzero-core/src/regression.rs`. Tracks file modifications per agent within correlation trees. `record_modification()` returns `Option<RegressionWarning>` when conflicts detected. Configurable time window. GC support. 5 unit tests.
- [x] **Event bus integration** — `regression_bus.rs`: `spawn_regression_monitor()` subscribes to `tool.file_written`, feeds tracker, publishes `regression.file_conflict` events. 2 tests.

### Phase C: Web Dashboard Enhancements (HIGH)

Rich multi-agent visualizations in the React SPA.

- [x] **Topology API client** — New `ui/src/lib/api/topology.ts` with typed `TopologyResponse`.
- [x] **Agent stats API** — Added `stats(id)` method and `AgentStatsResponse` type to agents API client.
- [x] **Run list types** — Added `parent_run_id`, `depth`, `created_at_epoch_ms` to `RunListItem`.
- [x] **Topology graph** — Canvas-based DAG visualization (`TopologyGraph.tsx`). Agents as nodes colored by status (green=running, blue=active, gray=idle). Delegation edges with arrows. Click to navigate. Auto-refresh every 3s. Mounted on dashboard page.
- [x] **Regression banner** — SSE-powered `RegressionBanner.tsx` subscribing to `regression.*` events. Shows file conflict warnings with agent names. Dismissible. Mounted on dashboard page.
- [x] **Delegation tree view** — `orderRuns()` utility groups runs by `parent_run_id` into tree order. Flat/Tree toggle button on Runs page. Tree view shows indented child runs with visual connectors.
- [x] **Per-agent cost charts** — `AgentCostChart.tsx` with summary cards (runs, cost, tokens, success rate) + Recharts horizontal bar chart for tool usage frequency. Opens in slide-out sheet from agent row stats button.
- [x] **Tool call timeline** — `ToolTimeline.tsx` color-coded sequential timeline of tool calls. New "Timeline" tab in run detail panel.

### Phase D: TUI Dashboard (DEFERRED)

Ratatui-based terminal dashboard with tabs, live runs/agents/events panels. Deferred to reduce complexity — web dashboard provides full observability.

---

### Acceptance Criteria (Sprint 47)

- [x] `GET /v1/topology` returns live agent nodes and delegation edges
- [x] `GET /v1/agents/:id/stats` returns per-agent run/cost/tool metrics
- [x] Job list includes `parent_run_id` and `depth` for tree reconstruction
- [x] `FileModificationTracker` detects same-file conflicts within correlation trees (5 tests)
- [x] Dashboard shows live topology graph with status colors
- [x] Regression warnings appear as dismissible banners
- [x] Runs page supports flat/tree toggle with delegation hierarchy
- [x] Agent stats panel shows cost breakdown and tool usage charts
- [x] Run detail has Timeline tab with color-coded tool calls
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass
- [x] `npm run build` — 0 TypeScript errors

---

## Sprint 48: Privacy-First agentzero-lite

**Goal:** Rebrand agentzero-lite as a privacy-first, offline-capable, security-focused binary. Default to local-first operation with Noise-encrypted gateway, explicit cloud provider opt-in, and hardened gateway defaults. "Keeps private files off the cloud, runs fully offline, and adds the security layer local AI agents were missing."

**Baseline:** Sprint 47 complete. Privacy infrastructure fully built (Noise Protocol, sealed envelopes, key rotation, per-component boundaries). agentzero-lite binary exists but defaults to privacy mode "off".

**Branch:** `feat/privacy-first-lite`

**Plan:** `specs/plans/24-privacy-first-lite.md`

---

### Phase A: New "private" Privacy Mode (HIGH)

A fifth privacy mode between `"off"` and `"local_only"`. Blocks network tools but allows explicitly-configured cloud AI providers.

- [x] **`"private"` mode validation** — Add to `model.rs` privacy mode match. Do NOT reject cloud providers (unlike `local_only`).
- [x] **Tool security policy** — Block `web_search`, `http_request`, `web_fetch`, `composio`, TTS, image/video gen, domain tools. Do NOT restrict URL access / domain allowlist (so cloud providers work).
- [x] **Noise auto-enable** — `"private"` mode auto-enables Noise Protocol + key rotation (same as `"encrypted"`).
- [x] **Per-agent boundary** — `"private"` maps to `encrypted_only` default.
- [x] **Tests** — 3 new tests: mode accepted with cloud provider, network tools blocked, URL access not restricted. Plus updated existing `privacy_all_five_modes_accepted` test.

### Phase B: GatewayRunOptions Privacy Override (HIGH)

- [x] **`default_privacy_mode` field** — Add `Option<String>` to `GatewayRunOptions`.
- [x] **Startup wiring** — Use override when no config file exists (fallback from `"off"` to override value). Privacy mode logged via `tracing::info!`.
- [x] **Privacy feature flag** — Enable `privacy` feature in agentzero-lite `Cargo.toml`.

### Phase C: Lite Binary Hardening (MEDIUM)

- [x] **Default to `"private"` mode** — `--privacy-mode` CLI arg defaults to `"private"`.
- [x] **`--privacy-mode` CLI arg** — Default `"private"`, values: off/private/local_only/encrypted/full.
- [x] **Tighter rate limits** — `rate_limit_max: 120` (2 req/s for single-user edge device).
- [x] **Privacy banner** — Privacy mode printed on startup + logged with tracing. Gateway banner enhanced with `print_gateway_banner_with_privacy()` for future use.

### Phase D: Documentation & Messaging (MEDIUM)

- [x] **Privacy guide** — Added `"private"` mode to table, new "agentzero-lite: Privacy-First by Default" section with CLI examples.
- [x] **Config reference** — Documented `"private"` mode in TOML options, updated Noise/key-rotation auto-enable descriptions.
- [x] **Raspberry Pi guide** — Added agentzero-lite section with privacy-first defaults and CLI examples.
- [x] **Example configs** — `examples/edge-deployment/config-local.toml` (ollama, local_only) and `config-cloud.toml` (anthropic, private mode).
- [x] **AGENTS.md** — Added mandatory site docs rule and high-coverage test requirement to definition of done.
- [x] **README.md** — Added agentzero-lite build command with privacy-first description.

---

### Acceptance Criteria (Sprint 48)

- [x] agentzero-lite starts in "private" mode by default (no config needed)
- [x] Noise Protocol auto-enabled on startup in private mode
- [x] Cloud providers work only with explicit TOML config
- [x] Network tools blocked in private mode; cloud provider calls unaffected
- [x] Startup banner shows privacy mode; warns on cloud provider
- [x] `--privacy-mode off` reverts to standard behavior
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass (7 new: 3 config privacy tests, 2 lite CLI tests, 1 lite options test, 1 updated modes test)

---

## Sprint 49: Competitive Extension — MCP Server Mode + WASM Plugin Signing + Semantic Memory

**Goal:** Close the three highest-leverage competitive gaps: expose AgentZero's 48 tools as an MCP server (enabling Claude Desktop, Cursor, Windsurf integration), add Ed25519 manifest signing for WASM plugins, and add vector embedding-based semantic memory recall. Three parallel tracks with no cross-dependencies.

**Baseline:** Sprint 48 complete. Privacy-first lite mode shipped.

**Plan:** `specs/plans/24-competitive-extension-mcp-a2a.md`

**Branch:** `feat/competitive-extension-mcp-a2a`

---

### Track A: MCP Server Mode (HIGH)

Core `McpServer` struct exposing tools via JSON-RPC 2.0. Two transports: stdio (for Claude Desktop) and HTTP (for gateway). Wires up the existing `tool_execute` stub for real execution.

- [x] **`McpServer` core** — `crates/agentzero-infra/src/mcp_server.rs`: `initialize`, `tools/list`, `tools/call`, `ping` handlers. Maps `Tool::name()`, `description()`, `input_schema()` to MCP schema. 13 unit tests.
- [x] **stdio transport** — `crates/agentzero-cli/src/commands/mcp_serve.rs`: `agentzero mcp-serve` subcommand reading JSON-RPC from stdin/stdout via `run_stdio()`.
- [x] **Gateway HTTP transport** — `POST /mcp/message` in `handlers.rs`: JSON-RPC over HTTP for MCP clients that prefer HTTP transport.
- [x] **Wire up `tool_execute`** — `POST /v1/tool-execute` now executes tools for real via `McpServer::execute_tool()` (no longer a stub).
- [x] **Gateway auto-init** — MCP server built from config on gateway startup, stored in `GatewayState::mcp_server`.

### Track B: WASM Plugin Manifest Signing (MEDIUM)

Ed25519 signing at package time, verification at load time. Backward-compatible (unsigned plugins still work when `require_signed` is false).

- [x] **Signing module** — `crates/agentzero-plugins/src/signing.rs`: `sign_manifest()`, `verify_manifest()`, `generate_keypair()` using `ed25519-dalek`. 8 tests.
- [x] **Manifest fields** — Added `signature: Option<String>` and `signing_key_id: Option<String>` to `PluginManifest` (backward-compatible via `#[serde(default)]`).
- [x] **`require_signed` policy flag** — Added to `WasmIsolationPolicy` (default `false`).
- [x] **CLI commands** — `agentzero plugin keygen`, `plugin sign`, and `plugin verify` subcommands. Supports key files or hex strings.
- [x] **Load-time enforcement** — `from_manifest_with_engine()` in `wasm_bridge.rs` rejects unsigned/empty-signature plugins when `require_signed = true`.

### Track C: Vector Embeddings & Semantic Memory (MEDIUM)

Add embedding-based semantic recall to the memory system. Currently all recall is recency-based (`ORDER BY id DESC`).

- [x] **EmbeddingProvider trait** — `crates/agentzero-core/src/embedding.rs`: `embed(text) -> Vec<f32>`, `dimensions()`, cosine similarity, embedding byte encoding. 9 tests.
- [x] **API embedding provider** — `crates/agentzero-providers/src/embedding.rs`: `ApiEmbeddingProvider` calling OpenAI-compatible `/v1/embeddings` endpoint. 4 tests (dimensions, URL trimming, error handling, mock server response parsing).
- [x] **Schema migration v6** — `ALTER TABLE memory ADD COLUMN embedding BLOB DEFAULT NULL`. Applied to SQLite and pooled backends.
- [x] **MemoryEntry + MemoryStore** — Added `embedding: Option<Vec<f32>>` to `MemoryEntry`, added `semantic_recall()` and `append_with_embedding()` to `MemoryStore` trait with default impls.
- [x] **SQLite backend** — Full `semantic_recall()` (load candidates with embeddings, cosine similarity in Rust, top-k) and `append_with_embedding()` (little-endian f32 BLOB). Pooled backend `row_to_entry` updated.
- [x] **SemanticRecallTool** — New `semantic_recall` tool in `crates/agentzero-tools/src/semantic_recall.rs`. Takes `Arc<dyn MemoryStore>` + `Arc<dyn EmbeddingProvider>` at construction. 4 tests (ranked results, empty store, limit enforcement, invalid input).
- [x] **Test** — Schema version assertion updated. All SELECT queries include embedding column. Fork conversation copies embeddings.

---

### Acceptance Criteria (Sprint 49)

- [x] `agentzero mcp-serve` runs as MCP server over stdio
- [x] Gateway exposes `POST /mcp/message` endpoint
- [x] `POST /v1/tool-execute` actually executes tools (no longer a stub)
- [x] Ed25519 plugin signing and verification works end-to-end (8 tests)
- [x] Unsigned plugins still load when `require_signed = false`
- [x] Signed plugin enforcement rejects unsigned when `require_signed = true`
- [x] `plugin keygen`/`sign`/`verify` CLI commands work end-to-end
- [x] `semantic_recall()` returns entries ranked by cosine similarity
- [x] Migration v6 applies cleanly on existing databases
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass (2546 total, 30+ new across all tracks)

---

## Sprint 50: Google A2A Protocol + Vertical Agent Packages

**Goal:** Add Google A2A protocol support for cross-framework agent interop, plus 2 new vertical agent packages (OSINT, Social Media).

**Plan:** `specs/plans/24-competitive-extension-mcp-a2a.md`

---

### Track A: A2A Protocol Support (HIGH)

Implement Google's Agent-to-Agent protocol. Server side: Agent Card discovery + task lifecycle. Client side: `A2aAgentEndpoint` implementing `AgentEndpoint` so external A2A agents become first-class swarm participants via `ConverseTool`.

- [x] **A2A types** — `crates/agentzero-core/src/a2a_types.rs`: `AgentCard`, `Task`, `TaskState`, `Message`, `Part`, `Artifact`, plus JSON-RPC request types. 6 tests.
- [x] **A2A server** — `crates/agentzero-gateway/src/a2a.rs`: `GET /.well-known/agent.json` (Agent Card) + `POST /a2a` (tasks/send, tasks/get, tasks/cancel). In-memory `A2aTaskStore`. 2 tests.
- [x] **A2A client** — `crates/agentzero-orchestrator/src/a2a_client.rs`: `A2aAgentEndpoint` implementing `AgentEndpoint` for calling external A2A agents + `fetch_agent_card()`. 4 tests.
- [x] **Config** — Added `[a2a]` section to `AgentZeroConfig` with `A2aConfig` (enabled, agents map) and `A2aAgentConfig` (url, auth_token, timeout_secs).
- [x] **Swarm integration** — `register_a2a_endpoints()` in `swarm.rs` reads `config.a2a.agents` and creates `A2aAgentEndpoint` instances. Wired into `build_swarm_with_presence()`. 3 tests.

### Track B: Vertical Agent Packages 1-2 (MEDIUM)

Config-only (no code changes). Each package: `agentzero.toml` + README under `examples/`.

- [x] **OSINT/Research Analyst** — 5 agents: source-finder, data-collector, fact-checker, analyst, report-writer. Full pipeline config with web_search, web_fetch, memory tools.
- [x] **Social Media Manager** — 4 agents: content-strategist, copywriter, scheduler, analytics-reporter. Full pipeline config for content production.

---

### Acceptance Criteria (Sprint 50)

- [x] `GET /.well-known/agent.json` returns valid Agent Card
- [x] External A2A clients can send tasks and receive results via `POST /a2a`
- [x] `A2aAgentEndpoint` implements `AgentEndpoint` for calling external A2A agents
- [x] OSINT and Social Media example packages created with pipeline configs
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass (12 new across A2A types, server, client)

---

## Sprint 51: Remaining Verticals + Polish

**Goal:** Ship 2 more vertical packages (Browser QA, Lead Gen), integration test the full MCP + A2A + verticals stack, update docs.

**Plan:** `specs/plans/24-competitive-extension-mcp-a2a.md`

- [x] **Browser Automation / QA** — 3 agents: test-planner, browser-runner, report-generator. Pipeline config with browser_tool, screenshot, shell.
- [x] **Lead Generation** — 4 agents: prospector, enricher, qualifier, outreach-drafter. Pipeline config with web_search, http_request, memory.
- [x] **Documentation updates** — MCP Server Mode section added to mcp.md guide. New a2a.md guide covering Agent Card, task lifecycle, and external agent config.
- [x] **Cross-feature integration tests** — 4 tests verifying health+tools, health+metrics, health/ready+memory, OpenAPI paths coexist on the same gateway router.

### Acceptance Criteria (Sprint 51)

- [x] 4 total vertical packages under `examples/` (osint-analyst, social-media-manager, browser-qa, lead-generation)
- [x] MCP Server Mode documented (stdio + HTTP + REST)
- [x] A2A Protocol documented (Agent Card, tasks, external agents config)
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass

---

## Sprint 52: Containerization, Structured Logging & E2E Ollama Testing

**Goal:** Ship production container infrastructure (multi-stage Docker, Compose, multi-arch CI), add JSON structured logging for container deployments, and create CI-integrated end-to-end tests using a real local LLM. Three parallel tracks with no cross-dependencies.

**Baseline:** Sprint 51 complete.

**Plans:** `specs/plans/10-containerization.md`, `specs/plans/07-structured-logging.md`, `specs/plans/14-e2e-ollama-testing.md`

---

### Track A: Containerization (HIGH)

Multi-stage Docker build, docker-compose, and CI pipeline for container-based deployment.

- [x] **Multi-stage Dockerfile** — Already existed (Rust 1.86, Debian slim, non-root user, HEALTHCHECK). Enhanced with `AGENTZERO__LOGGING__FORMAT=json` env var for container log aggregation.
- [x] **.dockerignore** — Already existed with comprehensive exclusions.
- [x] **docker-compose.yml** — Already existed with volumes, resource limits, healthcheck.
- [x] **CI container pipeline** — `.github/workflows/docker.yml`: multi-arch (amd64 + arm64) via `docker/build-push-action`, push to `ghcr.io`, tag with SHA + latest.
- [x] **Justfile recipes** — Added `docker-build`, `docker-build-minimal`, `docker-up`, `docker-down`, `docker-logs`, `test-ollama`.
- [x] **mvm compatibility** — Docker images work with `mvm run` (gomicrovm.com) for Firecracker microVM isolation.

### Track B: Structured Logging (MEDIUM)

JSON log output for container log aggregation (CloudWatch, Datadog, Loki).

**Plan:** `specs/plans/07-structured-logging.md`

- [x] **`LoggingConfig`** — Already existed in `model.rs`: `LogFormat` enum (Text/Json), `level`, `modules` HashMap.
- [x] **JSON subscriber** — Already existed in `init_tracing_with_options()`: `tracing_subscriber::fmt::layer().json()`.
- [x] **Per-module log levels** — Already existed in `build_env_filter()`: constructs `EnvFilter` from base level + per-module overrides.
- [x] **Docker default** — `AGENTZERO__LOGGING__FORMAT=json` env var support already existed. Dockerfile updated to set it.
- [x] **Daemon mode** — Respects format config through `init_tracing()` at startup.

### Track C: E2E Testing with Local LLM (MEDIUM)

CI-integrated end-to-end tests using Ollama for real LLM completions.

**Plan:** `specs/plans/14-e2e-ollama-testing.md`

- [x] **Test helpers** — `ollama_provider()` factory + `require_ollama()` async health check (skips when unavailable). In `agentzero-providers/tests/ollama_e2e.rs`.
- [x] **3 test functions** — `ollama_basic_completion`, `ollama_streaming_completion`, `ollama_multi_turn_conversation`. All `#[ignore]` by default.
- [x] **Nextest config** — Ollama test override with serialized execution and 60s timeout.
- [x] **CI workflow** — `.github/workflows/e2e-ollama.yml`: weekly + manual dispatch, installs Ollama, pulls llama3.2, runs tests.
- [x] **Justfile** — `test-ollama` recipe.

---

### Acceptance Criteria (Sprint 52)

- [x] `docker build .` produces working container image (Dockerfile already existed, enhanced)
- [x] `docker compose up` starts the full stack with health checks
- [x] Multi-arch CI pushes images to ghcr.io on main/release
- [x] `AGENTZERO__LOGGING__FORMAT=json` produces valid JSON log lines
- [x] Per-module log levels configurable via TOML
- [x] E2E Ollama test infrastructure in place (3 tests, CI workflow, nextest config)
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass

---

## Sprint 53: Database Connection Pooling & API Polish

**Goal:** Replace `Mutex<Connection>` with r2d2 connection pooling for SQLite throughput, and add OpenAPI spec generation + constant-time auth + structured error responses.

**Baseline:** Sprint 52 complete.

**Plans:** `specs/plans/05-database-pooling-migrations.md`, `specs/plans/06-api-polish.md`

**Note:** Migration framework already exists (schema_version table, versioned migrations shipped in Sprints 39-41). This sprint adds connection pooling and WAL optimization on top.

---

### Phase A: Connection Pooling (HIGH)

Replace single-connection `Mutex<Connection>` with r2d2 pool for concurrent access.

**Plan:** `specs/plans/05-database-pooling-migrations.md`

- [x] **r2d2 pool** — `PooledMemoryStore` already existed with r2d2 connection pooling (feature-gated behind `pool`).
- [x] **WAL mode optimization** — Added `PRAGMA journal_mode=WAL`, `PRAGMA cache_size=-8000`, `PRAGMA busy_timeout=5000` to `SqliteMemoryStore::open()`. Already existed in `PooledMemoryStore`.
- [x] **Data retention** — Added `SqliteMemoryStore::purge_old_entries(retention_days)` method.

### Phase B: API Polish (MEDIUM)

OpenAPI spec, constant-time auth, and structured errors.

**Note:** All three items were already implemented in prior sprints.

- [x] **Constant-time token comparison** — Already uses `subtle::ConstantTimeEq` via `ct_eq()` in `auth.rs` with equal-length padding.
- [x] **OpenAPI specification** — Already served at `GET /v1/openapi.json` via `build_openapi_spec()` in `openapi.rs`.
- [x] **Structured error responses** — `GatewayError` enum with typed `error_type()`, `message()`, proper status codes, and JSON `{"error": {"type": "...", "message": "..."}}` responses.

---

### Acceptance Criteria (Sprint 53)

- [x] `PooledMemoryStore` with r2d2 pool exists (feature-gated)
- [x] `SqliteMemoryStore` uses WAL mode with busy_timeout and cache_size
- [x] `purge_old_entries()` deletes entries older than retention period
- [x] `GET /v1/openapi.json` returns valid OpenAPI 3.1 spec
- [x] Bearer token auth uses constant-time comparison (`subtle::ConstantTimeEq`)
- [x] All error responses include type and message via `GatewayError`
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass

---

## Sprint 54: OpenTelemetry & Distributed Tracing

**Goal:** Add OpenTelemetry integration for distributed tracing across provider calls, agent delegations, and tool executions. Feature-gated behind `otel` — no binary size impact when disabled. Complements existing Prometheus metrics and correlation ID middleware.

**Baseline:** Sprint 53 complete. Correlation ID middleware (X-Request-ID) already shipped in Sprint 41.

**Plan:** `specs/plans/04-opentelemetry.md`

---

### Phase A: OpenTelemetry SDK Integration (HIGH)

- [x] **`telemetry` feature flag** — Already existed in `agentzero-infra/Cargo.toml` and `agentzero-gateway/Cargo.toml`. Deps: opentelemetry 0.29, opentelemetry-otlp, opentelemetry_sdk, tracing-opentelemetry.
- [x] **OTLP exporter** — Already in `telemetry.rs`: `init_telemetry()` with batch span processing, configurable endpoint.
- [x] **Config** — `ObservabilityConfig` in `model.rs`: `otel_endpoint` (default `localhost:4318`), `otel_service_name` (default `"agentzero"`), `backend` (none/otlp).
- [x] **Graceful shutdown** — `TelemetryGuard` with `Drop` impl calls `provider.shutdown()`.

### Phase B: W3C Trace Context Propagation (MEDIUM)

- [x] **Provider spans** — Already instrumented: `info_span!("openai_complete")` and `info_span!("anthropic_complete")` with provider/model fields in both streaming and non-streaming paths.
- [x] **Traceparent header** — `generate_traceparent()` + `apply_traceparent()` in transport.rs. Applied to both OpenAI and Anthropic HTTP requests. W3C format: `00-{trace_id}-{span_id}-01`. 3 tests.
- [x] **Tool execution spans** — `tool.execute()` wrapped with `info_span!("tool_execute", tool_name)` using `.instrument()` (replaces broken `_guard.enter()` pattern). Both timeout and non-timeout paths. 2 tests.

### Phase C: Build Integration (LOW)

- [x] **Justfile** — Added `build-otel` recipe: `cargo build --release -p agentzero --features telemetry`.
- [x] **Docker** — `--build-arg FEATURES=telemetry` supported in Dockerfile.
- [x] **Tests** — Feature compiles cleanly when disabled (default). `init_telemetry_none_backend_returns_none` test exists.

---

### Acceptance Criteria (Sprint 54)

- [x] `cargo build --features telemetry` compiles with OTLP exporter
- [x] OTLP exporter sends traces when `observability.backend = "otlp"` configured
- [x] Provider spans instrumented (openai_complete, anthropic_complete, streaming variants)
- [x] Zero overhead when `telemetry` feature disabled
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass

---

## Sprint 55: MiniMax-Inspired Feature Parity — Code Interpreter, Context Summarization, Media Generation

**Goal:** Add three high-value agent capabilities inspired by competitive analysis: sandboxed code execution (Python/JS), LLM-based context window summarization, and media generation tools (TTS, image, video). Each is independently useful and parallelizable.

**Baseline:** Sprint 54 complete.

**Plan:** `specs/plans/19-minimax-parity.md`

---

### Phase A: Code Interpreter (HIGH)

Sandboxed Python/JavaScript execution via subprocess.

- [x] **`CodeInterpreterTool`** — Already existed (348 lines). Accepts language, code. Subprocess execution with timeout and output cap.
- [x] **Sandbox isolation** — Temp dir per execution, configurable timeout, output truncation. Security policy `enable_code_interpreter` flag.
- [x] **Config** — `[code_interpreter]` section already in `AgentZeroConfig`.

### Phase B: Context Summarization (HIGH)

- [x] **`maybe_summarize_context()`** — Already in `agent.rs`. LLM-based summarization when entries exceed `min_entries_for_summarization`.
- [x] **Config** — `SummarizationConfig` in `agentzero-core/types.rs`: `enabled`, `keep_recent`, `min_entries_for_summarization`, `max_summary_chars`.

### Phase C: Media Generation Tools (MEDIUM)

- [x] **`media_gen.rs`** — Already existed (691 lines). TTS, image gen, and video gen tools with API integration and security policy flags.
- [x] **Config** — `[media_gen.tts]`, `[media_gen.image_gen]`, `[media_gen.video_gen]` sections. Security flags: `enable_tts`, `enable_image_gen`, `enable_video_gen`.

### Phase D: Browser Tool Enhancement (LOW)

- [x] **`ExecuteJs` action** — Already in `BrowserAction` enum.
- [x] **`Content` action** — Already in `BrowserAction` enum.

---

### Acceptance Criteria (Sprint 55)

- [x] Code interpreter exists with sandbox isolation and timeout enforcement
- [x] Context summarization exists with configurable thresholds
- [x] Media generation tools exist (TTS, image, video) with security policy flags
- [x] Browser tool supports ExecuteJs and Content actions
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass

---

## Sprint 56: WASM Runtime Migration — wasmi Interpreter

**Goal:** Replace wasmtime with wasmi as the default WASM runtime. wasmi is a pure-Rust interpreter that dramatically reduces binary size and enables embedded/WASM targets. wasmtime remains available as opt-in JIT backend for performance-critical deployments.

**Baseline:** Sprint 55 complete.

**Plan:** `specs/plans/03-wasm-runtime-migration.md`

---

### Phase A: wasmi Backend (HIGH)

- [x] **Cargo.toml restructure** — Already done: `wasm-runtime` (wasmi, default), `wasm-jit` (wasmtime, opt-in).
- [x] **wasmi backend** — Already in `wasm.rs`: fuel metering, WASI via `wasmi_wasi`, module compilation + caching.
- [x] **Plugin warming** — Pre-compilation at init via `WasmPluginRuntime::compile_module()` in `wasm_bridge.rs`.
- [x] **wasm_bridge.rs** — `WasmEngine`/`WasmModule` type aliases resolve based on feature flag.

### Phase B: Re-gate wasmtime (MEDIUM)

- [x] **Feature gate** — All wasmtime code behind `#[cfg(feature = "wasm-jit")]`. wasmi is default.
- [x] **Test parity** — WASM plugin tests pass with active backend.

### Phase C: Binary Size Validation (MEDIUM)

- [x] **Embedded profile** — `release-min` profile exists. agentzero-lite builds with wasmi by default.

---

### Acceptance Criteria (Sprint 56)

- [x] `cargo build --features wasm-runtime` uses wasmi (default)
- [x] `cargo build --features wasm-jit` uses wasmtime (opt-in)
- [x] WASM plugin tests pass with both backends
- [x] Fuel metering enforces execution timeouts
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass

---

## Sprint 57: Scaling & Operational Readiness

**Goal:** Ship provider fallback chains (automatic retry on circuit-open/5xx), backup/restore CLI, and production environment validation. Completes the operational readiness story.

**Baseline:** Sprint 56 complete. Per-identity rate limiting and circuit breakers already shipped in Sprints 38-41.

**Plan:** `specs/plans/14-scaling-ops.md`

**Note:** Per-identity rate limiting (Sprint 41), Prometheus metrics (Sprint 38), and circuit breakers (Sprint 39) already shipped. This sprint covers the remaining gaps.

---

### Phase A: Provider Fallback Chain (HIGH)

Automatic failover between providers on circuit-open or 5xx errors.

- [x] **`FallbackProvider`** — Already existed (365 lines). Tries providers in order with circuit breaker awareness.
- [x] **Config** — Fallback provider wired through provider config.

### Phase B: Backup & Restore CLI (HIGH)

- [x] **`agentzero backup export/restore`** — Already existed (442 lines). Export creates tar.gz with manifest, restore validates checksums and version.

### Phase C: Production Environment Validation (MEDIUM)

- [x] **`AGENTZERO_ENV`** — `validate_production_env()` in gateway startup. When `AGENTZERO_ENV=production`, warns about missing TLS, disabled pairing auth, missing config. Never fails — only logs. 5 tests.

---

### Acceptance Criteria (Sprint 57)

- [x] Provider fallback exists with circuit breaker awareness
- [x] `agentzero backup export/restore` creates/restores valid archives
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass

---

## Sprint 58: Enterprise Security & Routing — Privacy-Aware Model Routing + YAML Security Policies

**Goal:** Close two enterprise security gaps identified from NVIDIA GTC 2026 competitive analysis: connect the privacy mode system with model routing so sensitive queries automatically prefer local inference, and add a declarative YAML security policy file for per-tool egress/filesystem/command control. Two parallel tracks with no cross-dependencies.

**Baseline:** Sprint 57 complete. Privacy modes (`off`/`private`/`local_only`/`encrypted`/`full`) exist but only gate tools, not inference routing. `ToolSecurityPolicy` uses flat boolean flags with no per-tool granularity.

**Plan:** `specs/plans/25-enterprise-security-routing.md`

**Branch:** `feat/enterprise-security-routing`

---

### Track A: Privacy-Aware Model Routing (MEDIUM)

Connect `ModelRouter` (keyword/pattern classification → provider routing) with the privacy mode system. Currently disconnected — routing ignores privacy, privacy only disables tools.

- [x] **`PrivacyLevel` enum** — `Local`, `Cloud`, `Either` (default) in `routing.rs`. Added `privacy_level: PrivacyLevel` to core `ModelRoute` and `privacy_level: Option<String>` to config `ModelRoute`.
- [x] **`route_query_with_privacy()`** — New method on `ModelRouter`. In `local_only`: only `Local` routes. In `private`: prefer `Local`, fall through to `Cloud`/`Either`. In `off`: all routes.
- [x] **`resolve_hint_with_privacy()`** — Same filtering for explicit hint resolution.
- [x] **Runtime wiring** — `runtime.rs` converts config `privacy_level` string to `PrivacyLevel` enum when building `ModelRouter`.
- [x] **Config** — `privacy_level = "local" | "cloud" | "either"` on `[[model_routes]]` in TOML (optional, defaults to either).
- [x] **Tests** — 6 new: private prefers local, falls through to cloud, local_only blocks cloud, local_only none for cloud-only, off allows all, default either behavior.

### Track B: Declarative YAML Security Policy (HIGH)

Add `.agentzero/security-policy.yaml` — a standalone, auditable, version-controllable policy file providing per-tool egress/filesystem/command rules.

- [x] **`SecurityPolicyFile` struct** — `security_policy.rs`: `DefaultAction` (allow/deny), `ToolRule` with tool glob, egress domains, commands, filesystem paths, action (allow/deny/prompt).
- [x] **Policy evaluation** — `check_tool()`, `check_egress()`, `check_command()`, `check_filesystem()` methods. Tool glob matching (`mcp:*`), domain wildcards (`*.github.com`).
- [x] **Loader** — `SecurityPolicyFile::load(workspace_root)` reads `.agentzero/security-policy.yaml`, returns `None` if absent.
- [x] **Example policy file** — `examples/edge-deployment/security-policy.yaml` with reference rules.
- [x] **Tests** — 12 tests: YAML parses, default deny/allow, domain match, glob match, wildcard domains, prompt decision, command allowlist, filesystem check, tool glob, domain wildcard, missing file returns none.

---

### Acceptance Criteria (Sprint 58)

- [x] `private` mode prefers local model routes over cloud
- [x] `local_only` mode blocks all cloud model routes
- [x] Routes without `privacy_level` default to `either` (backward compat)
- [x] `SecurityPolicyFile` enforces per-tool egress/command/filesystem rules
- [x] Unlisted tools denied when `default: deny`
- [x] `prompt` egress triggers Prompt decision
- [x] Shell commands blocked unless in allowlist
- [x] YAML policy absent = no change (returns None)
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass (18 new: 6 routing + 12 security policy)

---

## Sprint 59: Container Sandbox Mode

**Goal:** Add an optional Docker-based sandbox that enforces the YAML security policy at the OS/network level (iptables) in addition to the application layer. Inspired by NVIDIA OpenShell but without K3s complexity — single container, single binary.

**Baseline:** Sprint 58 complete. YAML security policy file exists and is enforced at application layer.

**Plan:** `specs/plans/25-enterprise-security-routing.md`

**Branch:** `feat/enterprise-security-routing`

**Depends on:** Sprint 58 Track B (YAML security policy).

---

### Phase A: Sandbox Dockerfile & Entrypoint (HIGH)

- [x] **Sandbox Dockerfile** — `docker/sandbox/Dockerfile`: multi-stage build, iptables + ca-certificates + python3, non-root user (uid 1000), read-only workspace mount, tmpfs for `/sandbox` + `/tmp`.
- [x] **Entrypoint script** — `docker/sandbox/sandbox-entrypoint.sh`: reads `security-policy.yaml`, runs `policy-to-iptables.py`, applies rules, drops to non-root, execs gateway.
- [x] **Policy-to-iptables converter** — `docker/sandbox/policy-to-iptables.py`: parses YAML, resolves domains to IPs, outputs iptables rules (default DROP, allow DNS/loopback/established + listed domains).

### Phase B: CLI Command (HIGH)

- [x] **`agentzero sandbox` subcommand** — `start` (validate policy, build docker run args, launch), `stop` (docker stop + rm), `status` (docker inspect, JSON option), `shell` (docker exec -it). 8 tests.
- [x] **Policy validation** — Validates `security-policy.yaml` exists and has valid YAML with `default` key.

### Phase C: Documentation (MEDIUM)

- [x] **Sandbox guide** — `site/src/content/docs/security/sandbox.md`: what sandboxing provides, quickstart, architecture, YAML→iptables flow, comparison with NVIDIA OpenShell.

---

### Acceptance Criteria (Sprint 59)

- [x] `agentzero sandbox start` launches sandboxed container with iptables rules from YAML policy
- [x] Outbound to unlisted domains blocked at network level (iptables default DROP)
- [x] Outbound to listed domains succeeds (resolved IPs get ACCEPT rules)
- [x] Workspace mounted read-only, `/sandbox` and `/tmp` writable (tmpfs)
- [x] `agentzero sandbox status` shows active policy and iptables rules
- [x] `cargo clippy` — 0 warnings
- [x] All tests pass (8 new)

---

## Sprint 60: Visual Workflow Builder (LangChain Fleet-style)

**Goal:** Add a drag-and-drop visual workflow builder UI for composing agent workflows with tools, sub-agents, channels, schedules, and approval gates. Extends the [workflow-graph](https://github.com/auser/workflow-graph) WASM library with node CRUD APIs and integrates into the AgentZero React UI.

**Baseline:** Sprint 59 complete. React UI has CRUD agents, topology graph, run monitoring. No visual workflow builder exists.

**Plan:** `specs/plans/26-visual-workflow-builder.md`

**Branch:** `feat/visual-workflow-builder`

**Tauri compatible:** WASM + Canvas2D works natively in Tauri's WebView.

---

### Phase 1: workflow-graph Extension (upstream) (HIGH)

- [x] **Add `metadata` to `Job` struct** — `HashMap<String, Value>` in `shared/src/lib.rs`. Backward-compatible. Carries `node_type`, `description`, `icon`, `approval_required`.
- [x] **WASM-level node CRUD API** — `add_node()`, `remove_node()`, `update_node()`, `add_edge()`, `remove_edge()`, `get_nodes()`, `get_edges()` on `WorkflowGraphController`. Each mutation triggers re-layout + re-render. 7 new WASM functions.
- [x] **Edge metadata** — `Edge` struct gets `metadata: HashMap<String, Value>` for conditional labels.
- [x] **React wrapper** — Imperative handle: `ref.current.addNode()`, `removeNode()`, `addEdge()`, `removeEdge()`, `updateNode()`, `getNodes()`, `getEdges()`. TS types: `EdgeInfo` interface added.
- [ ] **Publish** — workflow-graph v0.5.0.

### Phase 2: Read-Only Visualization (MEDIUM)

- [x] **WorkflowCanvas** — `topologyToWorkflow()` converter maps AgentZero topology API to workflow-graph `Workflow` format.
- [x] **NodeRenderer** — `onRenderNode` callback renders 6 node types (agent/tool/subagent/channel/schedule/gate) with type-specific colors, icons, labels, and metadata display.
- [x] **WorkflowTopology** — Dashboard component wraps `WorkflowGraphComponent` with dark theme, auto-resize, zoom/reset controls.
- [x] **Dashboard redesign** — New layout: SystemHealthBar (compact metrics) → WorkflowTopology (hero graph) → two-column grid (AgentStatusPanel + ActiveRunsTimeline | ScheduleOverview + ChannelStatus). "Create Workflow" button placeholder.
- [ ] **Embedded UI** — Add `*.wasm` to `rust_embed` include list in gateway router.

### Phase 3: Visual Builder MVP (HIGH)

- [ ] **Zustand store** — `workflowBuilderStore.ts`: nodes, edges, selection, dirty flag, `toSwarmConfig()`, `loadFromSwarmConfig()`.
- [ ] **Builder page** — `/workflows/builder` composing canvas + palette + inspector + popover.
- [ ] **NodePalette** — Left sidebar with draggable node types (Agent, Tool, SubAgent, Channel, Schedule, Gate).
- [ ] **NodePopover** — Click node → inline Radix Popover with name, type badge, key fields, "Open full editor →".
- [ ] **NodeInspector** — Double-click → right-side Radix Sheet with full property form per node type.
- [ ] **Node forms** — AgentNodeForm, ToolNodeForm, ChannelNodeForm, ScheduleNodeForm, GateNodeForm, SubAgentNodeForm.
- [ ] **WorkflowToolbar** — Save, Deploy, Export TOML, Import, Auto-layout, Zoom.
- [ ] **QuickCreateWizard** — 6-step Radix Dialog: name → agent → tools → channel → schedule → review & create.
- [ ] **Serialization** — Builder ↔ SwarmConfig round-trip (toSwarmConfig + loadFromSwarmConfig).
- [ ] **Config model** — Add `approval_required_tools: Vec<String>` to `SwarmAgentConfig`.
- [ ] **API client** — `workflows.ts`: deploy via `PUT /v1/config`, fetch via `GET /v1/config`.

---

### Acceptance Criteria (Sprint 60)

- [ ] workflow-graph v0.5.0 published with metadata + WASM node CRUD API
- [ ] `/workflows` page renders live topology with type-specific node visuals
- [ ] Visual builder: drag Agent + Tool nodes, connect edges, deploy to swarm
- [ ] Quick-Create Wizard: 6 steps → populated builder canvas
- [ ] NodePopover (click) + NodeInspector (double-click) property editing works
- [ ] Round-trip: load SwarmConfig → edit → deploy → reload → no data loss
- [ ] `cargo clippy` — 0 warnings
- [ ] All existing tests pass

---

## Backlog

### Embedded Binary Size Reduction (HIGH)

Reduce the `embedded` profile binary for resource-constrained devices. Currently 10.1MB (budget temporarily at 11MB), target 5-8MB. Phased approach: feature-gate tools into tiers, add plain SQLite option (no sqlcipher), minimize reqwest features, audit with cargo-bloat.

**Plan:** `specs/plans/21-embedded-binary-size-reduction.md`

- [x] **Phase 1: Tool tiering** — Split into `core` (~20 tools), `extended` (~17 tools), `full` (~9 tools). Feature flags: `tools-core`/`tools-extended`/`tools-full`. ToolTier enum with classifier. agentzero-lite uses `tools-extended`. 3 tests.
- [x] **Phase 2: Optional WASM** — `embedded-minimal` feature excludes WASM plugin runtime. WASM tools gated behind `#[cfg(feature = "wasm-plugins")]` (already existed in infra, wired through to CLI/binary Cargo.toml).
- [x] **Phase 3: HTTP client minimization** — Workspace reqwest reduced to `["json"]` only. `stream` added only to providers + CLI (SSE). `multipart` only to infra (Whisper audio). Removed unused cookies/gzip/brotli/deflate/trust-dns.

*Plain SQLite removed — all storage must be encrypted (sqlcipher). cargo-bloat and UPX moved to `specs/BACKLOG-EXTERNAL.md`.*

### TUI Dashboard Enhancement (MEDIUM)

Upgrade the Ratatui CLI dashboard with live data from gateway APIs. Tab-based navigation (Overview, Runs, Agents, Events), HTTP client for gateway polling, auto-refresh via `tokio::select!`, and regression warnings. See Sprint 47 Phase D.

- [x] Tab-based navigation with `DashboardTab` enum and ratatui `Tabs` widget
- [x] HTTP client using daemon host/port + `reqwest::Client`
- [x] Auto-refresh architecture with `tokio::select!` + crossterm event stream (3s polling)
- [x] Runs tab: `Table` widget with status colors (green/yellow/red), tokens, cost
- [x] Agents tab: agent list with active run counts from gateway API
- [x] Events tab: scrolling list with topic color coding (tool=blue, job=green, error=red)
- [x] Regression warnings in Overview tab (health, run counts, agent count)

---

*Items requiring external dependencies (iOS/Xcode, Redis/NATS, Firecracker/mvmd, multi-node clusters, cargo-bloat, UPX) moved to `specs/BACKLOG-EXTERNAL.md`. AgentZero stays standalone.*
