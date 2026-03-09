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
- [ ] **Followup mode** — `LaneQueue::submit()` detects `Followup` mode and appends to an existing agent's task channel instead of routing through the AI router. Agent worker maintains conversation context across followup messages.
- [ ] **Collect mode** — `FanoutDispatcher::dispatch_collect()` sends to all agents in a lane, waits for all responses (with per-agent timeout), merges results into a single `AnnounceMessage`.
- [ ] **Interrupt mode** — `LaneQueue::submit()` with `Interrupt` cancels the currently running task in the lane (via `CancellationToken`), then submits the new task. Agent worker checks cancellation between tool iterations.
- [x] **Gateway API** — `POST /v1/runs` accepts optional `mode` field. `DELETE /v1/runs/:run_id` accepts optional `cascade` query param.
- [ ] **Tests** — Integration test: collect mode with 3 agents returning different results, verify merged output.

### Phase B: Cascade Stop

When a parent agent is cancelled, all its descendant sub-agents must also be cancelled recursively. Currently `JobStore::cancel()` only cancels a single run.

**Tasks:**

- [x] **`cascade_cancel()`** — BFS traversal cancels a run and all descendants where `parent_run_id` matches. Skips already-terminal jobs. 3 tests: parent+children, 3-level deep, skips-terminal.
- [ ] **CancellationToken propagation** — Each agent worker holds a `CancellationToken`. When cancelled, the token is dropped, which propagates to any child agents spawned with a child token.
- [x] **Gateway wiring** — `DELETE /v1/runs/:run_id?cascade=true` calls `cascade_cancel()`. Returns `cascade_count` and `cancelled_ids`. API test.
- [ ] **E-stop integration** — Global e-stop triggers `cascade_cancel()` on all active root-level runs.
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
- [ ] **Agent worker integration** — After each tool call, run all detectors. Apply the highest-severity action.
- [x] **Tests** — 10 unit tests: exact repeat (trigger, reset, different-args), similarity (trigger, no-trigger), cost (token-limit, cost-limit, disabled), severity ordering, Jaccard similarity edge cases.

### Phase D: Persistent Event Log & Presence Tracking

Currently job events are reconstructed from `JobRecord` state. OpenClaw stores a persistent event log per run and tracks agent presence with TTL.

**Tasks:**

- [x] **`EventLog`** — Append-only log in `job_store.rs`. `EventKind`: Created, Running, ToolCall, ToolResult, Completed, Failed, Cancelled. Auto-recorded on submit and status transitions. GC cleans up event logs with expired jobs.
- [x] **Event recording** — `record_tool_call()` and `record_tool_result()` methods on `JobStore`. `GET /v1/runs/:run_id/events` returns the persistent log (replaced reconstructed approach). API test verifies tool call events appear.
- [x] **Presence tracking** — `PresenceStore` with `register`, `heartbeat`, `is_alive`, `status`, `list_all`, `deregister`, `gc_expired`. TTL-based status: Alive/Stale/Dead. 7 tests.
- [x] **Gateway endpoint** — `GET /v1/agents` returns registered agents with presence status. 2 API tests (with presence, without presence store).
- [ ] **Coordinator wiring** — Agent workers heartbeat into PresenceStore. Coordinator skips dead agents in routing.

### Phase E: Block Streaming (Markdown-Aware Chunking)

OpenClaw's streaming doesn't just forward raw SSE tokens — it groups them into semantic blocks (paragraphs, code fences, lists) so subscribers receive coherent chunks.

**Tasks:**

- [x] **`BlockAccumulator`** — Stateful accumulator in `agentzero-orchestrator::block_stream`. Recognizes: paragraphs (double newline), code fences with language, headers (# through ######), list items (-, *, numbered). Handles incremental token feeding and unclosed code blocks on flush.
- [ ] **WebSocket integration** — `/ws/runs/:run_id` uses `BlockAccumulator` to send block-level frames instead of raw token frames when `?format=blocks` query param is set.
- [ ] **SSE endpoint** — `GET /v1/runs/:run_id/stream` as an alternative to WebSocket for environments that don't support WS. Uses `text/event-stream` with block-level events.
- [x] **Tests** — 11 tests: paragraph detection, code block (complete + incremental + unclosed), headers, list items (bullet + numbered), mixed content, flush behavior, empty input.

---

### Acceptance Criteria

- [ ] All four queue modes work: steer routes to one agent, followup appends to existing run, collect merges all responses, interrupt cancels and replaces
- [x] Cascade cancel propagates through 3+ levels of sub-agent hierarchy
- [x] Tool-loop detection escalates through inject → restrict → force-complete
- [x] Persistent event log records tool calls and status transitions; queryable via API
- [x] Presence tracking with TTL; dead agents queryable via `/v1/agents` API
- [ ] Block streaming delivers semantic chunks over WebSocket and SSE
- [ ] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test --workspace`

---

### Backlog (candidates for Sprint 34)

- [ ] **Auto-archival of sub-agent transcripts** — When a sub-agent completes, archive its full conversation to storage keyed by run_id. Parent can retrieve via `GET /v1/runs/:run_id/transcript`.
- [ ] **OpenTelemetry wiring** — Wire `[observability]` config to real OTLP exporter with span-per-run tracing
- [ ] **Distributed event bus** — Redis-backed `EventBus` for horizontal scaling (multi-node orchestration)
- [ ] **Multi-tenancy & RBAC** — User identity, API keys, org isolation
- [ ] **API polish** — OpenAPI spec generation, constant-time auth comparison, liveness/readiness probes
- [ ] **Database connection pooling** — Replace `Mutex<Connection>` with r2d2 pool + migration framework
