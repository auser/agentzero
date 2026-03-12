# AgentZero Sprint Plan

Previous sprints archived to `specs/sprints/25-32-privacy-e2e-multi-agent-production.md`.

---

## Sprint 33: Multi-Agent Gaps — Queue Modes, Cascade Stop, Tool-Loop Detection

**Goal:** Close the remaining gaps in our multi-agent orchestration. Sprint 32.5 (merged on `feat/examples-and-gaps`) delivered lanes, depth-gated tools, announce-back, async jobs API, and WebSocket run subscription. This sprint covers the behavioral and safety features that make multi-agent systems robust in production.

**Baseline:** 17-crate workspace, v0.4.2, 544+ tests in modified crates, 0 clippy warnings. Lane-based queue serialization, fan-out dispatcher, JobStore with cancel/list/events, depth-gated tool filtering, announce-back pattern, and `/ws/runs/:run_id` all implemented.

---

### Phase A: Queue Modes (steer / followup / collect / interrupt)

The queue system supports four message routing modes that determine how messages are delivered to agents within a lane:

- **steer** (default) — Route message to a single agent based on AI router classification.
- **followup** — Append to an existing run's conversation rather than starting a new one. Useful for multi-turn interactions with a specific agent.
- **collect** — Fan-out to all agents in a lane, collect all responses, merge into a single result. Used for parallel research or voting patterns.
- **interrupt** — Preempt the current running agent in a lane, cancelling its in-flight work. Used for priority overrides.

**Tasks:**

- [x] **QueueMode enum** — `QueueMode { Steer, Followup { run_id }, Collect, Interrupt }` in `agentzero-core/src/types.rs`. Wired into `WorkItem.queue_mode`. Serde round-trip tests.
- [x] **Followup mode** — Gateway `async_submit` detects `Followup` mode, validates the target run exists, and passes `conversation_id` to the agent so it appends to the existing conversation. 3 tests: missing run_id → 400, unknown run_id → 404, valid run_id → accepted.
- [x] **Collect mode** — Gateway `async_submit` with `collect` mode spawns N parallel agent invocations, collects all responses, and merges them into a single result with per-agent sections.
- [x] **Interrupt mode** — Gateway `async_submit` with `interrupt` mode cancels all active (non-terminal) runs in the job store before submitting the new job. Test: 2 running jobs → both cancelled after interrupt.
- [x] **Gateway API** — `POST /v1/runs` accepts optional `mode` field (`steer`, `followup`, `collect`, `interrupt`) and `run_id` for followup. `DELETE /v1/runs/:run_id` accepts optional `cascade` query param.
- [x] **Tests** — 4 queue mode tests: followup requires run_id, followup unknown 404, followup valid accepted, interrupt cancels active. Plus existing steer/collect path through 503 (no config).

### Phase B: Cascade Stop

When a parent agent is cancelled, all its descendant sub-agents must also be cancelled recursively. Currently `JobStore::cancel()` only cancels a single run.

**Tasks:**

- [x] **`cascade_cancel()`** — BFS traversal cancels a run and all descendants where `parent_run_id` matches. Skips already-terminal jobs. 3 tests: parent+children, 3-level deep, skips-terminal.
- [x] **CancellationToken propagation** — `ToolContext.cancelled` `Arc<AtomicBool>` flag checked between tool iterations in `Agent::respond_with_tools()`. `TaskMessage` carries a per-task cancellation flag wired into the `ToolContext` by the coordinator. Agent returns early with `[Execution cancelled]` when flag is set.
- [x] **Gateway wiring** — `DELETE /v1/runs/:run_id?cascade=true` calls `cascade_cancel()`. Returns `cascade_count` and `cancelled_ids`. API test.
- [x] **E-stop integration** — `POST /v1/estop` gateway endpoint triggers `emergency_stop_all()` on JobStore, which cascade-cancels all active root-level runs and their descendants. 3 API tests + 2 unit tests.
- [x] **Tests** — 3-level deep hierarchy (parent → child → grandchild), cancel parent, verify all three cancelled. Skip-already-terminal test. Gateway cascade cancel API test.

### Phase C: Tool-Loop Detection

Three detectors with tiered escalation prevent agents from getting stuck in repetitive tool-call loops:

