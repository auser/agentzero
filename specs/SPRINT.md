# AgentZero Sprint Plan

Previous sprints archived to `specs/sprints/25-32-privacy-e2e-multi-agent-production.md`.

---

## Sprint 33: OpenClaw Multi-Agent Gaps — Queue Modes, Cascade Stop, Tool-Loop Detection

**Goal:** Close the remaining gaps between our multi-agent orchestration and the OpenClaw reference architecture. Sprint 32.5 (merged on `feat/examples-and-gaps`) delivered lanes, depth-gated tools, announce-back, async jobs API, and WebSocket run subscription. This sprint covers the behavioral and safety features that make multi-agent systems robust in production.

**Baseline:** 17-crate workspace, v0.4.2, 544+ tests in modified crates, 0 clippy warnings. Lane-based queue serialization, fan-out dispatcher, JobStore with cancel/list/events, depth-gated tool filtering, announce-back pattern, and `/ws/runs/:run_id` all implemented.

---

### Phase A: Queue Modes (steer / followup / collect / interrupt)

OpenClaw's queue system supports four message routing modes that determine how messages are delivered to agents within a lane:

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

OpenClaw implements three detectors with tiered escalation to prevent agents from getting stuck in repetitive tool-call loops:

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

Currently job events are reconstructed from `JobRecord` state. OpenClaw stores a persistent event log per run and tracks agent presence with TTL.

**Tasks:**

- [x] **`EventLog`** — Append-only log in `job_store.rs`. `EventKind`: Created, Running, ToolCall, ToolResult, Completed, Failed, Cancelled. Auto-recorded on submit and status transitions. GC cleans up event logs with expired jobs.
- [x] **Event recording** — `record_tool_call()` and `record_tool_result()` methods on `JobStore`. `GET /v1/runs/:run_id/events` returns the persistent log (replaced reconstructed approach). API test verifies tool call events appear.
- [x] **Presence tracking** — `PresenceStore` with `register`, `heartbeat`, `is_alive`, `status`, `list_all`, `deregister`, `gc_expired`. TTL-based status: Alive/Stale/Dead. 7 tests.
- [x] **Gateway endpoint** — `GET /v1/agents` returns registered agents with presence status. 2 API tests (with presence, without presence store).
- [x] **Coordinator wiring** — Agent workers register/heartbeat/deregister with `PresenceStore`. Coordinator filters dead agents from routing candidates via `is_alive()`. `Coordinator::with_presence()` builder method.

### Phase E: Block Streaming (Markdown-Aware Chunking)

OpenClaw's streaming doesn't just forward raw SSE tokens — it groups them into semantic blocks (paragraphs, code fences, lists) so subscribers receive coherent chunks.

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

**Goal:** Close the Layer 1 delegation security gaps identified in the OpenClaw gap analysis. Prevent privilege escalation, cost amplification, credential exfiltration, and resource exhaustion when delegating to sub-agents.

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

### Backlog (candidates for Sprint 36)

- [ ] **Auto-archival of sub-agent transcripts** — When a sub-agent completes, archive its full conversation to storage keyed by run_id. Parent can retrieve via `GET /v1/runs/:run_id/transcript`.
- [ ] **Worker pool pattern** — Gateway-fronted long-lived worker management with heartbeat, request queue, and backpressure
- [ ] **OpenTelemetry wiring** — Wire `[observability]` config to real OTLP exporter with span-per-run tracing
- [ ] **Distributed event bus** — Redis-backed `EventBus` for horizontal scaling (multi-node orchestration)
- [ ] **Multi-tenancy & RBAC** — User identity, API keys, org isolation
- [ ] **API polish** — OpenAPI spec generation, constant-time auth comparison, liveness/readiness probes
- [ ] **Database connection pooling** — Replace `Mutex<Connection>` with r2d2 pool + migration framework
