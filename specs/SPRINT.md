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
- [ ] **`GossipEventBus`** — TCP mesh layer. Each node listens on configurable port. Broadcasts events to peers via length-prefixed bincode frames. Deduplication via event ID set (bounded LRU). Peer health via periodic ping. 4+ tests.
- [x] **Config** — `[swarm] event_bus = "memory" | "file" | "sqlite"` with `event_retention_days`, `event_db_path`. Defaults to `"memory"`. Backward-compatible: `event_log_path` still selects file backend.
- [ ] **Integration** — Wire `EventBus` into `JobStore` (publish on state transitions), `PresenceStore` (publish heartbeats), gateway SSE/WebSocket (subscribe for real-time push). Coordinator consumes events for cross-instance awareness.

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
- [ ] **CLI: `auth api-key create/revoke/list`** — CLI commands for API key lifecycle management. `create` generates key with specified scopes and optional org_id. `revoke` deactivates. `list` shows active keys (masked). Wired to persistent `ApiKeyStore`.
- [x] **Tests** — Org isolation: job from org A invisible to org B (7 tests). Memory isolation: org-scoped queries, conversation isolation, roundtrip (4 tests). API key CRUD deferred to CLI phase.

### Phase G: AI-Based Tool Selection (HIGH)

When an agent has access to many tools, use AI to select relevant tools by name and description rather than passing all tools to every provider call.