1. **Exact repeat detector** — Same tool + same arguments N times in a row (default N=3). Escalation: inject a system message telling the agent to try a different approach.
2. **Semantic similarity detector** — Tool calls with >90% argument similarity over a sliding window. Escalation: reduce available tools (remove the looping tool for the next iteration).
3. **Cost runaway detector** — Token spend exceeds budget threshold within a time window. Escalation: force-complete the run with a budget-exceeded error.

**Tasks:**

- [x] **`ToolLoopDetector`** — Stateful detector in `agentzero-orchestrator::loop_detection`. `LoopAction` enum in `agentzero-core`: `Continue`, `InjectMessage`, `RestrictTools`, `ForceComplete`. Highest-severity action wins.
- [x] **Exact repeat detector** — Tracks last N tool calls. Configurable threshold (default 3). Triggers `InjectMessage`.
- [x] **Similarity detector** — Jaccard bigram similarity on serialized arguments. Configurable threshold (default 0.9) and window size (default 5). Triggers `RestrictTools`.
- [x] **Cost runaway detector** — Checks tokens_used and cost_microdollars against configurable per-run limits. Triggers `ForceComplete`.
- [x] **Agent worker integration** — `ToolLoopDetector` moved to `agentzero-core::loop_detection` and integrated into `Agent::respond_with_tools()`. After each tool-call batch, detectors run and the highest-severity action is applied: `InjectMessage` adds a system notice, `RestrictTools` filters the tool from subsequent provider calls, `ForceComplete` forces a final response without tools.
- [x] **Tests** — 10 unit tests: exact repeat (trigger, reset, different-args), similarity (trigger, no-trigger), cost (token-limit, cost-limit, disabled), severity ordering, Jaccard similarity edge cases.

### Phase D: Persistent Event Log & Presence Tracking

Currently job events are reconstructed from `JobRecord` state. A persistent event log per run with agent presence tracking via TTL is needed.

**Tasks:**

- [x] **`EventLog`** — Append-only log in `job_store.rs`. `EventKind`: Created, Running, ToolCall, ToolResult, Completed, Failed, Cancelled. Auto-recorded on submit and status transitions. GC cleans up event logs with expired jobs.
- [x] **Event recording** — `record_tool_call()` and `record_tool_result()` methods on `JobStore`. `GET /v1/runs/:run_id/events` returns the persistent log (replaced reconstructed approach). API test verifies tool call events appear.
- [x] **Presence tracking** — `PresenceStore` with `register`, `heartbeat`, `is_alive`, `status`, `list_all`, `deregister`, `gc_expired`. TTL-based status: Alive/Stale/Dead. 7 tests.
- [x] **Gateway endpoint** — `GET /v1/agents` returns registered agents with presence status. 2 API tests (with presence, without presence store).
- [x] **Coordinator wiring** — Agent workers register/heartbeat/deregister with `PresenceStore`. Coordinator filters dead agents from routing candidates via `is_alive()`. `Coordinator::with_presence()` builder method.

### Phase E: Block Streaming (Markdown-Aware Chunking)

Streaming doesn't just forward raw SSE tokens — it groups them into semantic blocks (paragraphs, code fences, lists) so subscribers receive coherent chunks.

**Tasks:**

