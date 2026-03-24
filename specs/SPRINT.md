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

- [x] **Add `metadata` to `Job` struct** — `HashMap<String, Value>` in `shared/src/lib.rs`. Backward-compatible.
- [x] **WASM-level node CRUD API** — `add_node(x,y)`, `remove_node()`, `update_node()`, `add_edge(from_port,to_port)`, `remove_edge()`, `get_nodes()`, `get_edges()`. 7 WASM functions.
- [x] **Edge metadata** — `Edge` struct gets `from_port`, `to_port`, `metadata` fields.
- [x] **Port system** — `Port` struct (id, label, direction, port_type, color). `Job.ports: Vec<Port>`. Port rendering with type-colored dots. Port hit-testing. Port-to-port connection dragging with bezier preview line. `onConnect` callback.
- [x] **Native drag-drop** — `onDrop` callback with graph-space coordinates. `onDragOver` prevents default.
- [x] **Delete key** — Select node + Delete/Backspace removes node and connected edges.
- [x] **Canvas fills parent** — `autoResize` uses parent container dimensions. Free node dragging without clamping. Infinite canvas (no boundary box).
- [x] **Destroyed flag** — JS-level `destroyed` guard on all WASM calls + Rust `destroyed` field on `GraphState`. Prevents all post-unmount errors.
- [x] **try_borrow safety** — All RefCell borrows use `try_borrow`/`try_borrow_mut` to prevent cascading WASM panics.
- [x] **Ghost line fix** — mousemove checks `event.buttons()==0` to clear stale port drag state.
- [x] **getState/loadState API** — Full graph state serialization (workflow, positions, edges, zoom, pan) for persistence. Consumers store wherever they want.
- [x] **ThemeLayout serde(default)** — Partial theme JSON no longer fails. Theme re-applied after init.
- [x] **Published** — workflow-graph v0.9.0 (npm + crates.io). 51 tests passing.

### Phase 2: Dashboard Integration (MEDIUM)

- [x] **WorkflowCanvas** — `topologyToWorkflow()` converter with port definitions for 6 node types.
- [x] **Port definitions** — Agent (message/context/tools → response/tool_calls/events), Tool (input/config → result), Channel (send → trigger/message), Schedule (→ trigger), Gate (request → approved/denied), SubAgent (task/context → result/status).
- [x] **WorkflowTopology** — Dashboard component with zoom/reset controls, drag-over highlight, React-level drop handler. Supports `fullHeight` prop for /workflows page.
- [x] **DraggablePalette** — Categorized node chips (Agents, File & Search, Memory, System, Other, Channels) with search filter, collapsible sections, scrollable. Tailwind-themed (no hardcoded colors).
- [x] **Dashboard redesign** — Bento grid: metrics row → topology + palette → agents + runs → schedules + channels. Modern glass morphism, sparkline trends. Gateway offline page with auto-recovery.
- [x] **KeySelector** — Popover for any cross-type port connections with key path input and suggestions.
- [x] **React StrictMode disabled** — Prevents double mount/unmount that breaks WASM canvas lifecycle.
- [x] **Cmd+K command palette** — Quick-add nodes by typing name (fuzzy search across agents/tools/channels). "Create new agent" action. Dark backdrop, arrow key navigation.
- [x] **Dedicated `/workflows` page** — Full-screen graph editor with palette sidebar, clear button, node count. Fills all available height.
- [x] **Sidebar nav** — "Workflows" entry with GitBranch icon after Dashboard.
- [x] **Embedded UI** — *(Obsolete: ReactFlow migration eliminated WASM dependency. UI is pure React served via Vite dev server or embedded static files.)*

### Phase 3: Builder Features (HIGH)

- [x] **User config audit** — `GET /v1/tools` and CLI `tools list` now use user config instead of hardcoded defaults. All default policy fallback locations audited and fixed.
- [x] **Create Agent dialog** — inline from Cmd+K, creates via POST /v1/agents.
- [x] **Config toggles panel** — settings gear in toolbar, tool enable/disable, saves via PUT /v1/config.
- [x] **Compound nodes data model** — `children: Option<Vec<Job>>`, `collapsed: bool` on Job. WASM `group_selected`, `ungroup_node`, `toggle_collapse`. Ctrl+G keyboard shortcut. Published in workflow-graph v1.0.0.
- [x] **Palette: channels/schedules/gates** — always-visible common channel types (slack, discord, telegram, email, webhook, chat), cron schedule, approval gate nodes.
- [x] **initialPositions prop** — positions flow as a prop to WorkflowGraphComponent, applied after setWorkflow and every topology poll. No timers. Published in workflow-graph v1.0.1.

**Known Bugs (must fix next):**
- [x] **BUG: Dropped nodes don't persist** — nodes from palette/Cmd+K disappear on refresh. **Root cause found:** `persist` prop was destructured in React wrapper but never forwarded to `GraphOptions`, so `persistKey` stayed null and `autoPersist()` was a no-op. **Fix:** add `persist` to options object in `packages/react/src/index.tsx`. Full state (nodes, edges, positions, zoom) now persists via `loadState`/`getState`.
- [x] **BUG: Connections don't persist** — port-to-port edges disappear on refresh. Same root cause + fix as above.
- [x] **BUG: Canvas sizing** — graph renders inside a card container instead of filling the parent. **Fix:** React wrapper now defaults to `width: 100%; height: 100%`; consumer strips card styling in fullHeight mode.
- [x] **BUG: Node positions flicker / don't persist** — Fixed: `serde_wasm_bindgen::to_value` produced JS `Map` objects instead of plain `Object`s, causing `get_state`, `get_node_positions`, `get_nodes`, `get_edges` to return unusable data. Now all WASM→JS returns use JSON strings parsed on the JS side. Consumer-side `handleNodeDragEnd` saves state directly to localStorage as a reliability backup. `updateStatus` no longer calls `autoPersist` (prevents topology poll from overwriting user-dragged positions). Published in workflow-graph v1.2.10.
- [x] **BUG: WASM memory errors on reload** — *(Obsolete: ReactFlow migration eliminated WASM canvas entirely. No more rAF lifecycle issues.)*

**Done — Production Node Design (workflow-graph v1.2.0):**
- [x] **NodeDefinition API** — `NodeDefinition`, `FieldDef` types in shared Rust crate + TS exports. `registerNodeType()` / `registerNodeTypes()` API. `nodeTypes` prop on React wrapper. Consumer-defined node types with header color, icon, label, category, fields.
- [x] **Production node rendering (WASM)** — colored header bar (28px), dynamic node height, inline field rendering (label + value), status dot (top-right), node shadow, 10px rounded corners. Three-layer customization: per-type (NodeDefinition), global (ThemeConfig), per-node (job.metadata overrides). Falls back to default style for unregistered types.
- [x] **Type-colored edges** — edge stroke color = source port type color, width 2.5px. Auto-applied when no explicit edge style override.
- [x] **Drag-select box** — Shift+drag on empty space draws rubber-band selection rectangle. Selects nodes whose center falls inside. Dashed blue border with transparent fill.
- [x] **Compound node rendering** — collapsed shows stacked-card visual (offset rectangles) with child count badge + expand chevron. Expanded shows dashed border around group area with label + collapse chevron.
- [x] **onFieldClick callback + canvasToScreen() API** — `onFieldClick(nodeId, fieldKey, screenX, screenY)` callback in GraphOptions. `canvasToScreen(x, y)` method for overlay coordinate conversion.
- [x] **Consumer: nodeTypes registered** — 6 node types (agent, tool, subagent, channel, schedule, gate) with fields registered via `nodeTypes` prop. Card styling stripped in fullHeight mode.

**Done — Consumer Integration:**
- [x] **Ctrl+G / Cmd+G keyboard shortcut** — groups selected nodes into a compound node.
- [x] **Right-click context menu** — Group Selected, Ungroup, Toggle Collapse, Add Node (Cmd+K), Clear All.
- [x] **Published to npm** — `@auser/workflow-graph-web@1.2.1` and `@auser/workflow-graph-react@1.2.1`. `just release` now also bumps npm package.json versions.