- [ ] **`ToolSelector` trait** — `select(task_description, available_tools) -> Vec<ToolDef>`. Input: task/message text + list of `(name, description)` pairs. Output: ranked subset of relevant tools.
- [ ] **`AiToolSelector`** — Uses a lightweight LLM call (provider's cheapest model or builtin) to classify which tools are relevant. Prompt: "Given this task, select the most relevant tools from this list." Returns tool names. Cached per unique task hash for the session.
- [ ] **`KeywordToolSelector`** — Fallback: keyword/TF-IDF matching on tool descriptions. No LLM call needed. Fast but less accurate.
- [ ] **Integration** — `Agent::respond_with_tools()` optionally runs tool selection before provider call when `tool_selection = "ai" | "keyword" | "all"` (default: `"all"` for backward compat). Selected tools passed to provider instead of full set.
- [ ] **Config** — `[agent] tool_selection = "all" | "ai" | "keyword"`, `tool_selection_model` (optional override).
- [ ] **Tests** — AI selector picks relevant tools. Keyword selector matches on description. "all" mode passes everything. Cache hit on repeated task. 6+ tests.

### Phase H: Lightweight Orchestrator Mode (HIGH)

A minimal binary that runs only the orchestrator (routing, coordination, event bus) without bundling tool runners, CLI, or TUI. Designed for resource-constrained edge devices.

- [ ] **`agentzero-lite` binary** — New binary target in `bin/agentzero-lite/` that depends only on `agentzero-core`, `agentzero-config`, `agentzero-providers`, `agentzero-infra` (orchestrator subset), and `agentzero-gateway`. Excludes: `agentzero-tools` (heavy), `agentzero-channels`, `agentzero-plugins`, `agentzero-cli`, `agentzero-ffi`. Feature-gated tools replaced with remote delegation stubs.
- [ ] **Remote tool execution** — Lightweight mode delegates tool execution to full-featured nodes via HTTP (`POST /v1/tool-execute` on a peer). Config: `[orchestrator] tool_mode = "local" | "remote"` with `tool_remote_url`.
- [ ] **Minimal feature set** — Orchestrator, gateway, provider calls, event bus, delegation. No local tool execution, no TUI, no WASM plugins.
- [ ] **Binary size target** — Under 10 MB release binary (compared to ~25 MB full).
- [ ] **Tests** — Builds without tools feature. Remote tool delegation round-trip. Gateway starts in lite mode. 4+ tests.

### Phase I: Examples Directory (MEDIUM)

Comprehensive examples with READMEs demonstrating key use cases.

- [ ] **`examples/research-pipeline/`** — Already exists. Review and update README with current API. Ensure it runs against current codebase.
- [ ] **`examples/business-office/`** — Already exists. Review and update. This is the "1-click AI business" pattern: CEO agent delegates to CTO, CFO, CMO, CSO sub-agents. Each has role-specific tools and autonomy policies. Demonstrates hierarchical delegation, budget enforcement, and cascade stop.
- [ ] **`examples/chatbot/`** — Simple single-agent chatbot with tool use. Minimal config. Good first example for new users.
- [ ] **`examples/multi-agent-team/`** — Team of specialized agents (researcher, writer, reviewer) collaborating on a task via lane-based routing. Demonstrates queue modes, collect, and followup.
- [ ] **`examples/edge-deployment/`** — Lightweight mode on a Raspberry Pi or similar. Shows `agentzero-lite` config + remote tool execution to a full node.
- [ ] **Each example** has: `README.md` (purpose, setup, run instructions, architecture diagram in ASCII), `config.toml`, and any necessary agent definition files.

### Phase J: CI/CD Hardening (MEDIUM)

- [ ] **Container image scanning** — Add Trivy or Grype step in CI (GitHub Actions) that scans the Docker image on every push to main. Fail on CRITICAL/HIGH CVEs.
- [ ] **SBOM generation** — CycloneDX SBOM generated in release pipeline via `cargo-cyclonedx`. Published as release artifact.
- [ ] **Docker secrets** — Document and support Docker Secrets for API keys instead of plain environment variables. `docker-compose.yml` updated with secrets section. Config loader reads from `/run/secrets/` when available.

### Phase K: Fuzzing (LOW)

- [ ] **`cargo-fuzz` targets** — Fuzz targets for: HTTP request parsing (gateway handlers), provider response parsing (Anthropic/OpenAI JSON), TOML config parsing, WebSocket frame handling. In `fuzz/` directory.
- [ ] **CI integration** — Nightly fuzzing job (GitHub Actions) runs each target for 5 minutes. Corpus committed to repo.
- [ ] **Tests** — Fuzz targets compile and run for 10 seconds without panic.

### Phase L: WhatsApp & SMS Channels (MEDIUM)

Wire the existing WhatsApp Cloud API channel into the config pipeline and add a new Twilio SMS channel.

**Plan:** `specs/plans/11-whatsapp-sms-channels.md`

- [ ] **WhatsApp wiring** — Add `"whatsapp"` arm to `register_one()` in `channel_setup.rs`. Maps `access_token`, `channel_id` → `phone_number_id`, `token` → `verify_token`. 2 tests.
- [ ] **`ChannelInstanceConfig` new fields** — `account_sid: Option<String>`, `from_number: Option<String>` for Twilio SMS.
- [ ] **`sms.rs`** — New Twilio SMS channel: `send()` via Twilio REST API (Basic auth, form-encoded body, 1600-char chunking), `listen()` webhook stub, `health_check()`. 4+ tests.
- [ ] **Feature flag** — `channel-sms = ["reqwest"]` in `Cargo.toml`. Add to `channels-standard` and `all-channels`.
- [ ] **Catalog + registration** — `sms => (SmsChannel, SMS_DESCRIPTOR)` in `channel_catalog!`; `"sms"` arm in `register_one()`.

### Phase M: Operational Runbooks (LOW)

- [ ] **Incident response runbook** — `docs/runbooks/incident-response.md`: E-stop procedure, provider failover, how to inspect stuck jobs, log locations, escalation contacts template.
- [ ] **Backup & recovery runbook** — `docs/runbooks/backup-recovery.md`: Scheduled backup via cron, restore procedure, integrity verification, encrypted export format details.
- [ ] **Monitoring setup runbook** — `docs/runbooks/monitoring.md`: Prometheus scrape config, key metrics to alert on (`provider_errors_total`, `rate_limit_429_total`, `circuit_breaker_open`), Grafana dashboard JSON template.
- [ ] **Scaling runbook** — `docs/runbooks/scaling.md`: When to scale (metrics thresholds), horizontal scaling with gossip event bus, lightweight mode for edge nodes, provider fallback chain setup.

---

### Acceptance Criteria

- [ ] Embedded event bus works with SQLite persistence (no Redis)
- [ ] Gossip layer enables multi-instance event propagation over TCP
- [ ] All request handlers use typed structs with validation
- [ ] Circuit breaker wraps provider calls transparently
- [ ] Liveness probe verifies async runtime health
- [x] Turso migrations tracked with version table
- [x] Org isolation prevents cross-tenant data access
- [ ] API key CLI commands manage full key lifecycle
- [ ] AI tool selector reduces tool set passed to provider
- [ ] Lightweight binary builds under 10 MB without tool/plugin crates
- [ ] 5 example directories with working configs and READMEs
- [ ] Container scanning blocks CRITICAL CVEs in CI
- [ ] SBOM generated on release
- [ ] Fuzz targets cover HTTP, provider parsing, config, WebSocket
- [ ] WhatsApp Cloud API channel wired and config-registered
- [ ] SMS (Twilio) channel sends and health-checks via REST API
- [ ] Both channels in `channels-standard` and `all-channels` feature sets
- [ ] 4 operational runbooks cover incident, backup, monitoring, scaling
- [ ] All quality gates pass: `cargo clippy`, `cargo test --workspace`, 0 warnings

---

## Backlog

### E2E Testing with Local LLM

CI-integrated end-to-end tests using a real local LLM server.

- [ ] CI-integrated e2e tests using Ollama + tinyllama
- [ ] Real provider completion, streaming, tool use, multi-turn tests
- [ ] Orchestrator routing test with real LLM classification

### Multi-Node Orchestration (Full Distributed)

Full multi-node distributed orchestration beyond gossip event bus. See `specs/sprints/backlog.md` for details.

- [ ] Node registry (capabilities, health status)
- [ ] Task routing to best-fit node
- [ ] Result aggregation from distributed sub-agents
- [ ] Remote delegation with `node` parameter
- [ ] Gateway `node_control` endpoint
