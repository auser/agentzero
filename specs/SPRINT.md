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

- [ ] **`EventBus` trait** — `publish(topic, payload)`, `subscribe(topic) -> Receiver`, `replay_since(topic, event_id) -> Vec<Event>`. `Event` struct: `id`, `topic`, `payload`, `timestamp`, `source_node`. In `agentzero-core`.
- [ ] **`InMemoryEventBus`** — Backed by `tokio::sync::broadcast`. No persistence. For single-process/testing use.
- [ ] **`SqliteEventBus`** — New table in `agentzero-storage`. WAL mode for concurrent reads. `last_seen_id` tracking per subscriber. Configurable poll interval and retention. GC method. 6+ tests.
- [ ] **`GossipEventBus`** — TCP mesh layer. Each node listens on configurable port. Broadcasts events to peers via length-prefixed bincode frames. Deduplication via event ID set (bounded LRU). Peer health via periodic ping. 4+ tests.
- [ ] **Config** — `[orchestrator] event_bus = "memory" | "sqlite" | "gossip"` with `event_retention_days`, `gossip_port`, `gossip_peers`. Defaults to `"memory"`.
- [ ] **Integration** — Wire `EventBus` into `JobStore` (publish on state transitions), `PresenceStore` (publish heartbeats), gateway SSE/WebSocket (subscribe for real-time push). Coordinator consumes events for cross-instance awareness.
- [ ] **Tests** — Trait compliance tests for all 3 impls. SQLite replay test. GC test. Gossip dedup test. Integration: job state change → event published → subscriber receives.

### Phase B: Request Body Schema Validation (MEDIUM)

Replace untyped `Json<Value>` handlers with strongly-typed request structs.

- [ ] **Typed request structs** — `ChatRequest`, `RunSubmitRequest`, `WebhookRequest`, `ApiKeyCreateRequest`, `PairRequest` in `gateway::models`. All fields validated with serde `#[validate]` or custom deserialize. Invalid payloads return 400 with field-level error messages.
- [ ] **Webhook payload validation** — Webhook `channel` validated against existing channel name rules. Payload size limit (1 MB default, configurable).
- [ ] **Tests** — Missing required fields → 400. Invalid types → 400. Oversized payload → 413. Valid payloads accepted. 8+ tests.

### Phase C: Circuit Breaker Transparent Wiring (MEDIUM)

Currently callers must manually `.check()` the circuit breaker. Wrap it transparently.

- [ ] **Transparent circuit breaker** — Wrap provider `complete()` / `complete_streaming()` / `complete_with_tools()` calls inside the circuit breaker automatically. Remove manual `.check()` calls. Circuit state transitions logged at `info!`/`warn!`.
- [ ] **Half-open probe** — Single probe request on half-open. Success closes, failure reopens. Configurable reset duration.
- [ ] **Tests** — 4 tests: open circuit rejects, half-open probes, successful probe closes, failed probe reopens.

### Phase D: Liveness Probe (MEDIUM)

- [ ] **`GET /health/live`** — Liveness probe checking tokio runtime health (spawns a trivial task, confirms completion within 1s). Distinct from `/health` (static) and `/health/ready` (dependency checks).
- [ ] **Tests** — 2 tests: healthy runtime returns 200, probe timeout behavior.

### Phase E: Turso Migrations (MEDIUM)

- [ ] **Migration versioning for Turso** — Port `_schema_version` table and versioned migration tracking from SQLite to `TursoMemoryStore`. Same migration framework, different connection handling.
- [ ] **Tests** — Migration runs once, version tracked, re-run is no-op. 3 tests (behind `memory-turso` feature flag).

### Phase F: Multi-Tenancy Deepening (HIGH)

- [ ] **Org isolation on JobStore** — All job queries filter by `org_id` extracted from API key metadata. `ApiKey` struct gains `org_id: Option<String>` field. Jobs inherit org_id from the creating API key.
- [ ] **Per-org conversation memory** — `MemoryStore` queries scoped by org_id prefix on conversation_id. Org A cannot read Org B's transcripts.
- [ ] **CLI: `auth api-key create/revoke/list`** — CLI commands for API key lifecycle management. `create` generates key with specified scopes and optional org_id. `revoke` deactivates. `list` shows active keys (masked). Wired to persistent `ApiKeyStore`.
- [ ] **Tests** — Org isolation: job from org A invisible to org B. Memory isolation. API key CRUD. 8+ tests.

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

### Phase L: Operational Runbooks (LOW)

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
- [ ] Turso migrations tracked with version table
- [ ] Org isolation prevents cross-tenant data access
- [ ] API key CLI commands manage full key lifecycle
- [ ] AI tool selector reduces tool set passed to provider
- [ ] Lightweight binary builds under 10 MB without tool/plugin crates
- [ ] 5 example directories with working configs and READMEs
- [ ] Container scanning blocks CRITICAL CVEs in CI
- [ ] SBOM generated on release
- [ ] Fuzz targets cover HTTP, provider parsing, config, WebSocket
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