- [x] **`BlockAccumulator`** — Stateful accumulator in `agentzero-orchestrator::block_stream`. Recognizes: paragraphs (double newline), code fences with language, headers (# through ######), list items (-, *, numbered). Handles incremental token feeding and unclosed code blocks on flush.
- [x] **WebSocket integration** — `/ws/runs/:run_id?format=blocks` uses `BlockAccumulator` to send block-level JSON frames (paragraph, code_block, header, list_item) instead of raw status frames for completed results.
- [x] **SSE endpoint** — `GET /v1/runs/:run_id/stream` as an alternative to WebSocket. Uses `text/event-stream` with JSON data frames. Supports `?format=blocks` for block-level chunking. 3 tests: SSE completed, SSE blocks format, unknown 404.
- [x] **Tests** — 11 tests: paragraph detection, code block (complete + incremental + unclosed), headers, list items (bullet + numbered), mixed content, flush behavior, empty input.

---

### Acceptance Criteria

- [x] All four queue modes work: steer routes to one agent, followup appends to existing run, collect merges all responses, interrupt cancels and replaces
- [x] Cascade cancel propagates through 3+ levels of sub-agent hierarchy
- [x] Tool-loop detection escalates through inject → restrict → force-complete; integrated into Agent tool loop
- [x] Persistent event log records tool calls and status transitions; queryable via API
- [x] Presence tracking with TTL; dead agents queryable via `/v1/agents` API; coordinator skips dead agents
- [x] Block streaming delivers semantic chunks over WebSocket and SSE
- [x] All quality gates pass: `cargo clippy`, `cargo test --workspace` (1,360 tests, 0 warnings)

---

---

## Sprint 34: Delegation Security Hardening

**Goal:** Close the Layer 1 delegation security gaps. Prevent privilege escalation, cost amplification, credential exfiltration, and resource exhaustion when delegating to sub-agents.

**Baseline:** Sprint 33 complete (1,360 tests), all orchestration behavioral features shipped.

---

### Phase A: Autonomy Policy Intersection

- [x] **`AutonomyPolicy::intersect()`** — Produces a child policy that is at least as restrictive as the parent on every dimension. Level: most restrictive wins. Forbidden paths: union. Allowed roots: intersection. Auto-approve: intersection. Always-ask: union. Sensitive file access: AND. 8 tests.
- [x] **`AutonomyLevel::most_restrictive()`** — Helper that returns the more restrictive of two levels (ReadOnly > Supervised > Full).

### Phase B: Delegate Tool Security Mitigations

- [x] **Autonomy intersection in delegation** — `DelegateTool` accepts parent `AutonomyPolicy` via `with_parent_policy()`. Before building the sub-agent's tool set, intersects parent policy with child default. Tools blocked by the intersected policy are removed from the sub-agent. Prevents privilege escalation.
- [x] **Leak guard on delegation results** — `DelegateTool` accepts an `OutputScanner` closure via `with_output_scanner()`. After sub-agent execution, output is scanned for credentials. Scanner returns `Ok(safe_text)` (redacted) or `Err(reason)` (blocked). Wired to `LeakGuardPolicy::process()` at the integration point.
- [x] **CancelToken propagation** — `DelegateTool::execute()` checks `ctx.is_cancelled()` before starting. Child context inherits parent's `cancelled` `Arc<AtomicBool>` via `ctx.clone()`, so e-stop reaches sub-agents.
- [x] **Concurrency semaphore** — `DelegateTool` uses `tokio::sync::Semaphore` (default 4) via `with_max_concurrent()`. Blocks if at limit, preventing resource exhaustion from width explosion.
- [x] **Depth tracking** — Child context `depth` is incremented via `saturating_add(1)`.

### Acceptance Criteria

- [x] `AutonomyPolicy::intersect()` produces most-restrictive-wins policy (8 tests)
- [x] Read-only parent blocks write tools in child delegation
- [x] Cancelled parent prevents delegation from starting
- [x] Output scanner redacts/blocks credential leaks in sub-agent output
- [x] Concurrency semaphore limits parallel delegations
- [x] All quality gates pass: `cargo clippy`, `cargo test --workspace` (1,376 tests, 0 warnings)

---

### Sprint 35 — Hierarchical Budgeting

**Goal:** Token/cost budgeting flows from parent to child agents with automatic aggregation and enforcement.

**Changes:**
- [x] `ChatResult` carries `input_tokens` and `output_tokens` from provider responses
- [x] Anthropic provider extracts token usage from non-streaming and streaming (`message_start`/`message_delta`) responses
- [x] `ToolContext` budget fields: `tokens_used`, `cost_microdollars` (`Arc<AtomicU64>`), `max_tokens`, `max_cost_microdollars` with helper methods
- [x] `DelegateConfig` budget fields: `max_tokens`, `max_cost_microdollars`
- [x] `AgentError::BudgetExceeded` variant for clean error propagation
- [x] `Agent::respond_with_tools()` accumulates tokens after each provider call, checks budget limits, passes actual values to loop detector
- [x] `DelegateTool::execute()` creates fresh child accumulators, sets child budget limits (explicit or inherited remaining), aggregates child usage back to parent
- [x] All quality gates pass: `cargo clippy`, `cargo test --workspace` (0 warnings)

---

## Sprint 36: Production Hardening

**Goal:** Close the remaining production-readiness gaps: transcript archival, connection pooling, API security, health probes, observability wiring, distributed event bus, and multi-tenancy foundations.

**Baseline:** Sprint 35 complete (hierarchical budgeting), all delegation security mitigations shipped, 0 clippy warnings.

---

### Phase 0: Production Hardening (Message TTL, Claim Locks, Directive Integrity)

Three targeted hardening features for multi-agent coordination and security.

- [x] **Message TTL / ephemeral messages** — `MemoryEntry.expires_at: Option<i64>` (unix timestamp). Expired entries excluded from all queries (`WHERE expires_at IS NULL OR expires_at > unixepoch()`). `MemoryStore::gc_expired()` trait method deletes expired rows. Migration, SqliteMemoryStore, PooledMemoryStore all updated. 4 tests.
- [x] **Job claim locks** — `JobStore::try_claim(run_id, agent_id) -> bool` atomically transitions Pending→Running with agent attribution. Prevents Steer-mode race between routing and execution. `JobRecord.claimed_by: Option<String>` for audit. 5 tests.
- [x] **Directive integrity verification** — `compute_prompt_hash()` / `verify_prompt_hash()` (HMAC-SHA256) for system prompts. `DelegateConfig.system_prompt_hash` verified in `validate_delegation()`. Constant-time comparison via hmac crate. Backward-compatible (None hash = skip check). 7 tests.

### Phase A: Sub-Agent Transcript Archival

When a sub-agent completes, archive its full conversation to storage keyed by run_id so parent or operator can inspect what happened.

**Current state:** `MemoryStore::recent_for_conversation()` exists and the gateway already uses `run_id` as `conversation_id`. Missing: dedicated archive/export API, run metadata on entries.

**Tasks:**

- [x] **`GET /v1/runs/:run_id/transcript`** — New gateway endpoint that calls `memory_store.recent_for_conversation(run_id, limit)` and returns the full conversation as JSON array of `{ role, content, timestamp }` objects. Auth-gated. 404 if no entries found.
- [x] **`TranscriptEntry` response model** — Lightweight struct in `gateway::models` with `role`, `content`, `created_at` fields. Serialized from `MemoryEntry`.
- [x] **Timestamp on MemoryEntry** — Add `created_at: Option<String>` field to `MemoryEntry` (populated from SQLite `created_at` column on retrieval). Update `SqliteMemoryStore::recent_for_conversation()` to include it.
- [x] **DelegateTool transcript wiring** — After sub-agent completes in `DelegateTool::execute()`, generate a unique child `conversation_id` (`delegate-{agent}-{depth}-{nanos}`) and log it via `tracing::info!` so it's discoverable from parent's event log/trace.
- [x] **Tests** — Gateway API test: create run with transcript entries, retrieve via endpoint. Empty transcript 404 test. Auth test.

### Phase B: Database Connection Pooling

Replace single `Mutex<Connection>` with r2d2 connection pool for SQLite. Eliminates lock contention under concurrent requests.

**Current state:** `SqliteMemoryStore` wraps `Mutex<Connection>`. Lock acquired per query, panics on poison. No pooling, no connection limits.

**Tasks:**

- [x] **Add `r2d2` + `r2d2_sqlite` dependencies** to `agentzero-storage/Cargo.toml`.
- [x] **`PooledMemoryStore`** — New implementation of `MemoryStore` backed by `r2d2::Pool<SqliteConnectionManager>`. Pool size configurable (default 4, max 16). Each method calls `pool.get()` instead of `mutex.lock()`.
- [x] **Migration on pool init** — Run the same schema migrations on first connection from pool (via `r2d2::CustomizeConnection` or init hook).
- [x] **SQLCipher support** — `SqliteConnectionManager` custom initialization callback that runs `PRAGMA key` on each new connection.
- [x] **Config wiring** — Add `pool_size: usize` to memory config. `build_memory_store()` selects `PooledMemoryStore` when pool_size > 1, falls back to existing `SqliteMemoryStore` for pool_size=1.
- [x] **Tests** — Concurrent write test (spawn 10 tasks writing simultaneously). Pool exhaustion behavior test. Migration runs once test.

### Phase C: API Security & Health Probes

Fix timing attack vulnerability in token comparison, add readiness probe with dependency checks, add version info.

**Current state:** `auth.rs` uses `==` for token comparison (timing-vulnerable). `/health` returns static `"ok"` with no dependency checks. No readiness endpoint.

**Tasks:**

- [x] **Constant-time token comparison** — Replace `expected.as_str() == token` in `auth.rs` with `subtle::ConstantTimeEq` (add `subtle` crate dependency). Apply to both bearer and paired token paths.
- [x] **`GET /health/ready`** — Readiness probe that checks: (1) memory store is connectable (try `recent(1)`), (2) at least one provider is configured. Returns `{ ready: true/false, checks: { memory: "ok"/"error: ...", provider: "ok"/"error: ..." } }`. Separate from liveness `/health`.
- [x] **Version in health response** — Add `version: &'static str` to `HealthResponse`, populated from `env!("CARGO_PKG_VERSION")`.
- [x] **Auth audit logging** — `tracing::warn!` on failed auth attempts with IP (from headers) and reason. Rate-limit the log to prevent log flooding.
- [x] **Tests** — Timing-safe comparison unit test (subtle crate). Readiness probe: healthy returns ready=true, broken store returns ready=false. Version appears in health response.

### Phase D: OpenTelemetry Wiring

Wire the existing `[observability]` config skeleton to a real OTLP exporter with span-per-run tracing.

**Current state:** `ObservabilityConfig` has `backend`, `otel_endpoint`, `otel_service_name` fields but they're unused. Runtime tracing goes to a JSONL file only.

**Tasks:**

- [x] **Add `opentelemetry`, `opentelemetry-otlp`, `opentelemetry_sdk`, `tracing-opentelemetry` dependencies** to `agentzero-infra/Cargo.toml` behind a `telemetry` feature flag.
- [x] **`init_telemetry(config)` function** — In `agentzero-infra::telemetry`, initialize OTLP exporter when `backend = "otlp"`. Configure batch span processor, resource attributes (service name, version). Returns a guard that flushes on drop.
- [x] **Span-per-run** — In `Agent::respond()` / `respond_streaming()`, create `agent_run` span with `request_id`, `depth`, `conversation_id` attributes. Provider calls get `provider_call` child spans with iteration and tool count.
- [x] **Span-per-tool-call** — `Agent::execute_tool()` creates `tool_call` child span with tool name, request_id, and iteration.
- [x] **Config integration** — Gateway `run()` calls `init_telemetry()` on startup when `telemetry` feature is enabled. Feature plumbed through `bin/agentzero` → `agentzero-cli` → `agentzero-gateway` → `agentzero-infra`.
- [x] **Tests** — Unit test: `init_telemetry` with `backend = "none"` is a no-op (pre-existing). Compilation verified with and without `telemetry` feature. 0 clippy warnings.

---

### Acceptance Criteria

- [x] Sub-agent transcripts retrievable via `GET /v1/runs/:run_id/transcript`
- [x] Connection pool eliminates Mutex contention under concurrent load
- [x] Token comparison is constant-time (no timing side-channel)
- [x] Readiness probe checks real dependencies, version in health response
- [x] OTLP traces emitted for agent runs and tool calls when configured
- [x] DelegateTool generates child conversation_id and logs it for transcript discoverability
- [x] All quality gates pass: `cargo clippy --workspace`, 0 warnings

---

## Sprint 37: Production Hardening II — Security, TLS, Observability, Data Integrity

**Goal:** Close all CRITICAL and HIGH gaps that block external deployment. Harden security boundaries, wire TLS, add observability instrumentation, fix data integrity gaps, and add targeted production test coverage.

**Baseline:** Sprint 36 complete (production hardening), 1,400+ tests, 0 clippy warnings.

**Plan:** `specs/plans/08-production-hardening-ii.md`

---

### Phase A: Security Essentials (CRITICAL)

- [x] **API key scope enforcement** — Scope-checking on all `/v1/*` handler routes. API keys carry scopes (RunsRead, RunsWrite, RunsManage, Admin). Insufficient scope returns 403.
- [x] **Session TTL** — Paired tokens expire after configurable TTL (default 7 days). Legacy tokens without timestamps remain valid for backward compatibility.
- [x] **Persistent API key store** — `ApiKeyStore` backed by `EncryptedJsonStore` from `agentzero-storage`. Keys survive restarts. Encrypted at rest with XChaCha20Poly1305.
- [x] **OTP secret log level** — Moved from `println!` to `tracing::debug!` / `tracing::info!`. No longer leaks to stdout in production.
- [x] **`.unwrap()` prohibition** — Added to AGENTS.md rule 10: never use `.unwrap()` in production code, use `.expect()` with descriptive message.

### Phase B: TLS & Input Hardening (CRITICAL/HIGH)

- [x] **TLS support** — Feature-gated `tls` feature wires `axum-server` with `tls-rustls`. `[gateway.tls]` config with `cert_path` and `key_path`. Clear error when TLS configured without feature.
- [x] **HSTS middleware** — `Strict-Transport-Security: max-age=31536000; includeSubDomains` added to all responses when TLS is active.
- [x] **WebSocket message size limit** — 2 MB max frame size on both `/ws/chat` and `/ws/runs/:run_id`.
- [x] **Channel name validation** — Webhook channel names validated: alphanumeric + hyphens + underscores, 1–64 chars. Invalid names return 400.

### Phase C: Observability & Audit (HIGH)

- [x] **Per-provider metrics** — `agentzero_provider_requests_total`, `agentzero_provider_request_duration_seconds`, `agentzero_provider_errors_total`, `agentzero_provider_tokens_total`. Injected into all Anthropic and OpenAI provider methods (complete, streaming, tool-calling).
- [x] **Correlation ID propagation** — `X-Request-Id` header middleware. Propagates incoming ID or generates UUID v4. Added to tracing span and returned in response header.
- [x] **Structured audit log** — `AuditEvent` enum with 8 variants (AuthFailure, ScopeDenied, PairSuccess/Failure, ApiKeyCreated/Revoked, Estop, RateLimited). Emitted to `target: "audit"` at INFO level with consistent structured fields.
- [x] **`#[instrument]` on key paths** — Added `#[instrument]` to `execute_tool` and `respond_with_tools` in agent.rs.

### Phase D: Data Integrity (HIGH)

- [x] **Schema version table** — `_schema_version` table with versioned migration tracking. Backward-compatible with pre-versioned databases. 4 tests.
- [x] **Cost tracker migration** — `CostTracker` now uses `EncryptedJsonStore` from `agentzero-storage`. Automatic plaintext→encrypted migration. Fixes AGENTS.md rule 9.
- [x] **Per-tool execution timeout** — `tool_timeout_ms` config field (default 120s, 0 = disabled). `tokio::time::timeout` wraps `tool.execute()`. Timeout emits `tool_timeouts_total` counter. 2 tests.

### Phase E: Testing (HIGH)

- [x] **E2E security integration test** — 4 tests: full API key lifecycle (create → auth → scope enforcement → revoke → 401), admin scope access, expired key rejection, session TTL enforcement.
- [x] **Gateway load test** — 2 tests: 100 concurrent /health requests, 50 concurrent authenticated /v1/models requests. All succeed, no panics.
- [x] **WebSocket relay tests** — 2 tests: non-upgrade rejection (400 on both /ws/chat and /ws/runs), WS_MAX_MESSAGE_SIZE constant assertion (2 MB).

---

### Acceptance Criteria

- [x] API key scopes enforced on all authenticated endpoints
- [x] TLS terminates correctly when configured (feature-gated)
- [x] HSTS active when TLS is enabled
- [x] Per-provider metrics emitted for all LLM calls
- [x] Correlation IDs propagated through request lifecycle
- [x] Structured audit log captures security events
- [x] Schema migrations tracked with version table
- [x] Cost tracker uses `agentzero-storage` encrypted backend
- [x] Per-tool timeout prevents runaway tool execution
- [x] E2E security test covers full auth lifecycle
- [x] All quality gates pass: `cargo clippy`, `cargo test --workspace`, 0 warnings (2,132 tests)

---

## Sprint 38: Scaling & Operational Readiness

**Goal:** Enable multi-instance scaling (per-identity rate limiting, provider fallback chains), add operational tooling (backup/restore, OpenAPI spec), and harden production deployment (config validation, Docker resource limits).

**Baseline:** Sprint 37 complete (2,132 tests, 0 clippy warnings). All security and reliability gaps closed.

**Plan:** `specs/plans/09-scaling-and-ops.md`

---

### Phase A: Per-Identity Rate Limiting (HIGH)

- [ ] **Refactor `RateLimiter`** — Replace global `AtomicU64` with `DashMap<String, SlidingWindowCounter>` for per-identity tracking. Identity extracted from API key (`key_id`), bearer token (`"bearer"`), or unauthenticated (`"global"`).
- [ ] **Per-identity config** — `rate_limit_per_identity` field (default: 100/min). Global limit remains as fallback.
- [ ] **Rate limit headers** — `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` on all responses.
- [ ] **Bucket GC** — Periodic cleanup of expired identity buckets to prevent unbounded memory growth.
- [ ] **Tests** — Per-key isolation, key A at limit doesn't block key B, global fallback, header values.

### Phase B: Provider Fallback Chain (HIGH)

- [ ] **`FallbackProvider`** — Wrapper that chains multiple providers. On primary failure (circuit open, 5xx, timeout), tries next in chain.
- [ ] **Config** — `fallback_providers` ordered list in provider config.
- [ ] **Metrics** — `provider_fallback_total` counter with `from`/`to` labels.
- [ ] **Tests** — Primary fails → fallback succeeds, all fail → error, circuit open triggers fallback.

### Phase C: OpenAPI Spec Generation (MEDIUM)

- [ ] **`utoipa` integration** — Annotate request/response types with `ToSchema`, handlers with `#[utoipa::path]`.
- [ ] **`GET /v1/openapi.json`** — Serves auto-generated OpenAPI 3.0 spec.
- [ ] **Tests** — Endpoint returns valid JSON, schema includes key endpoints.

### Phase D: Backup/Restore CLI (MEDIUM)

- [ ] **`backup export <output-dir>`** — Exports encrypted stores (API keys, cost tracker, conversation memory) to portable tar.gz archive.
- [ ] **`backup restore <archive-path>`** — Imports archive with integrity validation. Re-encrypts on import.
- [ ] **Tests** — Round-trip export → restore preserves data, corrupt archive rejected.

### Phase E: Production Config & Docker Hardening (MEDIUM)

- [ ] **`AGENTZERO_ENV` support** — `development` / `production` environment modes.
- [ ] **Production validation** — Require TLS or explicit `allow_insecure`, require auth (no open mode), warn on localhost with public bind.
- [ ] **Docker resource limits** — `deploy.resources` in docker-compose.yml (512MB memory, 1.0 CPU).
- [ ] **Tests** — Prod validation rejects insecure config, dev mode permissive.

---

### Acceptance Criteria

- [ ] Per-identity rate limiting isolates API keys from each other
- [ ] Rate limit response headers present on all responses
- [ ] Provider fallback chain recovers from primary provider failure
- [ ] OpenAPI 3.0 spec served at `/v1/openapi.json`
- [ ] Backup export → restore round-trip preserves all data
- [ ] Production mode rejects insecure configuration
- [ ] Docker compose includes resource limits
- [ ] All quality gates pass: `cargo clippy`, `cargo test --workspace`, 0 warnings

---

## Backlog

### Distributed Event Bus

Redis-backed `EventBus` for horizontal scaling — multiple gateway/orchestrator instances share job state and events. Requires external Redis dependency.

- `EventBus` trait with `InMemoryEventBus` and `RedisEventBus` impls
- `RedisJobStore` backed by Redis hashes + pub/sub
- Config: `[orchestrator] event_bus = "memory" | "redis"` with `redis_url`

### Multi-Tenancy Deepening

Org isolation on data stores and deeper RBAC.

- Org isolation on `JobStore` queries (filter by `org_id` from API key)
- Per-org conversation memory isolation
- CLI: `auth api-key create/revoke/list`

### Additional Hardening

- Fuzzing targets (`cargo-fuzz`) for HTTP handlers and provider parsers
- Container image scanning (Trivy/Grype) in CI
- SBOM generation (CycloneDX) in release pipeline
- Operational runbooks for incident response
