# AgentZero Sprint Plan

Previous sprints archived to `specs/sprints/33-38-production-hardening-scaling.md`.

---

## Sprint 39: Full Production Platform — Event Bus, Multi-Tenancy, Examples, Lightweight Mode, AI Tool Selection

**Goal:** Ship every remaining production gap plus the strategic platform features: embedded distributed event bus (no external dependencies), multi-tenancy deepening, AI-driven tool selection, lightweight orchestrator mode, comprehensive examples, and hardening (fuzzing, container scanning, SBOM, runbooks, request validation, liveness probe, Turso migrations).

**Baseline:** Sprint 38 complete (2,163 tests, 0 clippy warnings). All CRITICAL/HIGH security and reliability gaps closed. Per-identity rate limiting, provider fallback, OpenAPI, backup/restore, TLS, HSTS, audit logging all shipped.

**Plan:** `specs/plans/10-full-production-platform.md`

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
- [ ] **Binary size target** — Currently 12MB release, target <10MB. Needs tool tiering + reqwest trimming.
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

**Plan:** `specs/plans/11-whatsapp-sms-channels.md`

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
- [ ] **Binary size target** — Under 10 MB release binary (vs ~25 MB full).
- [ ] **Tests** — Builds without tools feature. Remote tool delegation round-trip. Gateway starts in lite mode. 4+ tests.

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
- [ ] **Tests** — Config loader reads from mock `/run/secrets/` path. 2+ tests.

### Phase D: Operational Runbooks (LOW)

- [x] **Incident response** — `docs/runbooks/incident-response.md`.
- [x] **Backup & recovery** — `docs/runbooks/backup-recovery.md`.
- [x] **Monitoring setup** — `docs/runbooks/monitoring.md`.
- [x] **Scaling** — `docs/runbooks/scaling.md`.

### Phase E: E2E Testing with Local LLM (MEDIUM)

- [ ] **CI-integrated e2e tests** — GitHub Actions job using Ollama + tinyllama (or similar small model). Tests run against real LLM completions.
- [ ] **Test coverage** — Provider completion, streaming, tool use, multi-turn conversation.
- [ ] **Orchestrator routing test** — Real LLM classification for agent routing decisions.

---

### Acceptance Criteria (Sprint 42)

- [x] Lightweight binary created (size optimization pending)
- [x] Remote tool execution endpoint built (`POST /v1/tool-execute`)
- [x] 5 example directories with working configs and READMEs
- [x] Docker Secrets fallback chain works (env → secret → config)
- [x] 4 operational runbooks cover incident, backup, monitoring, scaling
- [ ] E2E tests pass with real local LLM
- [ ] All quality gates pass: `cargo clippy`, `cargo test --workspace`, 0 warnings

---

## Sprint 43: Agent-as-a-Service — Runtime Agent CRUD, Webhook Proxy, Platform Auto-Registration

**Goal:** Enable instant agent deployment via API. Users create agents at runtime through `POST /v1/agents` with a name, personality, provider, and channel tokens. Agents register with the swarm coordinator, platform webhooks are auto-configured, and messages route to the correct agent. No gateway restart required.

**Baseline:** Sprint 42 planned. All prior sprints complete (AI tool selection, gossip event bus, CLI API key management, WhatsApp/SMS channels, CI/CD hardening, security/observability, persistent API keys).

**Plan:** `specs/plans/12-agent-as-a-service.md`

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

**Plan:** `specs/plans/agent-manage-cli-configui.md`

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

**Plan:** `specs/plans/platform-control-ui.md`

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

## Backlog

### Embedded Binary Size Reduction (HIGH)

Reduce the `embedded` profile binary for resource-constrained devices. Currently 10.1MB (budget temporarily at 11MB), target 5-8MB. Phased approach: feature-gate tools into tiers, add plain SQLite option (no sqlcipher), make WASM plugins optional, minimize reqwest features, audit with cargo-bloat.

**Plan:** `specs/plans/embedded-binary-size-reduction.md`

- [ ] **Phase 1: Tool tiering** — Split tool registration into `core`/`extended`/`full` tiers. Embedded compiles only `core` (file ops, shell, memory, sub-agents). Target: -500KB to -1MB.
- [ ] **Phase 2: Plain SQLite** — Add `memory-sqlite-plain` feature without bundled-sqlcipher encryption. Target: -2MB.
- [ ] **Phase 3: Optional WASM** — Create `embedded-minimal` (no WASM) and keep `embedded` with WASM. Target: -300KB.
- [ ] **Phase 4: HTTP client minimization** — Audit and trim reqwest features; evaluate `ureq` for embedded. Target: -200KB.
- [ ] **Phase 5: cargo-bloat audit** — Profile with `cargo bloat --release --crates`, eliminate hidden size contributors.
- [ ] **Phase 6: Binary compression** — Evaluate UPX for deployment-time compression.
- [ ] **CI: cargo-bloat report** — Add size breakdown as CI artifact for tracking trends.

### TUI Dashboard Enhancement

Upgrade the Ratatui CLI dashboard with live data from gateway APIs. Tab-based navigation (Overview, Runs, Agents, Events), HTTP client for gateway polling, auto-refresh via `tokio::select!`, and regression warnings. See Sprint 47 Phase D.

- [ ] Tab-based navigation with `DashboardTab` enum and ratatui `Tabs` widget
- [ ] HTTP client using daemon host/port + `reqwest::Client`
- [ ] Auto-refresh architecture with `mpsc` channels + background polling tasks
- [ ] Runs tab: `Table` widget with status colors, cost, duration
- [ ] Agents tab: agent list with active run counts
- [ ] Events tab: scrolling SSE event stream with topic color coding
- [ ] Regression warnings in Overview tab

### Lightweight Orchestrator Mode

A minimal binary that runs only the orchestrator (routing, coordination, event bus) without bundling tool runners, CLI, or TUI. Designed for resource-constrained edge devices. See Sprint 39 Phase H for details.

### Examples Directory

Comprehensive examples with READMEs demonstrating key use cases: research-pipeline, business-office, chatbot, multi-agent-team, edge-deployment. See Sprint 39 Phase I for details.

### Operational Runbooks

Incident response, backup & recovery, monitoring setup, and scaling runbooks. See Sprint 39 Phase M for details.

### E2E Testing with Local LLM

CI-integrated end-to-end tests using a real local LLM server.

- [ ] CI-integrated e2e tests using Ollama + tinyllama
- [ ] Real provider completion, streaming, tool use, multi-turn tests
- [ ] Orchestrator routing test with real LLM classification

### Fleet Mode (mvmctl + mvmd Integration)

Agent-as-a-Service backed by Firecracker microVM isolation via mvmctl/mvmd. Feature-gated behind `"fleet"`.

- [ ] AgentStore backend that delegates to mvmd for Firecracker-based isolation
- [ ] Warm sandbox pool integration (sub-second agent provisioning)
- [ ] Sleep/wake with wake-on-message (webhook triggers snapshot restore)
- [ ] agentzero Firecracker template (Nix flake for rootfs)
- [ ] Config/secrets drive injection
- [ ] Autoscaling across cloud providers (Hetzner, AWS, GCP, DigitalOcean)
- [ ] Per-agent Turso auto-provisioning for memory durability across instances

### Multi-Node Orchestration (Full Distributed)

Full multi-node distributed orchestration beyond gossip event bus. See `specs/sprints/backlog.md` for details.

- [ ] Node registry (capabilities, health status)
- [ ] Task routing to best-fit node
- [ ] Result aggregation from distributed sub-agents
- [ ] Remote delegation with `node` parameter
- [ ] Gateway `node_control` endpoint