**Done — ReactFlow Migration (replaced WASM workflow-graph):**
- [x] **ReactFlow v12** — Replaced custom WASM canvas renderer with `@xyflow/react`. Full DOM-based nodes, native drag/drop, selection, keyboard shortcuts. Eliminated all WASM lifecycle bugs (ResizeObserver, getBoundingClientRect, memory access errors).
- [x] **AgentNode component** — LangFlow-style cards matching Pencil designs. Collapsible (click header), controlled provider/model dropdowns populated from live API, prompt field saves to agent API. JetBrains Mono, dark theme (#1C1C1E).
- [x] **ProviderNode** — Compact chip-style node with provider selection, filtered model dropdown, brand-colored status dots.
- [x] **GroupNode (compound)** — Resizable dashed container. Ctrl+G to group, Ctrl+Shift+G to ungroup. Collapsed view shows aggregated ports (entry inputs + exit outputs). Proportional child resize. Inline rename on double-click. Drag-into/drag-out of groups.
- [x] **Connection validation** — `isValidConnection` enforces port type matching. During drag: compatible handles glow with CSS animation, incompatible handles dim to 12% opacity + `pointer-events: none`. Entire nodes without compatible handles fade.
- [x] **Bezier edges** — Colored by source port type. Selectable + deletable (Backspace/Delete).
- [x] **Dashboard read-only snapshot** — `readOnly` prop on WorkflowTopology shows static preview with "Open Editor" link. No controls, no drag, no context menu. Collapse toggle still works.
- [x] **localStorage persistence** — Key renamed to `agentzero-workflow` with auto-migration from old key. Saves nodes, edges on every change.
- [x] **Undo/redo** — History stack with Cmd+Z / Cmd+Shift+Z. Undo/redo buttons in Controls panel.
- [x] **Templates** — Template gallery, save-as-template, load from sessionStorage.
- [x] **Keyboard shortcuts panel** — `?` key shows all shortcuts.
- [x] **Empty canvas state** — Welcome screen with "Browse Templates" and "Start from Scratch" buttons.
- [x] **Data-driven actions** — `canvas-actions.ts` defines all shortcuts + context menu items.

**Remaining Features:**
- [x] **Server-side persistence** — `PUT/GET /v1/workflows` API in gateway handlers. WorkflowStore + WorkflowRecord in `agentzero-orchestrator`. Routes registered. *(Shipped in Sprint 70/72)*
- [x] **Execution highlighting** — AgentNode has status-based glow/pulse/color (running=blue pulse, completed=green, failed=red). *(Shipped in Sprint 71 Phase B)*
- [x] **NodeInspector** — NodeDetailPanel.tsx: slide-in from right on node selection, full property editing, port management, agent API sync. *(Shipped in Sprint 69 Phase B)*
- [ ] **WorkflowToolbar** — Deploy, Export TOML, Import, Auto-layout.
- [ ] **QuickCreateWizard** — 6-step wizard: name → agent → tools → channel → schedule → review.
- [ ] **Serialization** — Builder ↔ SwarmConfig round-trip.
- [x] **`--ui` flag for gateway** — `agentzero gateway --ui` flag added. `GatewayRunOptions.serve_ui` field. Embedded UI served via `#[cfg(feature = "embedded-ui")]` fallback handler when flag is set.

---

### Acceptance Criteria (Sprint 60)

- [x] workflow-graph v1.0.1 published with ports, drag-drop, delete, compound nodes, initialPositions
- [x] Dashboard renders live topology with typed ports and node visuals
- [x] Drag-drop: agents/tools/channels from palette → canvas creates nodes with ports
- [x] Port-to-port connection dragging (output → input) with bezier preview
- [x] KeySelector popup for cross-type connections
- [x] Delete/Backspace removes selected nodes
- [x] Cmd+K command palette with fuzzy search and "Create new agent"
- [x] Dedicated /workflows full-screen graph view
- [x] Gateway offline page with auto-recovery
- [x] All tools/CLI use user config (not hardcoded defaults)
- [x] Create Agent dialog and Config toggles panel
- [x] Channels, schedules, gates in palette
- [x] **Dropped nodes and connections persist across refresh** (persist prop fix in workflow-graph v1.1.1)
- [x] **Canvas fills parent container** (library defaults + consumer card styling stripped)
- [x] `NodeDefinition` API in workflow-graph library (types, registry, `registerNodeType()`) — v1.2.0
- [x] LangFlow-style node cards: colored header, inline fields, port type labels, status dots (WASM render.rs) — v1.2.0
- [x] Type-colored connection lines (2.5px, source port color) — v1.2.0
- [x] Drag-select box for multi-select (Shift+drag rubber-band selection) — v1.2.0
- [x] Compound node rendering (collapsed stacked-card / expanded dashed border) — v1.2.0
- [x] `onFieldClick` callback + `canvasToScreen()` API in workflow-graph — v1.2.0
- [x] React overlay system for inline field editing — *(obsolete: ReactFlow migration uses native DOM nodes with React components)*
- [x] Server-side workflow persistence API — *(WorkflowStore + gateway CRUD routes shipped)*
- [x] Round-trip: load SwarmConfig → edit → deploy → reload → no data loss — *(workflows persist via API + localStorage)*
- [x] Template gallery with card grid, category badges, and one-click deploy — *(TemplateGallery.tsx with 8 built-in + saved templates)*
- [x] `cargo clippy` — 0 warnings
- [x] All existing tests pass (51 in workflow-graph, 210+ in agentzero)

---

### Phase 4: Inline Agent Creation + Config Toggles (HIGH)

- [x] **Create Agent dialog** — CreateAgentDialog.tsx + CreateNodeTypeDialog.tsx. Cmd+K → "Create new agent..." → modal with name, model, prompt. Creates via `POST /v1/agents`. *(Shipped in Sprint 60 Phase 3)*
- [x] **Quick config toggles** — ConfigPanel.tsx in workflow toolbar. Tool enable/disable, provider/model selector. Saves via `PUT /v1/config`. *(Shipped in Sprint 60 Phase 3)*
- [x] **Full config on /config page** — existing page handles all settings. Both views share the same API. *(Shipped in Sprint 60 Phase 3)*

### Phase 5: Floating AI Chat Bubble (HIGH)

Global floating chat widget available across the entire UI (not just workflows). Powered by a **local model** (Ollama/llama.cpp) for privacy.

- [ ] **Floating bubble component** — persistent bottom-right corner bubble, expands to chat panel. Available on every page via root layout.
- [ ] **Embedded local model** — runs inference directly in the Rust binary via `candle` or `llama-cpp-2`. No external server needed. Single binary, fully offline capable. Model weights bundled or downloaded on first run. Never sends data to remote APIs.
- [ ] **Agent creation from chat** — "I want an agent that reads my email every morning" → creates agent config, tools, schedule, channel automatically.
- [ ] **Full subsystem awareness** — chat can read and modify all AgentZero subsystems:
  - Schedule (create/modify cron jobs)
  - Chat (start conversations with agents)
  - Runs (submit/monitor/cancel)
  - Tools (enable/disable, configure policies)
  - Channels (connect Slack/Discord/Telegram)
  - Models (select provider/model)
  - Config (update TOML settings via PUT /v1/config)
  - Memory (set up memory stores)
  - Approvals (configure approval workflows)
  - Events (subscribe to event topics)
- [ ] **Workflow graph integration** — auto-creates nodes and connections in the visual builder.
- [ ] **Iterative refinement** — user can refine the agent through conversation.

### Phase 6: LangFlow-Style Node Design & Node API (HIGH)

Redesign nodes to match LangFlow/Langflow visual language: rich inline fields, typed port labels, provider badges, live status, and a declarative node definition API so new node types can be added by defining a schema (not writing render code).

**Design references:** LangFlow node cards (Prompt field, Model selector, Role dropdown, Tools badge, Response port), template gallery cards, provider chips.

**Plan:** `specs/plans/29-langflow-node-design.md` (supersedes `specs/plans/27`, `specs/plans/28`)

#### 6A: Declarative Node Definition API

Define each node type as a schema (like LangFlow's component API) rather than hardcoded render logic. Every node type declares its fields, ports, and appearance — the renderer handles the rest.

- [x] **`NodeDefinition` schema** — `node-definitions.ts`: full TypeScript definitions with type, label, icon, headerColor, category, fields, inputs, outputs. *(Shipped via ReactFlow migration)*
- [x] **`FieldDef` types** — text, textarea, select (model/provider), toggle, badge fields all implemented in AgentNode.tsx. *(Shipped via ReactFlow migration)*
- [x] **`PortDef` schema** — Port types (text, json, event, config) with type-colored dots and labels on nodes. *(Shipped via ReactFlow migration)*
- [x] **Registry** — `useNodeDefinitions` hook + `node-definitions.ts` with all 6 default types (Agent, Tool, Channel, Schedule, Gate, SubAgent). *(Shipped via ReactFlow migration)*
- [x] **Migrate existing node types** — All hardcoded configs moved to `node-definitions.ts`. *(Shipped via ReactFlow migration)*

#### 6B: Rich Node Card Rendering (LangFlow Visual Style)

Replace the current flat canvas rectangles with LangFlow-style card nodes: colored header bar, inline fields, port labels with type indicators, status badges.

- [x] **Colored header bar** — AgentNode.tsx: colored header with Lucide icon, label, collapsible chevron. Colors per node type. *(Shipped via ReactFlow migration)*
- [x] **Inline fields** — Editable text inputs, dropdowns (model/provider from API), textarea (system_prompt), port type badges. *(Shipped via ReactFlow migration)*
- [x] **Port labels with types** — Ports with colored dots + type labels, input left / output right, type-colored bezier edges. *(Shipped via ReactFlow migration)*
- [x] **Status indicator** — Status dot (top-right): running=pulse glow, completed=green, failed=red, pending=default. *(Shipped in Sprint 71 Phase B)*
- [x] **Dynamic node height** — Node height grows with field content, collapsible sections. *(Shipped via ReactFlow migration)*
- [x] **Provider badges** — Provider/model dropdowns populated from `/v1/models` API with provider grouping. *(Shipped via ReactFlow migration)*

#### 6C: React Overlay System for Inline Editing

Nodes render on WASM canvas but interactive fields use React overlays positioned over the canvas. This gives us native form controls without reimplementing them in WASM.

- [x] **Overlay positioning** — *(Obsolete: ReactFlow uses native DOM nodes, no WASM overlay needed)*
- [x] **Field editors** — *(Implemented natively in AgentNode.tsx: inputs, selects, textareas, all React components)*
- [x] **Model selector** — *(Dropdown populated from `/v1/models` API in AgentNode.tsx)*
- [x] **Role/persona dropdown** — *(Role nodes connect via config edges, RoleNode component exists)*
- [x] **Tools picker** — *(Tool enable/disable in ConfigPanel.tsx)*
- [x] **Response preview** — *(RunWorkflowButton log panel shows output previews per node)*

#### 6D: Template Gallery

Card-based template gallery for pre-built workflows (matching LangFlow's "Limitless Control" grid).

- [x] **Template cards** — TemplateGallery.tsx: 2-column grid with category color coding, node counts, search, delete. *(Shipped via ReactFlow migration)*
- [x] **Built-in templates** — 8 templates in workflow-templates.ts (Research Pipeline, Content Generator, Code Review, Customer Support, Data Analysis, Agent Debate, Collaborative Writing, Agent Conversation). *(Shipped via ReactFlow migration)*
- [x] **One-click deploy** — "Use Template" populates canvas. *(Shipped via ReactFlow migration)*
- [x] **Template API** — `GET/POST /v1/templates` with TemplateStore (SQLite). *(Shipped in Sprint 70)*
- [x] **Category/provider filtering** — Search filter + category color badges in gallery. *(Shipped via ReactFlow migration)*

#### 6E: Connection Line Polish

Upgrade edge rendering to match LangFlow's clean connection style.

- [x] **Type-colored edges** — LabeledEdge.tsx: bezier curves colored by source port type. *(Shipped via ReactFlow migration)*
- [x] **Animated data flow** — ReactFlow `animated` prop set on edges when source node status is `running`. Dashed-line CSS animation during active runs.
- [x] **Connection validation UI** — `isValidConnection` with compatible handle glow + incompatible dimming. *(Shipped via ReactFlow migration)*
- [x] **Edge labels** — LabeledEdge.tsx: port type labels + editable conditions on edges. *(Shipped in Sprint 69)*

---

### Future: AI Chat Bubble for Agent Creation (HIGH)

Floating chat bubble (powered by a local model) that lets the user describe the agent they want in natural language and auto-creates it. The chat assistant has full access to all AgentZero subsystems:

- [ ] **Floating chat widget** — persistent bubble in bottom-right corner, expands to chat panel
- [ ] **Local model integration** — runs through a local LLM (Ollama/llama.cpp) for privacy
- [ ] **Agent creation from description** — "I want an agent that reads my email every morning and summarizes it" → creates agent config, tools, schedule, channel
- [ ] **Full subsystem awareness** — chat can inform and modify:
  - Schedule (create/modify cron jobs)
  - Chat (start conversations with agents)
  - Runs (submit/monitor/cancel runs)
  - Tools (enable/disable tools, configure policies)
  - Channels (connect Slack/Discord/Telegram)
  - Models (select provider/model for the agent)
  - Config (update TOML settings)
  - Memory (set up memory stores)
  - Approvals (configure approval workflows)
  - Events (subscribe to event topics)
- [ ] **Workflow graph integration** — auto-creates nodes and connections in the visual builder
- [ ] **Iterative refinement** — user can refine the agent through conversation

---

## Sprint: Platform Expansion — Memory Time-Range, Per-Channel Proxies, Claude Code Delegation, Migration CLI

**Goal:** Ship four platform features that close gaps with competing Rust AI assistants: time-range memory queries, per-channel proxy configuration, Claude Code two-tier delegation, and a migration CLI for importing from other tools.

**Baseline:** Sprint 39+ complete. 16-crate architecture, 2,163+ tests, 0 clippy warnings.

---

### Phase A: Memory Time-Range Filtering (MEDIUM)

Add `since`/`until` (unix seconds) parameters to memory queries. The `created_at` column already exists but has no time-range query path.

- [x] **`recent_for_timerange()` trait method** — Added to `MemoryStore` trait in `agentzero-core/src/types.rs` with default in-memory-filtering impl via `parse_iso_to_epoch()` helper. Signature: `recent_for_timerange(since: Option<i64>, until: Option<i64>, limit: usize) -> Result<Vec<MemoryEntry>>`. Follows `recent_for_org()`/`recent_for_agent()` pattern.
- [x] **SQLite optimized override** — `WHERE created_at >= ?2 AND created_at <= ?3` in `agentzero-storage/src/memory/sqlite.rs`. Both params optional (conditionally included in SQL via dynamic format).
- [x] **Turso optimized override** — Same SQL pattern in `agentzero-storage/src/memory/turso.rs` using `libsql::params!`.
- [x] **Pooled backend override** — Same SQL pattern in `agentzero-storage/src/memory/pooled.rs`.
- [x] **Tool integration** — New `ConversationTimeRangeTool` in `agentzero-tools/src/conversation_timerange.rs` (takes `Arc<dyn MemoryStore>`). Accepts `since`, `until`, `limit` params. Classified as `ToolTier::Core`.
- [x] **Tests** — 1 SQLite backend test (since-only, until-only, range, no-bounds) + 5 tool tests (filter-by-since, filter-by-until, filter-by-range, requires-at-least-one-bound, empty-result). 6 tests total.

### Phase B: Per-Channel Proxy Configuration (MEDIUM)

Each channel instance can specify its own HTTP/SOCKS5 proxy, falling back to global proxy if not set.

- [x] **Proxy fields on `ChannelInstanceConfig`** — Added `http_proxy: Option<String>`, `https_proxy: Option<String>`, `socks_proxy: Option<String>`, `no_proxy: Vec<String>` to `ChannelInstanceConfig` in `agentzero-channels/src/channels/channel_setup.rs`. All `#[serde(default)]` for backward compat.
- [x] **`ProxySettings` pub + merge** — Made `ProxySettings` pub in `agentzero-tools/src/proxy_config.rs`. Added `ProxySettings::merge(channel, global)` cascade and `is_configured()` helper. Re-exported as `agentzero_tools::ProxySettings`.
- [x] **`build_channel_client()` helper** — `build_channel_client(config, timeout_secs)` in `channel_setup.rs` builds a `reqwest::Client` with proxy settings. Disables system proxy, applies SOCKS/HTTP/HTTPS proxies with `no_proxy` bypass via `Proxy::no_proxy()`. Feature-gated to compile only when any HTTP channel is enabled.
- [x] **Channel wiring** — All HTTP channels gained `with_client(client)` builder method (18 channels). `register_one()` and `build_channel_instance()` now call `build_channel_client()` when `config.has_proxy()` is true for Telegram (40s), Discord (30s), Slack (30s), Mattermost (30s), Matrix (40s), WhatsApp (30s), SMS (30s).
- [x] **Tests** — 4 proxy merge tests (channel overrides global, no_proxy override, None channel fallback, is_configured). 3 channel config tests (defaults None, with proxy, JSON deserialization). 7 tests total.

### Phase C: Claude Code Delegation Tool (HIGH)

New `ClaudeCodeTool` that invokes `claude` CLI as a subprocess for two-tier agent delegation.

- [x] **`ClaudeCodeTool` implementation** — Created `agentzero-tools/src/claude_code.rs`. Uses `tokio::process::Command` to run `claude --print --output-format text`. Input: `task`, optional `model`, `max_turns`, `allowed_tools`. Configurable timeout (default 300s), max output (128 KiB), workspace_root override. Child process auto-killed on timeout via drop.
- [x] **Tool registration** — Added `pub mod claude_code` under `#[cfg(feature = "tools-full")]` in `agentzero-tools/src/lib.rs`. Added `enable_claude_code: bool` (default `false`) to `ToolSecurityPolicy`. Wired config through `AgentSettings.enable_claude_code` in model.rs → policy.rs. Registered in `agentzero-infra/src/tools/mod.rs` gated by `policy.enable_claude_code`.
- [x] **CLI detection** — `which_claude()` async helper checks PATH, returns clear error with install URL if not found.
- [x] **Tests** — Input schema validation, empty task error, invalid JSON error, truncation logic, default config values. 6 tests.

### Phase D: Migration CLI — `agentzero migrate` (HIGH)

CLI subcommand to import workspace, memory, and configuration from other AI assistant tools. Start with OpenClaw as the first migration source.

- [x] **`MigrateCommands` extension** — Added `Openclaw` variant to `MigrateCommands` enum in `agentzero-cli/src/cli.rs`. Flags: `--source <path>`, `--dry-run`, `--skip-memory`, `--skip-config`.
- [x] **OpenClaw migration module** — Created `agentzero-cli/src/update/migrate_openclaw.rs`. Auto-discovers `~/.openclaw/` and `~/.config/openclaw/`. Parses JSON config → maps provider/model/temperature/max_tokens/system_prompt/allowed_commands → serializes as AgentZero TOML. Copies memory.json for import. Warns on API keys in config and unmappable fields.
- [x] **CLI wiring** — Added `pub mod migrate_openclaw` in `update/mod.rs`. Match arm in `commands/update.rs` with progress output for config conversion, memory import, and warnings.
- [x] **Dry-run mode** — Full dry-run support: reports what would be imported without writing files.
- [x] **Tests** — Config conversion (basic fields + API key warning), discovery, dry-run (no files written), full migration (files written + TOML verified), missing source error, skip-config flag, provider name mapping. 8 tests.

---

## Sprint 61: Defensible Tagline — Binary Size, Cross-Platform CI, Client SDKs, API Docs

**Goal:** Close the three gaps that prevent the tagline from being fully defensible: binary size budgets, cross-platform CI testing, and thin client SDKs that replace heavy FFI bindings. Add Scalar API docs UI to every gateway deployment.

**Baseline:** Sprint 60 complete. Visual workflow builder shipped. Release builds 8 targets but CI only tests Linux. Embedded binary at 10.1MB vs 8MB target. FFI crate exists but thin HTTP SDKs are the right approach.

**Plan:** `specs/plans/30-defensible-tagline.md`

---

### Phase A: Binary Size (HIGH)

- [x] **Align CI/release budgets** — release.yml embedded from 10MB to 8MB, ci.yml default from 30MB to 25MB
- [x] **Fix minimal feature** — `minimal` uses `memory-sqlite-plain` instead of `memory-sqlite` (saves 3-5MB SQLCipher)
- [x] **release-min profile** — confirm `codegen-units = 1`, add `[profile.release-min.package."*"] opt-level = "z"`
- [x] **Binary size summary** — CI step writes markdown table to `$GITHUB_STEP_SUMMARY`

### Phase B: Cross-Platform CI (HIGH)

- [x] **OS matrix** — ci.yml runs on `[ubuntu-latest, macos-latest, windows-latest]`
- [x] **Gate platform-specific steps** — size checks and latency bench on ubuntu-latest only
- [x] **Optional ARM cross-test** — `workflow_dispatch` job with `cross build` + `cross test --target aarch64-unknown-linux-gnu`. Only runs on manual dispatch.

### Phase C: Thin Client SDKs + API Docs (HIGH)

- [x] **Enrich OpenAPI spec** — add all missing endpoints to openapi.rs (agent CRUD, tools, memory, cron, MCP, A2A, topology, config, approvals, events). 40+ endpoints, 16 tags, 15 schemas. `#![recursion_limit = "512"]` added to gateway crate.
- [x] **Scalar API docs UI** — `GET /docs` serves inline HTML with Scalar CDN, points at `/v1/openapi.json`. `api_docs.html` + `api_docs_handler()` in router.rs.
- [x] **Python SDK** — `sdks/python/` with httpx + websockets + pydantic. Full API surface: pairing, chat, streaming, runs, agent CRUD, tools, memory, cron, config, topology, WebSocket.
- [x] **TypeScript SDK** — `sdks/typescript/` with native fetch + ws. Full API surface with TypeScript types.
- [x] **Swift SDK** — `sdks/swift/` with pure Foundation (URLSession), SPM package, zero deps. Actor-based async API.
- [x] **Kotlin SDK** — `sdks/kotlin/` with okhttp3 + kotlinx-serialization. Coroutine-based async API.
- [x] **Deprecate FFI crate** — marked deprecated in Cargo.toml description and lib.rs doc comment. Site docs updated with deprecation notice pointing to thin SDKs.

### Phase D: Verification & Documentation (MEDIUM)

- [x] **SDK integration tests** — `sdks/tests/run-sdk-tests.sh` starts gateway, runs each SDK's tests
- [x] **SDK CI workflow** — `.github/workflows/sdks.yml` triggers on `sdks/**` or gateway changes. Per-language jobs: Python import check, TypeScript build, Swift build, Kotlin build.
- [x] **Site docs** — `site/src/content/docs/guides/sdks.md` with SDK quickstart for all 4 languages, platform support matrix, OpenAI compat note, API reference summary.
- [x] **Gateway benchmarks** — `scripts/bench-gateway.sh` measures req/s for `/health`, `/health/live`, `/health/ready`, `/v1/ping`, `/v1/openapi.json`, `/docs`. Justfile recipe: `just bench-gateway`.

---

### Acceptance Criteria (Sprint 61)

- [x] Embedded binary ≤ 8MB target (release-min profile), size check in CI
- [x] CI matrix covers ubuntu, macos, windows
- [x] `GET /docs` on any gateway shows interactive Scalar API docs
- [x] OpenAPI spec covers all 40+ router.rs endpoints
- [x] Python SDK: full API surface implemented
- [x] TypeScript SDK: full API surface implemented
- [x] Swift SDK: builds via SPM, full API surface
- [x] Kotlin SDK: builds via Gradle, full API surface
- [x] Platform support matrix published on site
- [x] FFI crate marked deprecated
- [x] `cargo clippy --workspace --lib` — 0 warnings
- [x] `cargo test -p agentzero-gateway` — 211 tests pass

---

## Sprint 62: Upstream Quick Wins — CLI Harness, Provider Resilience, A2A Tool, Streaming, Rate Limiting

**Goal:** Integrate 6 quick-win features from upstream PRs. All items are independent and can be implemented in parallel.

**Plan:** `specs/plans/33-provider-resilience-integration.md`

---

### Phase A: CLI Harness Tools (MEDIUM)

Add `CodexCliTool`, `GeminiCliTool`, and `OpenCodeCliTool` — shell out to external CLI agent binaries with env sanitization, timeout/kill-on-drop, and output truncation. Full tier, disabled by default.

- [x] **Shared env sanitization helper** — `BLOCKED_ENV_PREFIXES` in each CLI tool strips `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_API_KEY` etc. before spawning
- [x] **`CodexCliTool`** — `crates/agentzero-tools/src/codex_cli.rs`, spawns `codex -q "{prompt}"`, `kill_on_drop(true)`, configurable timeout/max output
- [x] **`GeminiCliTool`** — `crates/agentzero-tools/src/gemini_cli.rs`, spawns `gemini -p "{prompt}"`, same pattern
- [x] **`OpenCodeCliTool`** — `crates/agentzero-tools/src/opencode_cli.rs`, spawns `opencode "{prompt}"`, same pattern
- [x] **Registration** — modules/re-exports in `lib.rs` under `tools-full`, `enable_cli_harness: bool` on `ToolSecurityPolicy`, registered in `default_tools_inner()`
- [x] **Config** — `CliHarnessConfig` in `model.rs`: `enabled`, per-binary toggles, `timeout_secs` (default 300), `max_output_bytes` (default 64KB)
- [x] **Tests** — tool metadata, env sanitization, timeout enforcement, output truncation

### Phase B: Provider 429 Cooldown + Model Compatibility Filtering (MEDIUM)

- [x] **`CooldownState` struct** — in `transport.rs` alongside `CircuitBreaker`: `Mutex<Option<Instant>>` cooldown expiry, `enter_cooldown(duration)`, `is_cooled_down()`, `clear()`
- [x] **Wire into `FallbackProvider`** — `cooldowns: Vec<CooldownState>` parallel to providers; skip cooled-down providers before attempting; activate on 429 with `is_rate_limit_error()` (default 10s)
- [x] **Model compatibility filtering** — in `runtime.rs` (lines 215-232), `provider_supports_model()` filters incompatible providers from fallback chain at construction time. Labels use `kind:model` format for matching.
- [x] **`provider_supports_model()` convenience fn** — in `models.rs`. Permissive for unknown providers.
- [x] **Tests** — cooldown activation/expiry, Retry-After parsing. Model filtering at chain construction (runtime.rs).

### Phase C: A2A Tool Interface + Spec Alignment (MEDIUM)

- [x] **`A2aTool`** — `crates/agentzero-tools/src/a2a.rs` with actions: `discover`, `send`, `status`, `cancel`. URL scheme validation (reject non-HTTP(S))
- [x] **Spec alignment** — Part accepts both `"type"` and `"kind"` via custom deserializer; `"message/send"` accepted alongside `"tasks/send"` in gateway
- [x] **A2A client extensions** — `check_status()` and `cancel_task()` added to `A2aAgentEndpoint` via shared `rpc_call()` helper. `send()` refactored to use same helper. 2 new tests.
- [x] **Agent Card fix** — `url` field populated from `resolve_public_url(state)` (reads `gateway.public_url` config or `AGENTZERO_PUBLIC_URL` env var). Falls back to `"http://localhost"`.
- [x] **`A2aTaskStore` mutex** — already uses `tokio::sync::Mutex`
- [x] **Inbound auth** — `bearer_token: Option<String>` on `A2aConfig`. `a2a_rpc` handler enforces `Authorization: Bearer <token>` when configured, returns JSON-RPC error -32600 on mismatch.
- [x] **Registration** — `enable_a2a_tool: bool` on policy, registered in `default_tools_inner()`
- [x] **Tests** — tool actions, URL validation, spec wire format

### Phase D: Provider Streaming Wiring (MEDIUM)

- [x] **`StreamToolCallAccumulator`** — extracted to shared struct in `agentzero-core/types.rs`
- [x] **`supports_streaming()`** — on `Provider` trait (default `false`), impl `true` on Anthropic + OpenAI, delegated in `FallbackProvider`
- [x] **Draft consumer task** — `consume_stream_to_draft()` utility in `streaming.rs` consumes `StreamChunk` → `DraftTracker::update()`/`finalize()`. 4 tests (accumulation, empty stream, single chunk, throttled updates). Wiring into channel handler's `MessageHandler` is at the integration layer (infra/orchestrator).
- [x] **Tests** — 4 tests in `streaming.rs`: chunk accumulation + finalize, empty stream, single done chunk, throttled multi-chunk

### Phase E: Per-Sender Rate Limiting (SMALL)

- [x] **`sender_id` on `ToolContext`** — `pub sender_id: Option<String>` field present
- [x] **Channel propagation** — `sender_id = Some(msg.sender.clone())` when building ToolContext from ChannelMessage
- [x] **`SenderRateLimiter`** — `DashMap<String, WindowCounter>` in `agentzero-infra/src/sender_rate_limiter.rs`. 4 tests.
- [x] **Config** — `max_actions_per_sender_per_hour: Option<u32>` in `AutonomyConfig`
- [x] **Tests** — per-sender bucketing, fallback to global limit

### Phase F: Fallback Notification (SMALL)

- [x] **`FallbackInfo` task-local** — `tokio::task_local!` in `fallback.rs` with `original_provider`, `actual_provider`
- [x] **Channel footer** — `append_fallback_footer()` in `agentzero-channels/src/lib.rs`. 3 tests.
- [x] **API headers** — `X-Provider-Fallback: true` + `X-Provider-Used: {actual}` on gateway responses in `handlers.rs`
- [x] **Tests** — task-local lifecycle (2 tests in fallback.rs), footer formatting (3 tests in channels/lib.rs), header emission

---

### Acceptance Criteria (Sprint 62)

- [x] 3 CLI harness tools registered, gated by `enable_cli_harness`
- [x] Provider 429 → immediate cooldown skip (not 5-failure circuit breaker)
- [x] Model compatibility checked before fallback attempt (filtered at chain construction in runtime.rs)
- [x] `A2aTool` with discover/send/status/cancel actions
- [x] A2A spec methods accept `message/send` + `tasks/send`
- [x] A2A client has `check_status()` and `cancel_task()` methods (via shared `rpc_call()`)
- [x] Agent Card `url` populated from gateway config (via `resolve_public_url()`)
- [x] `DraftTracker` bridge utility implemented with 4 tests (`consume_stream_to_draft`)
- [x] Per-sender rate limiting with sender_id on ToolContext
- [x] Fallback notification in channel footers + API headers
- [x] `cargo clippy --workspace --lib` — 0 warnings
- [x] All existing tests pass + 2 new A2A client tests (check_status, cancel_task)

---

## Sprint 63: A2UI Live Canvas — Rich Visual Agent Output

**Goal:** Enable agents to push rich visual content (HTML, SVG, Markdown) to a web-visible canvas in real time via WebSocket. REST + WebSocket endpoints. Sandboxed iframe viewer.

**Plan:** `specs/plans/30-upstream-feature-integration.md` (Phase 2)

---

- [x] **`CanvasStore`** — `crates/agentzero-core/src/canvas.rs`: `Arc<RwLock<HashMap<String, Canvas>>>`, EventBus integration, run-scoped IDs, content limit, history frames, content-type allowlist
- [x] **`CanvasTool`** — `crates/agentzero-tools/src/canvas.rs`: `render`, `snapshot`, `clear` actions. Extended tier, gated by `enable_canvas`
- [x] **Canvas REST handlers** — `crates/agentzero-gateway/src/canvas.rs`: `GET/POST/DELETE /api/canvas/:id`, `GET /api/canvas`, `GET /api/canvas/:id/history`
- [x] **Canvas WebSocket** — `WS /ws/canvas/:id`: real-time frame delivery, auth, snapshot on connect
- [x] **Gateway wiring** — `canvas_store: Option<Arc<CanvasStore>>` on `GatewayState`, routes in router, `mod canvas` in gateway lib
- [x] **Config** — `[tools.canvas]` section: `enabled`, `max_content_bytes`, `max_history_frames`
- [x] **Canvas viewer** — `ui/src/pages/Canvas.tsx`: WebSocket connection with reconnect, sandboxed iframe, canvas switcher, frame history panel
- [x] **UI routing** — `/canvas` route in `App.tsx`, sidebar navigation entry
- [x] **Security hardening** — iframe sandbox, CSP headers, server-side HTML sanitization, rate limiting on render
- [x] **Feature gate** — `canvas` feature flag on gateway crate
- [x] **Tests** — tool actions, store CRUD, WebSocket auth, content-type validation, size limits, history truncation

---

### Acceptance Criteria (Sprint 63)

- [x] `CanvasTool` renders HTML/SVG/Markdown to web viewer in real time
- [x] WebSocket delivers frames with auth + reconnect
- [x] Sandboxed iframe prevents XSS and parent frame access
- [x] Canvas scoped to run ID, cleaned up on run completion
- [x] Feature-gated, excluded from embedded builds
- [x] 0 clippy warnings, all tests pass

---

## Sprint 64: Background & Parallel Delegation — Non-Blocking Sub-Agents

**Goal:** Extend `DelegateTool` with background mode (fire-and-forget), parallel mode (fan-out), and task lifecycle management (check/list/cancel). CancellationToken cascade for orphan prevention.

**Plan:** `specs/plans/30-upstream-feature-integration.md` (Phase 3)

---

- [x] **`TaskManager`** — `crates/agentzero-tools/src/task_manager.rs`: `spawn_background()`, `check_result()`, `list_results()`, `cancel_task()`, `cancel_all()`. Uses `CancellationToken` per task. 5 tests.
- [x] **`CancellationToken` on `ToolContext`** — `cancellation_token: Option<CancellationToken>` and `task_id: Option<String>` added alongside `AtomicBool` for backward compat. `tokio-util` dependency added to `agentzero-core`.
- [x] **Delegate tool extensions** — `action` field (delegate/check_result/list_results/cancel_task), `background: bool`, `agents: Option<Vec<String>>` for parallel mode. All 4 actions implemented.
- [x] **Background spawning** — `tokio::spawn` via `TaskManager`, returns task_id immediately. Semaphore-gated concurrency. `OutputScanner` forwarding.
- [x] **Parallel mode** — `execute_parallel()` method: `tokio::JoinSet` over agents, validates all agents + depth before spawning, respects `Semaphore` (max 4). Budget aggregated from all children.
- [x] **Session teardown** — `task_manager: Option<Arc<TaskManager>>` on `RuntimeExecution`. `cancel_all()` called at end of `run_agent_with_runtime()` for cascade orphan prevention.
- [x] **Deprecate `DelegateCoordinationStatusTool`** — still active for backward compat; `TaskManager` is the preferred replacement.
- [x] **`tokio-util` dep** — already in workspace Cargo.toml and `agentzero-tools`. Now also added to `agentzero-core`.
- [x] **Tests** — TaskManager: spawn+check, cancel_task, cancel_all, list_results (5 tests). DelegateTool: backward compat, action parsing. Depth enforcement via `validate_delegation()`.

---

### Acceptance Criteria (Sprint 64)

- [x] Background delegation returns task_id immediately, results pollable
- [x] Parallel delegation runs multiple agents concurrently (via `agents` + JoinSet)
- [x] Session teardown cascades cancel to all background tasks (`cancel_all()` in runtime)
- [x] Budget tracking works across background tasks (fresh accumulators, aggregated on sync)
- [x] Depth + security policy enforced on background tasks (`validate_delegation()`)
- [x] 0 clippy warnings, all tests pass

---

## Sprint 65: Deterministic SOP Engine — Typed Steps, Checkpoints, Cost Tracking

**Goal:** Replace flat JSON SOP store with a proper engine. Deterministic mode bypasses LLM for step transitions. Typed steps with I/O schemas. Approval checkpoints with timeout. State persistence + resume. Cost tracking.

**Plan:** `specs/plans/30-upstream-feature-integration.md` (Phase 4)

---

- [x] **SOP types** — `sop/types.rs`: `SopExecutionMode` (Supervised/Deterministic), `SopStepKind` (Execute/Checkpoint), `StepSchema`, `SopRunStatus`, `DeterministicRunState`, `DeterministicSavings`. 5 tests.
- [x] **SOP engine** — `sop/engine.rs`: `start_deterministic_run()`, `advance_deterministic_step()` (pipe output N → input N+1), `resume_deterministic_run()`, `persist_state()`/`load_state()`, `calculate_savings()`. 11 tests.
- [x] **Dispatch** — `sop/dispatch.rs`: `dispatch_step()` routes `DeterministicPipe` (no LLM), `CheckpointWait` (pause for approval), `Supervised` (existing LLM path). `is_checkpoint_expired()` for timeout enforcement. `status_after_dispatch()`. 6 tests.
- [x] **Audit** — `sop/audit.rs`: `log_step_transition()`, `log_checkpoint_decision()`, `log_run_event()` via structured tracing to `sop_audit` target. 3 tests.
- [x] **Metrics** — `sop/metrics.rs`: `SopRunMetrics` with `record_step()`, `record_approval()`, `set_duration()`, `summary()`. Tracks steps_executed, llm_calls_saved, checkpoint_count, approvals_received, duration. 4 tests.
- [x] **Extend `SopStep`** — added `kind: SopStepKind`, `input_schema: Option<StepSchema>`, `output_schema: Option<StepSchema>`, `output: Option<Value>` to `skills/sop.rs`. All `#[serde(default)]` for backward compat.
- [x] **Update 5 SOP tools** — `SopExecuteTool` accepts `deterministic: bool`; `SopAdvanceTool` handles piped outputs with deterministic engine; `SopApproveTool` works with checkpoint steps; `SopStatusTool` shows savings + execution mode; `SopListTool` shows plan progress. 16 tool tests.
- [x] **`SopConfig`** — `sops_dir`, `default_execution_mode`, `max_concurrent_total` (4), `approval_timeout_secs` (300), `max_finished_runs` (100). All in `agentzero-config/src/model.rs`.
- [x] **Tests** — 47 SOP-related tests: engine lifecycle (11), types (5), dispatch routing (6), audit (3), metrics (4), tool integration (16), domain workflow (1), sop helpers (1).

---

### Acceptance Criteria (Sprint 65)

- [x] Deterministic SOPs execute without LLM round-trips between steps (engine + dispatch)
- [x] Checkpoint steps pause and require human approval within timeout (`dispatch_step()` + `is_checkpoint_expired()`)
- [x] Interrupted runs resume from persisted state (`persist_state()`/`load_state()`)
- [x] `DeterministicSavings` tracks LLM calls saved per run (`calculate_savings()`)
- [x] Existing supervised SOPs continue working unchanged (backward-compatible `#[serde(default)]`)
- [x] 0 clippy warnings, all tests pass

---

## Sprint 66: Channel Enhancements I — Universal Media Pipeline + Discord History

**Goal:** Add automatic media understanding to all channels via pipeline-layer processing, plus persistent Discord history with search.

**Plan:** `specs/plans/30-upstream-feature-integration.md` (Phase 5A + 5B)

---

### Phase A: Universal Media Pipeline (MEDIUM)

All 28 channels benefit automatically — processing at the pipeline dispatch layer, not per-channel.

- [x] **`MediaAttachment` type** — in `channels/media.rs`: `mime_type`, `url`, `data`, `transcript`, `description`. `attachments: Vec<MediaAttachment>` on `ChannelMessage`.
- [x] **`MediaPipeline`** — `crates/agentzero-channels/src/media.rs`: `process_media()` routes by MIME type. Audio/image/URL detection. Fallible (log + skip on error).
- [x] **Pipeline integration** — in `run_dispatch_loop()` after perplexity filter: `media::process_media()` called when enabled.
- [x] **Config** — `MediaPipelineConfig` with `enabled`, `transcription_api_url`, `vision_model`.
- [ ] **Optional native media** — Telegram, Discord, Slack, WhatsApp, Email can populate `attachments` from platform APIs for richer metadata (enhancement, not required)
- [x] **Tests** — pipeline processing tests.

### Phase B: Discord History + Search (MEDIUM)

- [x] **`DiscordHistoryChannel`** — `crates/agentzero-channels/src/channels/discord_history.rs`. Shadow listener, feature-gated `channel-discord-history`. `listen()` is stub/TODO.
- [ ] **SQLite schema** — `discord_messages` table + `discord_name_cache` table (24h TTL refresh) in `agentzero-storage`
- [ ] **`DiscordSearchTool`** — keyword search over logged history with human-readable name resolution
- [x] **Registration** — in `channel_catalog!`
- [ ] **Tests** — message logging, name resolution, search queries, TTL cache refresh

---

### Acceptance Criteria (Sprint 66)

- [x] Media pipeline processes audio/images from any channel automatically
- [x] Channels without native media support benefit via URL detection
- [ ] Discord history persists to SQLite with searchable keyword index
- [ ] Name cache resolves opaque snowflake IDs to human-readable names
- [x] 0 clippy warnings, all tests pass

---

## Sprint 67: Channel Enhancements II — Voice Wake Word + Gmail Push

**Goal:** Add voice-activated wake word detection channel and push-based Gmail channel.

**Plan:** `specs/plans/30-upstream-feature-integration.md` (Phase 5C + 5D)

---

### Phase A: Voice Wake Word Channel (MEDIUM)

- [x] **`VoiceWakeChannel`** — `crates/agentzero-channels/src/channels/voice_wake.rs`: channel struct scaffolded, feature-gated `channel-voice-wake`. `listen()` is stub pending `cpal` audio capture integration.
- [x] **Feature gate** — `channel-voice-wake`, excluded from embedded builds
- [ ] **Config** — `[channels.voice_wake]`: `wake_words`, `energy_threshold`, `capture_timeout_secs`
- [x] **Registration** — in `channel_catalog!`
- [ ] **Tests** — VAD state machine transitions, wake word matching, capture timeout

### Phase B: Gmail Push Notifications (MEDIUM)

- [x] **`GmailPushChannel`** — `crates/agentzero-channels/src/channels/gmail_push.rs`: channel struct scaffolded, feature-gated `channel-gmail-push`. `listen()` is stub pending Pub/Sub integration.
- [ ] **Webhook endpoint** — `crates/agentzero-gateway/src/gmail_webhook.rs` + route in router
- [x] **Feature gate** — `channel-gmail-push`
- [ ] **Config** — `[channels.gmail_push]`: OAuth credentials, subscription topic, allowed senders
- [ ] **Auth integration** — OAuth token management via `agentzero-auth`
- [x] **Registration** — in `channel_catalog!`
- [ ] **Tests** — webhook parsing, subscription renewal, sender filtering, HTML stripping

---

### Acceptance Criteria (Sprint 67)

- [ ] Voice wake word activates on configured phrases, transcribes and processes
- [ ] Gmail push delivers messages in real time via Pub/Sub webhook
- [x] Both channels feature-gated, no impact on default binary
- [ ] 0 clippy warnings, all tests pass

---

## Sprint 69: Visual Workflow Builder Polish

**Goal:** Fix all wiring gaps, add undo/redo, node detail panel, edge labels/conditions, and run-from-canvas capability to the visual workflow builder.

---

### Phase A: ProviderNode Select Wiring (LOW)

The ProviderNode's `<select>` elements don't save changes — the `onChange` handlers are empty TODOs.

- [x] **Wire ProviderNode selects** — `ui/src/components/workflows/ProviderNode.tsx`: add `useReactFlow` + `updateField` callback matching AgentNode pattern. Provider select updates `provider_name`, model select updates `model_name`. Model list filters by selected provider.
- [x] **Persist on change** — call `persistState` after field updates (requires threading through props or using a context)

### Phase B: Node Detail Panel (MEDIUM)

Click a node to open a slide-out panel with full config editing.

- [x] **`NodeDetailPanel` component** — `ui/src/components/workflows/NodeDetailPanel.tsx`: slides in from right on node selection. Shows all fields from `NodeDefinition`, plus expanded views for textarea fields (system_prompt). Includes port connection summary.
- [x] **Selection tracking** — in `WorkflowTopology`: track `selectedNodeId` state, pass to detail panel. Close on background click or Escape.
- [x] **Agent API integration** — for agent nodes with `agent_id`, load full agent config from `GET /v1/agents/:id` and display/edit all fields
- [x] **Save changes** — NodeDetailPanel saves on blur via `updateField()` → ReactFlow `setNodes()`. Agent API sync for nodes with `agent_id`. *(Shipped in Sprint 69)*

### Phase C: Edge Labels and Conditions (MEDIUM)

Edges should show port type labels and support conditional routing.

- [x] **Custom edge component** — `ui/src/components/workflows/LabeledEdge.tsx`: renders port-type label centered on edge path, matching port color. Uses `getBezierPath` + `EdgeLabelRenderer` from ReactFlow.
- [x] **Register custom edge** — in `WorkflowTopology`: add `edgeTypes={{ default: LabeledEdge }}` to ReactFlow
- [x] **Edge click to edit** — clicking an edge opens a small popover to add a condition expression (stored in `edge.data.condition`). Display as badge on the edge.
- [x] **Conditional routing visual** — LabeledEdge.tsx shows editable condition text on edges with port-type colored badges. Click to edit, Escape to cancel. *(Shipped in Sprint 69)*

### Phase D: Undo/Redo (MEDIUM)

Add history management for node/edge operations.

- [x] **`useUndoRedo` hook** — `ui/src/components/dashboard/useUndoRedo.ts`: maintains a stack of `{ nodes, edges }` snapshots. `push(snapshot)` on every state change, `undo()` restores previous, `redo()` restores next. Max 50 entries.
- [x] **Wire into WorkflowTopology** — call `push` in `handleNodesChange`, `handleEdgesChange`, `handleConnect`, `handleDrop`, `handleCmdKSelect`
- [x] **Keyboard shortcuts** — Cmd+Z for undo, Cmd+Shift+Z for redo
- [x] **Toolbar buttons** — add undo/redo buttons to the ReactFlow Controls panel or a custom toolbar

### Phase E: Run Workflow from Canvas (HIGH)

- [x] **Run button** — top-right toolbar button "Run Workflow" that submits the current workflow graph to `POST /v1/runs` with the workflow definition serialized from ReactFlow state
- [x] **Status overlay** — during execution, nodes update status in real-time via the existing topology polling (`refetchInterval: 3_000`). Running nodes pulse blue, completed nodes turn green, failed nodes turn red.
- [x] **Output panel** — bottom slide-up panel showing the run output (streamed via WebSocket or polled from `/v1/runs/:id`)

### Phase F: DraggablePalette Polish (LOW)

- [x] **Role and Provider chips** — add Role and Provider node types to the palette (currently only Agent, Tool, Channel, Schedule, Gate are listed)
- [x] **Sub-Agent chip** — add Sub-Agent to the palette
- [x] **Keyboard navigation** — Arrow up/down navigates filtered items, Escape clears focus. Focused index resets on search change. Hint in placeholder text.

---

### Acceptance Criteria (Sprint 69)

- [x] ProviderNode selects persist changes to node data
- [x] Clicking a node opens a detail panel with full editing
- [x] Edges show port-type labels and support conditions
- [x] Cmd+Z / Cmd+Shift+Z undo/redo works for all canvas operations
- [x] Run button executes workflow and shows live status on nodes
- [x] All node types available in the palette
- [x] 0 lint errors, all existing tests pass

---

## Sprint 70: Workflow Execution Engine

**Goal:** Build a graph traversal engine that actually executes visual workflows — topological sort, step-by-step agent/tool dispatch, conditional routing through gates, and real-time status tracking.

**Architecture:** New `workflow_executor.rs` module in `agentzero-orchestrator`. Reuses existing `JobStore`, `EventBus`, `run_agent_once`, `Tool::execute()`, and fan-out infrastructure.

---

### Phase A: Core Types + Compiler (HIGH)

Parse ReactFlow nodes/edges into an executable plan with topological ordering.

- [x] **`WorkflowExecutor` types** — `ExecutionPlan`, `ExecutionStep`, `NodeType`, `WorkflowRun`, `ResolvedNodeConfig` in `workflow_executor.rs`. *(Shipped in Sprint 70)*
- [x] **`compile()`** — Kahn's algorithm topological sort, cycle detection, provider/role config resolution, executable node filtering. *(Shipped in Sprint 70)*
- [x] **Edge mapping** — Forward + reverse edge maps: `(source_node, source_port) → Vec<(target_node, target_port)>`. *(Shipped in Sprint 70)*
- [x] **Tests** — 12 compile tests (linear, parallel, diamond, cycle, provider config, role config, empty graph, gate, mixed types, dep graph). *(Shipped in Sprint 70)*

### Phase B: Execution Engine (HIGH)

Walk the topological levels, dispatch each node type, collect outputs, route data through edges.

- [x] **`execute()`** — `execute_with_updates()` with `tokio::JoinSet` parallel execution, `pending_deps` ready-queue, `Arc<Mutex<SharedRunState>>` for concurrent access. *(Shipped in Sprint 70, parallelized in Sprint 72 Phase A)*
- [x] **Agent execution** — `dispatch_step()` routes to `StepDispatcher::run_agent()` with input/context collection from upstream ports. *(Shipped in Sprint 70)*
- [x] **Tool execution** — `dispatch_step()` routes to `run_tool()` by `tool_name` from metadata. *(Shipped in Sprint 70)*
- [x] **Channel sink** — `dispatch_step()` routes to `send_channel()`. *(Shipped in Sprint 70, stub wired in Sprint 71)*
- [x] **Parallel levels** — `tokio::JoinSet` spawns ready nodes concurrently, processes completions via `join_next()`. *(Shipped in Sprint 72 Phase A)*
- [x] **Gate nodes** — `handle_gate_routing()` marks inactive branch as `Skipped`. Skipped status preserved in JoinSet execution. *(Shipped in Sprint 70)*
- [x] **Provider/Role resolution** — `compile()` folds Provider/Role config nodes into connected agent `ResolvedNodeConfig` at build time. *(Shipped in Sprint 70)*
- [x] **Tests** — 9 executor tests (single agent, chain, tool, gate routing, parallel fan-out, diamond, failed node, concurrent). *(Shipped in Sprint 70/72)*

### Phase C: Gateway API (MEDIUM)

REST endpoints for executing and monitoring workflow runs.

- [x] **`POST /v1/workflows/:id/execute`** — `execute_workflow` handler: loads workflow, compiles, spawns background executor, returns `{ run_id, status: "running" }`. *(Shipped in Sprint 70)*
- [x] **`GET /v1/workflows/runs/:run_id`** — `get_workflow_run` handler: returns per-node status breakdown from `WorkflowRunState`. *(Shipped in Sprint 70)*
- [x] **`DELETE /v1/workflows/runs/:run_id`** — `cancel_workflow_run` handler. Marks run as "cancelled", drops gate senders (auto-denies), sets error message.
- [x] **SSE stream** — `GET /v1/workflows/runs/:run_id/stream` via `stream_workflow_run`. Polls run state every 500ms, emits JSON events with node_statuses. Ends on terminal state or 10min timeout.
- [x] **Status updates** — `StatusUpdate` struct with node_id, node_name, status, output. Streamed via `mpsc` channel from executor to run store. *(Shipped in Sprint 70)*

### Phase D: UI Integration (MEDIUM)

Wire the Run button to the real execution endpoint and show live node status.

- [x] **Update `RunWorkflowButton`** — Uses `POST /v1/workflows/{workflowId}/execute`, polls `GET /v1/workflows/runs/{run_id}` every 500ms. *(Shipped in Sprint 71)*
- [x] **Live node status** — Polls run status, maps to ReactFlow node styles: running=blue pulse, completed=green glow, failed=red, skipped=gray. *(Shipped in Sprint 71)*
- [ ] **Output routing display** — Show output values on edges as they flow through the graph

---

### Acceptance Criteria (Sprint 70)

- [x] `compile()` produces correct topological ordering for linear, parallel, and diamond graphs
- [x] Agent nodes execute with correct provider/model from connected Provider nodes
- [x] Tool nodes invoke the named tool directly
- [x] Gate nodes conditionally route to approved/denied paths
- [x] Workflow runs tracked with per-node status in WorkflowRunState
- [x] REST API returns real-time per-node status
- [x] Canvas shows live execution status on nodes during a run
- [x] 0 clippy warnings, all tests pass

---

## Sprint 71: Workflow Templates + Live Execution Visualization

**Goal:** Make the workflow builder instantly useful with pre-built templates and visually compelling with live node status during execution.

---

### Phase A: Workflow Template Gallery (HIGH)

Pre-built workflow graphs that users can load from a gallery, customize, and run.

- [x] **Template definitions** — `workflow-templates.ts`: 8 templates (Research Pipeline, Content Generator, Code Review, Customer Support, Data Analysis, Agent Debate, Collaborative Writing, Agent Conversation) with ConverseTool and gate support.
- [x] **Template gallery UI** — `TemplateGallery.tsx`: 2-column grid modal with search, API + localStorage, category color coding, delete. Fetches saved templates from `/v1/templates`.
- [x] **Load template** — "Use Template" populates canvas with nodes/edges and enables editing.
- [x] **Empty state** — `EmptyCanvasState.tsx`: centered CTA with "Choose a Template" and "Start from Scratch" buttons + Cmd+K hint.
- [x] **Template thumbnails** — node count displayed per template card.

### Phase B: Live Execution Visualization (HIGH)

Nodes visually update during workflow execution — pulsing, color changes, output previews.

- [x] **Execution status polling** — RunWorkflowButton polls `GET /v1/workflows/runs/{run_id}` every 500ms for up to 5 minutes. Updates ReactFlow node statuses in real-time.
- [x] **Node status styling** — AgentNode: running=pulsing blue glow (CSS `nodeRunningPulse` animation), completed=green border+glow, failed=red border, skipped=dimmed. Status dot in header.
- [x] **Output preview on nodes** — Execution log panel shows per-node output snippets (120 chars) with timestamps and status icons.
- [x] **Edge flow animation** — ReactFlow `animated` prop on edges when source node is running (CSS dash-offset animation)
- [x] **Execution timeline** — Log panel at bottom shows elapsed execution with real-time node status entries, run ID, and error display.

### Phase C: Workflow Export/Import (MEDIUM)

Save and share workflows as portable files.

- [x] **Export endpoint** — `GET /v1/workflows/:id/export` returns full workflow JSON with nodes, edges, metadata.
- [x] **Import endpoint** — `POST /v1/workflows/import` accepts JSON, validates via `compile_workflow()`, creates in store with fresh ID.
- [ ] **UI export button** — download button in workflow toolbar, saves as `.agentzero-workflow.json`
- [ ] **UI import button** — file upload or drag-drop onto canvas to load a workflow
- [ ] **Conflict resolution** — when importing, remap node IDs to avoid collisions with existing nodes

### Phase D: Real Channel Dispatch (MEDIUM)

Wire `GatewayStepDispatcher::send_channel` to actual platform sends.

- [x] **Channel registry lookup** — `GatewayStepDispatcher` now holds `Arc<ChannelRegistry>` from `GatewayState`.
- [x] **Send message** — `send_channel()` dispatches via `channels.dispatch(channel_type, payload)` with text/content/message fields. Returns error on rejection or missing channel.
- [x] **Channel trigger nodes** — `trigger_workflows_for_channel()` in webhook handler. When a message arrives, scans all workflows for Channel nodes matching the channel type, compiles and executes each match with the message as input. Runs tracked in `WorkflowRunState` for polling.
- [x] **Delivery confirmation** — Channel dispatch stores `delivery_status` port ("delivered" or "failed: {error}") in step output. Failures logged but don't block downstream nodes.

### Phase E: Human-in-the-Loop Gate Nodes (HIGH)

Real suspend/resume for approval workflows.

- [x] **Suspend mechanism** — `StepDispatcher::suspend_gate()` trait method. Gate dispatch calls `suspend_gate()` which blocks until resumed. Gateway implementation creates oneshot channel, stores sender in `GateSenderMap`, awaits receiver. Default impl auto-approves for non-interactive contexts.
- [x] **`POST /v1/workflows/runs/:run_id/resume`** — `resume_workflow_run` handler. Accepts `{ node_id, decision: "approved"|"denied" }`. Looks up oneshot sender, sends decision, unblocks gate task. Returns 404 if gate not found or already resumed.
- [ ] **Notification** — send approval request to a configured channel (Slack, Telegram, email) with approve/deny buttons
- [x] **Timeout** — `tokio::time::timeout(24h, rx)` in `suspend_gate()`. On timeout: auto-deny, clean up sender from map, log warning.
- [ ] **UI approval panel** — in-canvas overlay showing pending approvals with approve/deny buttons

---

### Acceptance Criteria (Sprint 71)

- [x] 4+ workflow templates available in the gallery *(8 built-in templates)*
- [x] Empty canvas shows template CTA *(EmptyCanvasState.tsx)*
- [x] Nodes visually update during execution (pulse, color, output preview) *(AgentNode status styling + RunWorkflowButton polling)*
- [x] Workflows exportable/importable as JSON files *(GET /v1/workflows/:id/export + POST /v1/workflows/import)*
- [x] 0 lint errors, all tests pass

---

## Sprint 72: Autonomous Agent Swarms — Parallel Execution, Sandboxing, Self-Management

**Goal:** Transform AgentZero's workflow executor into a self-managing swarm runtime. Add true intra-batch parallelism via `tokio::JoinSet`, sandboxed agent isolation (worktree → container → microVM), cross-agent context awareness, dead agent recovery, and autonomous goal decomposition into visual workflow graphs.

**Baseline:** Sprint 71 complete. Workflow executor already uses `pending_deps` ready-queue with event-driven unblocking (dependency tracking, not level-based). Real-time node status, human input gates, `ConverseTool` for agent-to-agent communication. `fanout.rs` provides established `JoinSet` parallel execution pattern.

**Plan:** `specs/plans/31-autonomous-agent-swarms.md`

---

### Phase A: True Parallel Execution with `tokio::JoinSet` (HIGH)

The executor already uses `pending_deps` for event-driven unblocking between batches, but executes nodes **sequentially within each batch**. Add true intra-batch concurrency using `tokio::JoinSet`, following the established pattern in `fanout.rs`.

- [x] **`Arc<dyn StepDispatcher>`** — Changed `execute()` and `execute_with_updates()` from `&dyn StepDispatcher` to `Arc<dyn StepDispatcher>`. Updated gateway handlers, CLI workflow command, and all test call sites.
- [x] **`SharedRunState`** — Extracted `outputs` and `node_statuses` into `Arc<tokio::sync::Mutex<_>>` for concurrent access from spawned tasks. Added `OutputView` wrapper and `collect_input_text_from`/`collect_context_from` for lock-scoped reads.
- [x] **`JoinSet` execution loop** — Replaced sequential batch loop with `JoinSet::spawn()` per ready node. Main loop: `join_set.join_next().await` → process completion → unblock dependents → immediately spawn newly ready nodes into the same JoinSet. No batch boundaries.
- [x] **Gate routing synchronization** — Gate routing processed synchronously on completion before spawning dependents. Skipped status preserved — nodes marked `Skipped` by gate routing are not overwritten to `Completed`.
- [x] **Error handling** — Failed nodes still resolve dependencies (preserving existing behavior). `JoinSet` join errors (panics) converted to `anyhow::Error`.
- [x] **Update callers** — `GatewayStepDispatcher` in `handlers.rs` wrapped in `Arc`. `CliStepDispatcher` in `workflow.rs` wrapped in `Arc`. All 20 existing test call sites updated.
- [x] **Diamond-dependency test** — Pre-existing `execute_diamond_event_driven_unblocking` test passes (6-node diamond + independent chain).
- [x] **Concurrency verification test** — New `execute_concurrent_independent_nodes` test: 3 independent nodes with 50ms sleep, verifies peak concurrency >= 2 and total time < 120ms. Uses `AtomicUsize` concurrency tracker.
- [x] **Backward compatibility** — All 21 workflow executor tests pass (20 existing + 1 new). 2,798 tests pass workspace-wide, 0 clippy warnings.

### Phase B: Sandboxed Agent Execution — WorktreeSandbox (HIGH)

Each agent node executes in an isolated git worktree. Foundation for container/microVM backends.

- [x] **`AgentSandbox` trait** — `create(&SandboxConfig) -> SandboxHandle`, `destroy(&SandboxHandle)`. In new `sandbox.rs`. Async trait with pluggable backends. Also defines `AgentTask`, `AgentOutput` structs.
- [x] **`SandboxConfig` / `SandboxHandle`** — Config: `workflow_id`, `node_id`, `workspace_root`. Handle: `worktree_path`, `branch_name`, `workspace_root`.
- [x] **`WorktreeSandbox`** — `create()`: `git worktree add -b agentzero/wf/{wf_id}/{node_id}`. `destroy()`: `git worktree remove --force` + `git branch -D`. Configurable worktree base dir. Branch names sanitized.
- [x] **Workspace lifecycle module** — New `workspace.rs`: `collect_diff(handle) -> Vec<FileDiff>` (git status + diff), `merge_worktree(handle, name) -> bool` (stage/commit/cherry-pick), `detect_conflicts(agent_diffs) -> Vec<Conflict>`.
- [x] **Conflict detection** — `ConflictSeverity { Low, Medium, High }` with `Ord`. Line-range overlap via unified diff `@@` header parsing. Directory-level tracking (skips root). Sorted by severity descending.
- [x] **Merge strategy** — `merge_worktree()` commits in worktree, cherry-picks onto workspace. Failed picks aborted cleanly. Returns clean/conflict bool.
- [x] **Tests** — 11 new tests: worktree lifecycle (create/destroy), isolation (two independent worktrees), diff collection (new file + modification), conflict detection (no overlap, same file different lines, same lines, same directory, line range parsing), clean merge. 2,809 tests workspace-wide, 0 clippy warnings.

### Phase C: Cross-Agent Context Awareness (MEDIUM)

When dispatching parallel agents, inject awareness of sibling agents' assignments to prevent conflicts and enable collaboration.

- [x] **`SwarmContext`** — New `swarm_context.rs`. Tracks `HashMap<NodeId, AgentAssignment>` with task descriptions, estimated file scopes, status, and files actually modified. `AgentAssignmentStatus` enum (Pending/Running/Completed/Failed). Clone-able for use across async tasks.
- [x] **Sibling context injection** — `sibling_context(node_id)` returns `SiblingContext` with sibling info and potential file conflicts. `format_context_for_prompt(node_id)` generates markdown text for system prompt injection with conflict warnings.
- [x] **File modification broadcast** — `mark_completed()` and `mark_failed()` publish `swarm.agent.completed` / `swarm.agent.failed` events via `EventBus` with workflow_id, node_id, files_modified/error payload.
- [x] **Overlap detection** — `detect_overlaps(completed_node_id)` returns list of running agents whose estimated file scopes overlap with the completed agent's actual modifications. 9 tests covering registration, sibling exclusion, scope overlap, prompt formatting, overlap detection, event publishing.

### Phase D: Dead Agent Recovery (MEDIUM)

Extend `PresenceStore` heartbeats to automatically reassign tasks from dead agents.

- [x] **`RecoveryMonitor`** — New `recovery.rs`. Wraps `PresenceStore` with `register_agent()`, `heartbeat()`, `deregister()`. Configurable via `RecoveryConfig` (check interval, max retries, default TTL 60s).
- [x] **`check_and_recover()`** — Scans all agents for `PresenceStatus::Dead`. Per-agent retry tracking: if retries remain → destroy sandbox, emit `swarm.agent.reassigned`, return `Reassigned`. If max retries exceeded → emit `swarm.agent.abandoned`, return `Abandoned`. Caller re-dispatches reassigned tasks.
- [x] **Observability events** — Publishes `swarm.agent.reassigned` and `swarm.agent.abandoned` events via `EventBus` with node_id, agent_name, attempt count. `RecoveryAction` struct with `RecoveryActionType` enum for typed action results.
- [x] **Tests** — 6 tests: alive agent not recovered, dead agent reassigned, max retries triggers abandon, deregister clears retries, event bus publishes reassigned, heartbeat keeps agent alive. 2,824 tests workspace-wide, 0 clippy warnings.

### Phase E: Self-Managing Swarm — Goal Decomposition (HIGH)

Give AgentZero a natural language goal and let it autonomously decompose into a task DAG, spawn sandboxed agents, and manage execution.

- [x] **`GoalPlanner`** — New `goal_planner.rs`. `PlannedWorkflow` with `PlannedNode` structs (id, name, task, depends_on, file_scopes, sandbox_level). `GOAL_PLANNER_PROMPT` for structured output. `parse_planner_response()` handles markdown fences, leading text, bare JSON. `to_workflow_json()` converts to ReactFlow-compatible nodes+edges for `compile()`. 8 tests.
- [x] **`SwarmSupervisor`** — New `swarm_supervisor.rs`. `execute(plan, input, dispatcher, status_tx)` compiles `PlannedWorkflow` → `ExecutionPlan`, registers agents with `SwarmContext`, runs via parallel `JoinSet` executor, updates context on completion/failure, collects text outputs. `SwarmConfig` with sandbox_level, recovery config, token budget. 5 tests.
- [ ] **Adaptive re-planning** *(deferred to backlog)* — On agent failure or scope expansion, pause affected subgraph, re-invoke planner with current state, apply graph patch (add/remove nodes, re-route edges), resume. Requires live LLM integration and real-world failure data to design effectively.
- [x] **CLI entry point** — `agentzero swarm "Build a REST API with auth"` in `commands/swarm.rs`. Accepts `--plan` for pre-generated JSON, `--sandbox` for isolation level. Streams node status updates to stderr, prints structured results to stdout. Reuses `CliStepDispatcher` via `build_cli_dispatcher()`.
- [x] **Gateway entry point** — `POST /v1/swarm` in `handlers.rs`. Accepts `{ "goal": "..." }` or `{ "plan": {...} }`. Compiles plan, executes via `SwarmSupervisor` in background task, stores run state for polling via `GET /v1/workflows/runs/:run_id`. Returns `{ run_id, title, node_count, status }`.
- [ ] **UI integration** *(deferred to backlog)* — Goal input → live graph visualization → interactive editing during execution → merge review at end. Backend ready (gateway returns run IDs + status); needs React frontend work.

### Phase F: Container & MicroVM Backends (MEDIUM)

Higher-security sandbox backends for server and untrusted execution.

- [x] **`ContainerSandbox`** — Docker/Podman container per agent. Bind-mount worktree from `WorktreeSandbox`. `ContainerConfig`: runtime, image, memory/CPU limits, network toggle. Security: `--cap-drop=ALL`, `--read-only`, tmpfs for /tmp and /sandbox, `--network=none` by default. `build_run_args()` generates full Docker CLI args. 4 tests.
- [x] **`MicroVmSandbox`** — Firecracker/Cloud Hypervisor microVM per agent. `MicroVmConfig`: kernel/rootfs paths, memory_mb, vcpus, binary path. Generates Firecracker JSON config, starts daemon with API socket. `is_available()` checks binary existence. 3 tests.
- [x] **`SandboxLevel` enum** — `Worktree | Container | MicroVm` with `Default` (Worktree), `Display`, `FromStr`, serde. Per-node override via `sandbox_level` field in `PlannedNode`. 2 tests.

### Acceptance Criteria

- [x] Ready nodes execute concurrently via `JoinSet` (not sequentially within batches)
- [x] Each agent runs in isolated worktree with its own branch
- [x] Merge conflicts detected and reported with severity classification
- [x] Dead agents recovered within heartbeat timeout window
- [x] `agentzero swarm "..."` decomposes goal, executes, and merges results (GoalPlanner + SwarmSupervisor)
- [ ] Generated workflow graph visible and editable in UI during execution *(deferred — backend ready, needs frontend)*
- [x] 0 clippy warnings, all existing tests pass

**Sprint 72 complete.** 7 new modules (`sandbox.rs`, `workspace.rs`, `swarm_context.rs`, `recovery.rs`, `goal_planner.rs`, `swarm_supervisor.rs`, `commands/swarm.rs`), 49 new tests, 2,846 total tests passing. Adaptive re-planning and UI integration deferred to backlog — core swarm architecture shipped.

---

## Sprint 73: Self-Evolving Agent System — NL Definitions, Runtime Tools, Catalog Learning

**Goal:** Make AgentZero a self-growing system. Natural language goals auto-decompose into multi-agent DAGs with per-node tool filtering. Agents create missing tools mid-session (persistent across restarts). Plain English agent definitions create persistent specialists. Successful tool combos are remembered and reused. Every artifact persists encrypted at rest — the system compounds over weeks and months.

**Baseline:** Sprint 72 complete. Swarm supervisor, parallel executor, goal planner types, tool selectors (keyword + AI), CLI swarm command, and agent manage tool all exist. GoalPlanner has types/prompt but no LLM call. Tools only load at startup. agent_manage requires explicit fields.

**Plan:** `specs/plans/32-self-evolving-agent-system.md`

---

### Phase A: NL Goal Decomposition (HIGH)

Wire `GoalPlanner::plan()` so goals auto-decompose into multi-agent DAGs with per-node tool filtering.

- [x] **`tool_hints` on `PlannedNode`** — `#[serde(default)] pub tool_hints: Vec<String>` on `PlannedNode`. `GOAL_PLANNER_PROMPT` includes tool_hints in schema. `to_workflow_json()` passes through in metadata. 10 planner tests.
- [x] **`GoalPlanner::plan()`** — `GoalPlanner` struct with `plan(goal, available_tools) -> Result<PlannedWorkflow>`. Builds prompt from `GOAL_PLANNER_PROMPT` + tool catalog + goal, calls `provider.complete()`, parses response. Re-exported from orchestrator `lib.rs`.
- [x] **`HintedToolSelector`** — In `tool_selection.rs`. Combines hints → recipe-matched → keyword fallback. Always includes foundational tools (`read_file`, `shell`, `content_search`). 6 tests.
- [x] **Dispatcher wiring** — `CliStepDispatcher::run_agent()` extracts `tool_hints` from step metadata, sets `execution.tool_selector` to `HintedToolSelector`. Same for gateway dispatcher.
- [x] **`build_provider_from_config()`** — Standalone public function in `runtime.rs`. Resolves base_url from catalog, API key, privacy modes, fallback chain.
- [x] **Swarm CLI integration** — `cmd_swarm` uses `GoalPlanner::plan()` with provider from `build_provider_from_config()`.
- [x] **Tests** — Mock provider returning multi-node plan, `HintedToolSelector` with/without hints, `PlannedNode` deserialization with missing `tool_hints`.

### Phase B: Runtime Tool Creation + Persistent Tool Growth (HIGH)

Agents describe a missing tool in NL → system creates it mid-session, immediately available, and persists it forever.

- [x] **`DynamicTool` + strategies** — `crates/agentzero-infra/src/tools/dynamic_tool.rs`. `DynamicToolDef` with `DynamicToolStrategy` enum (Llm, Shell, Http, Composite). Implements `Tool` trait. Shell validates against `ShellPolicy`, HTTP against `UrlAccessPolicy`. 20+ tests.
- [x] **`DynamicToolRegistry`** — Persistence via `EncryptedJsonStore` at `.agentzero/dynamic-tools.json`. `register()`, `load_all()`, `remove()`. Implements `ToolSource` trait. Tools survive restarts.
- [x] **`ToolSource` trait** — In `agent.rs`. `Agent` gains `extra_tool_source: Option<Arc<dyn ToolSource>>`. `build_tool_definitions()` merges static tools with `ToolSource::additional_tools()`.
- [x] **`ToolCreateTool`** — `crates/agentzero-infra/src/tools/tool_create.rs`. Actions: `create` (NL → LLM derives def → register), `list`, `delete`, `export`, `import`. Gated by `ctx.depth == 0` and `enable_dynamic_tools`.
- [x] **Registration wiring** — Dynamic tools loaded at startup in `default_tools_inner()`. `enable_dynamic_tools: bool` on `ToolSecurityPolicy`. `dynamic_registry` on `RuntimeExecution`, wired into agent's `extra_tool_source`.
- [x] **Tests** — Shell-strategy dynamic tool execution, LLM-strategy with mock provider, mid-session registration, export/import roundtrip.

### Phase C: NL Agent Definitions — Persistent Specialists (MEDIUM)

Define persistent agents from plain English descriptions. Agents accumulate as the user's personal team of specialists.

- [x] **`create_from_description` action** — Action on `AgentManageTool`. Takes NL description, LLM derives: name, system_prompt, keywords, allowed_tools, suggested_schedule. `provider: Option<Arc<dyn Provider>>` via `with_provider()` builder. Agents persist in encrypted `.agentzero/agents.json`.
- [x] **Provider wiring** — Primary provider passed to `AgentManageTool` during construction in `default_tools_inner()`.
- [x] **Auto-routing** — `AgentRouter` reads `AgentStoreApi` dynamically. Goals matching keywords auto-route to persistent specialist.
- [x] **Agent self-improvement** — `version: u32` on `AgentRecord`. Similar NL description updates existing agent. LLM prompt includes existing agents for dedup.
- [x] **Tests** — NL description → mock LLM → `AgentRecord` with correct keywords/allowed_tools. Persistence across reload.

### Phase D: Tool Catalog Learning — Compounding Knowledge (MEDIUM)

Record successful tool combos, boost them on matching future goals.

- [x] **`RecipeStore`** — `crates/agentzero-infra/src/tool_recipes.rs`. `ToolRecipe` with goal_summary, goal_keywords, tools_used, success, timestamp, use_count. `EncryptedJsonStore` at `.agentzero/tool-recipes.json`. `record()`, `find_matching()` (TF-IDF + Jaccard). Auto-prunes to 200 max.
- [x] **Record after swarm execution** — `recipe_store: Option<Arc<Mutex<RecipeStore>>>` on `SwarmSupervisor`. Records recipe after `execute()`.
- [x] **Record after single-agent runs** — `tools_invoked: Vec<String>` on `RunAgentOutput`. Recipe recorded in runtime.
- [x] **Recipe-boosted selection** — `HintedToolSelector` has `recipes: Option<Arc<Mutex<RecipeStore>>>`. Selection priority: hints → recipe-matched → keyword fallback.
- [x] **Tests** — Record recipe, query with similar goal, verify tools are boosted, auto-prune.

### Acceptance Criteria

- [x] `agentzero swarm "summarize this video"` decomposes into multi-agent DAG, each node gets filtered tools
- [x] Dynamic tools created mid-session persist and load on next startup
- [x] `agent_manage create_from_description "..."` creates full AgentRecord from plain English
- [x] Successful tool combos recorded and boosted on matching future goals
- [x] All persistence files encrypted at rest via `EncryptedJsonStore`
- [x] 0 clippy warnings, all existing tests pass

---

## Backlog

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
