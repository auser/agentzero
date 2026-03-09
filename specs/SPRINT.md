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

- [ ] **QueueMode enum** — Add `QueueMode { Steer, Followup { run_id }, Collect, Interrupt }` to `agentzero-core/src/types.rs`. Wire into `TaskMessage`.
- [ ] **Followup mode** — `LaneQueue::submit()` detects `Followup` mode and appends to an existing agent's task channel instead of routing through the AI router. Agent worker maintains conversation context across followup messages.
- [ ] **Collect mode** — `FanoutDispatcher::dispatch_collect()` sends to all agents in a lane, waits for all responses (with per-agent timeout), merges results into a single `AnnounceMessage`.
- [ ] **Interrupt mode** — `LaneQueue::submit()` with `Interrupt` cancels the currently running task in the lane (via `CancellationToken`), then submits the new task. Agent worker checks cancellation between tool iterations.
- [ ] **Gateway API** — `POST /v1/runs` accepts optional `mode` field (`"steer"`, `"followup"`, `"collect"`, `"interrupt"`). Default: `"steer"`.
- [ ] **Tests** — Unit tests for each mode. Integration test: collect mode with 3 agents returning different results, verify merged output.

### Phase B: Cascade Stop

When a parent agent is cancelled, all its descendant sub-agents must also be cancelled recursively. Currently `JobStore::cancel()` only cancels a single run.

**Tasks:**

- [ ] **`cascade_cancel()`** — New method on `JobStore` that cancels a run and all runs where `parent_run_id` matches, recursively. Uses BFS to find all descendants.
- [ ] **CancellationToken propagation** — Each agent worker holds a `CancellationToken`. When cancelled, the token is dropped, which propagates to any child agents spawned with a child token.
- [ ] **Gateway wiring** — `DELETE /v1/runs/:run_id?cascade=true` calls `cascade_cancel()` instead of `cancel()`.
- [ ] **E-stop integration** — Global e-stop triggers `cascade_cancel()` on all active root-level runs.
- [ ] **Tests** — 3-level deep hierarchy (parent → child → grandchild), cancel parent, verify all three cancelled. E-stop cancels all active runs.

### Phase C: Tool-Loop Detection

OpenClaw implements three detectors with tiered escalation to prevent agents from getting stuck in repetitive tool-call loops:

1. **Exact repeat detector** — Same tool + same arguments N times in a row (default N=3). Escalation: inject a system message telling the agent to try a different approach.
2. **Semantic similarity detector** — Tool calls with >90% argument similarity over a sliding window. Escalation: reduce available tools (remove the looping tool for the next iteration).
3. **Cost runaway detector** — Token spend exceeds budget threshold within a time window. Escalation: force-complete the run with a budget-exceeded error.

**Tasks:**

- [ ] **`ToolLoopDetector` trait** — Define in `agentzero-core` with `fn check(&mut self, tool_name: &str, args: &Value) -> LoopAction`. `LoopAction` enum: `Continue`, `InjectMessage(String)`, `RestrictTools(Vec<String>)`, `ForceComplete(String)`.
- [ ] **Exact repeat detector** — Tracks last N tool calls. Configurable via `[orchestration.loop_detection] exact_repeat_threshold = 3`.
- [ ] **Similarity detector** — Levenshtein or Jaccard similarity on serialized arguments. Configurable threshold (default 0.9) and window size (default 5).
- [ ] **Cost runaway detector** — Checks `JobRecord.tokens_used` against `[orchestration.loop_detection] max_tokens_per_run` and `[orchestration.loop_detection] max_cost_microdollars_per_run`.
- [ ] **Agent worker integration** — After each tool call, run all detectors. Apply the highest-severity action.
- [ ] **Tests** — Unit tests for each detector. Integration test: agent in a tool loop gets escalated through all three tiers.

### Phase D: Persistent Event Log & Presence Tracking

Currently job events are reconstructed from `JobRecord` state. OpenClaw stores a persistent event log per run and tracks agent presence with TTL.

**Tasks:**

- [ ] **`EventLog`** — Append-only log of `(Instant, RunId, EventKind)` tuples per run. `EventKind`: `Created`, `Running`, `ToolCall { name }`, `ToolResult { name }`, `Completed { summary }`, `Failed { error }`, `Cancelled`. Stored in `JobStore` alongside `JobRecord`.
- [ ] **Event recording** — Agent worker appends events after each tool call and status transition. `GET /v1/runs/:run_id/events` returns the persistent log instead of the reconstructed one.
- [ ] **Presence tracking** — `PresenceStore` with `register(agent_id, ttl)`, `heartbeat(agent_id)`, `is_alive(agent_id)`, `gc_expired()`. Agent workers heartbeat every 30s. Coordinator uses presence to skip dead agents in routing.
- [ ] **Gateway endpoint** — `GET /v1/agents` returns list of registered agents with presence status (`alive`, `stale`, `dead`).
- [ ] **Tests** — Event log append + query. Presence TTL expiry. Dead agent skipped in routing.

### Phase E: Block Streaming (Markdown-Aware Chunking)

OpenClaw's streaming doesn't just forward raw SSE tokens — it groups them into semantic blocks (paragraphs, code fences, lists) so subscribers receive coherent chunks.

**Tasks:**

- [ ] **`BlockAccumulator`** — Stateful accumulator that buffers streaming tokens and emits complete blocks. Recognizes: paragraph breaks (double newline), code fences (``` open/close), list items, headers.
- [ ] **WebSocket integration** — `/ws/runs/:run_id` uses `BlockAccumulator` to send block-level frames instead of raw token frames when `?format=blocks` query param is set.
- [ ] **SSE endpoint** — `GET /v1/runs/:run_id/stream` as an alternative to WebSocket for environments that don't support WS. Uses `text/event-stream` with block-level events.
- [ ] **Tests** — Accumulator correctly groups paragraphs, code blocks, interleaved content. SSE endpoint streams expected events.

---

### Acceptance Criteria

- [ ] All four queue modes work: steer routes to one agent, followup appends to existing run, collect merges all responses, interrupt cancels and replaces
- [ ] Cascade cancel propagates through 3+ levels of sub-agent hierarchy
- [ ] Tool-loop detection escalates through inject → restrict → force-complete
- [ ] Persistent event log records tool calls and status transitions; queryable via API
- [ ] Presence tracking with TTL; dead agents skipped in routing
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
