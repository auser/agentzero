# Runtime Enhancements Plan — Self-Improving Agent + Infrastructure Hardening

## Context

After researching state-of-the-art agent frameworks and auditing our own codebase, we identified features that would make AgentZero genuinely self-improving and production-ready. The audit revealed:

- **Tool auto-generation already exists** (5 strategies including WASM codegen, ToolEvolver auto-fix/improve, RecipeStore pattern learning) — covered by [specs/plans/38-tool-fn-macro-codegen.md](specs/plans/38-tool-fn-macro-codegen.md). No duplication needed.
- **A2A protocol is partial** — sync-only, no streaming, no discovery, no multi-turn
- **Event bus is audit-only** — lossy broadcast, not suitable for inter-agent messaging
- **Voice/STT is 74% complete** — STT works via markers, TTS works as tool, but no real-time audio capture (cpal missing), wake word stubbed

This plan covers what's **missing** across 8 phases.

---

## Phase 1: Self-Improving Learning Loop (HIGH PRIORITY)

**Why**: AgentZero has tool-level learning (ToolEvolver, RecipeStore, PatternCapture) but no **session-level** trajectory recording or aggregate insights. The agent can't learn from its own history.

**Note**: Tool auto-generation/evolution is already handled by spec 38. This phase adds the higher-level learning that feeds into those systems.

### 1A. Trajectory Recorder
- **New file**: `crates/agentzero-infra/src/trajectory.rs`
- Record full session: `TrajectoryRecord { session_id, run_id, outcome, goal_summary, messages, tool_executions, token_usage, cost_microdollars, model, latency_ms, tags }`
- Outcome classification from existing `StopReason` + loop detection
- Append-only JSONL: `trajectories/successful.jsonl`, `trajectories/failed.jsonl`
- Follow `FileAuditSink` pattern (spawn_blocking for I/O, per-line encryption)
- **Hook point**: Add `trajectory_recorder: Option<Arc<TrajectoryRecorder>>` to `RuntimeExecution` (after line 97), call post-run alongside existing `pattern_capture` and `recipe_store`

### 1B. Insights Engine
- **New files**: `crates/agentzero-infra/src/insights.rs` + `crates/agentzero-tools/src/insights.rs`
- Lazy JSONL scanning (not in-memory) for: token efficiency per goal type, tool usage heatmap (success vs failure), cost trends, failure clustering (3-tool sliding window before errors), model effectiveness scores
- Exposed as `insights_report` tool so the agent can query its own performance history
- Cross-references existing `CostTracker` data

### 1C. Session Summarization to Persistent Memory
- **Modify**: `crates/agentzero-tools/src/memory_tools.rs`
- At session end: feed conversation to cheap model with structured prompt for learnings/preferences/mistakes
- Defense: run memory updates through existing `PromptInjectionGuard` + new Unicode invisible-character detection (zero-width spaces, RTL overrides, homoglyphs) in `crates/agentzero-providers/src/guardrails.rs`

### 1D. Proactive Tool Gap Detection (extends spec 38)
- **Where**: `crates/agentzero-infra/src/tool_recipes.rs`
- When RecipeStore sees repeated failures for a goal pattern with no matching tools, trigger `tool_create` proactively
- Currently reactive only (fix on failure); add predictive detection: "I noticed we kept failing at X, let me create a tool for that"

### 1E. Richer Evolution Context (extends spec 38)
- **Where**: `crates/agentzero-infra/src/tool_evolver.rs`
- Currently ToolEvolver only gets `last_error` string. Enrich with: partial outputs, timing data, resource utilization, stack of recent errors (not just last)
- Enable multi-strategy pivoting: when Shell keeps failing, suggest "try HTTP instead"
- Enable codegen auto-improvement (currently disabled, generation cap=0)

---

## Phase 2: Advanced Context Compression (HIGH ROI)

**Why**: Every LLM call pays for context tokens. Current `SummarizationConfig` is basic (keep N recent, summarize rest). A 4-phase approach can cut costs ~75%.

- **New file**: `crates/agentzero-core/src/context_compression.rs`
- **Modify**: `SummarizationConfig` in `crates/agentzero-core/src/types.rs`, integrate in `agent.rs` at `build_provider_prompt`
- **4 phases**:
  1. **Tool result pruning** — truncate `ToolResult` beyond configurable char limit (pure fn, no LLM)
  2. **Boundary protection** — first N + last M messages immutable; never split `ToolUseRequest`/`ToolResult` pairs
  3. **Middle turn summarization** — cheap model with Goal/Progress/Decisions/NextSteps structure
  4. **Iterative updates** — on subsequent compressions, merge into existing summary (track `summary_version`)
- Phases 1-2 are zero-allocation transforms on `&mut Vec<ConversationMessage>`. Phase 3 requires one async LLM call.

---

## Phase 3: A2A Protocol Hardening (CRITICAL for multi-agent)

**Current state**: Sync-only task execution, no streaming, no discovery, `InputRequired` state exists but never set, 120s hard timeout, text-only payloads.

**Files to modify**:
- `crates/agentzero-core/src/a2a_types.rs` (343 lines, 15 tests)
- `crates/agentzero-gateway/src/a2a.rs` (421 lines, 15 tests)
- `crates/agentzero-orchestrator/src/a2a_client.rs` (277 lines, 6 tests)
- `crates/agentzero-tools/src/a2a.rs` (419 lines, 9 tests)

### 3A. Streaming Results via SSE
- `tasks/sendSubscribe` endpoint returning `text/event-stream`
- Events: `TaskStatusUpdateEvent`, `TaskArtifactUpdateEvent` per A2A spec
- Replace 120s sync block with async task ID + streaming
- Set `streaming: true` in AgentCard capabilities

### 3B. Multi-Turn with InputRequired
- Wire up `InputRequired` state (exists in enum, never set)
- Agent can pause mid-task and request clarification from caller
- `tasks/send` with existing task_id resumes the conversation

### 3C. Agent Discovery
- Service registry endpoint: `GET /a2a/agents` listing available agents
- mDNS/DNS-SD for local network discovery (optional feature gate)
- AgentCard caching with TTL

### 3D. Rich Payload Handling
- Support `DataPart` and `FilePart` (currently only `TextPart` extracted)
- Binary artifact streaming for files/images

### 3E. Integration Tests
- Multi-agent chain: agent A → agent B → result back
- Streaming end-to-end test
- Failure scenarios: timeout, network error, malformed payload

---

## Phase 4: Event Bus for Inter-Agent Communication

**Current state**: `tokio::sync::broadcast` — lossy, transient, no queue semantics. `agents_ipc` tool falls back to file storage because bus isn't reliable. Good for audit/logging, not for messaging.

**File**: `crates/agentzero-core/src/event_bus.rs` (773 lines)

### 4A. Durable Message Queue per Agent
- Add `MessageQueue` abstraction alongside existing broadcast
- Per-agent topic queues backed by SQLite (extend `SqliteEventBus` in `crates/agentzero-storage/src/event_bus.rs`)
- Messages persist until ACK'd — survives subscriber offline periods

### 4B. Delivery Guarantees
- At-least-once delivery with ACK/NACK
- Retry with exponential backoff for unACK'd messages
- Dead letter queue for messages that exceed retry count

### 4C. Request/Reply Pattern
- Built-in correlation: `publish_and_wait(topic, payload, timeout)` → response
- Currently only `correlation_id` exists with no matching mechanism

### 4D. Backpressure & Flow Control
- Per-agent rate limiting
- Publisher blocking when subscriber queue full (configurable: block vs drop)
- Buffer metrics exposed via `RuntimeMetrics`

### 4E. Remove agents_ipc File Fallback
- Once durable queues work, remove the file-based fallback in `agents_ipc` tool
- Single communication path through event bus

---

## Phase 5: Voice/STT Pipeline Completion

**Current state**: STT via `[AUDIO:path]` markers works (16 tests). TTS via tool works (3 tests). No real-time audio capture — `cpal` missing, wake word logic stubbed.

### 5A. Real-Time Audio Capture
- Add `cpal` + `hound` dependencies to `crates/agentzero-channels/Cargo.toml` (feature-gated: `channel-voice-wake`)
- Implement the stubbed `listen()` in [voice_wake.rs:93-106](crates/agentzero-channels/src/channels/voice_wake.rs#L93-L106):
  - Open default audio input device via cpal
  - Energy-based VAD using existing `compute_energy()` (already tested)
  - Buffer captured samples, encode as WAV via hound
  - POST to transcription endpoint
  - Check transcript via existing `matches_wake_word()` (already tested)

### 5B. Voice Wake Config
- Add `[channels.voice_wake]` to config model (`crates/agentzero-config/src/model.rs`):
  - `wake_words`, `energy_threshold`, `capture_timeout_secs`, `transcription_url`

### 5C. Automatic Voice Response
- Hook TTS into agent response pipeline in `runtime.rs` (~line 875)
- When source channel is `voice_wake`, auto-call TTS on response text
- Add audio playback abstraction (platform-specific: `rodio` on desktop, WebSocket streaming for remote)

### 5D. Streaming Audio I/O (future)
- WebSocket audio streaming for real-time interaction
- Streaming STT (not batch transcription)
- Streaming TTS (chunked audio generation)

---

## Phase 6: Smart Cost-Aware Model Routing

**Why**: Direct cost savings on every request.

- **New file**: `crates/agentzero-core/src/complexity.rs`
- **Modify**: `crates/agentzero-core/src/routing.rs` — add complexity scoring to `ModelRouter::classify`
- Complexity scorer: char count, word count, code presence (markdown blocks, imports), keyword signals, tool hints → `ComplexityTier` (Simple/Medium/Complex)
- Route Simple to cheapest model, Complex to premium
- Conservative: default to premium on uncertainty (composite score 0.4-0.6)
- Builds on existing `ClassificationRule` + `ModelPricing` in `crates/agentzero-providers/src/pricing.rs`

---

## Phase 7: Credential Pooling

**Why**: Avoid rate limits with multiple API keys per provider.

- **New file**: `crates/agentzero-providers/src/credential_pool.rs`
- **Modify**: `crates/agentzero-config/src/model.rs` — extend `ProviderConfig`
- Strategies: FillFirst, RoundRobin, LeastUsed, Random
- Exhaustion tracking: reuse `CooldownState` from `FallbackProvider` — 1h on 429, 24h on other errors
- OAuth token auto-refresh across pooled credentials
- Config: `[provider.credential_pool] strategy = "round-robin" keys = ["KEY_1", "KEY_2"]`

---

## Phase 8: Infrastructure Hardening

### 8A. Pre-Execution Cost Estimation
- **Where**: `crates/agentzero-providers/src/pipeline.rs` — new `CostEstimateLayer`
- Estimate token count before LLM call using tiktoken-compatible tokenizer
- Warn or block if estimated cost exceeds threshold
- Currently `CostCapLayer` only tracks post-execution

### 8B. Context File Injection Scanning
- **Where**: `crates/agentzero-providers/src/guardrails.rs`
- Scan `.agentzero.md`, loaded context files, and project files before inclusion in system prompt
- Currently `PromptInjectionGuard` only scans user input messages
- Add invisible Unicode detection (zero-width spaces, RTL overrides, homoglyphs)

### 8C. Adaptive Thinking Effort
- **Where**: `crates/agentzero-core/src/types.rs` — extend `ReasoningConfig`
- Currently static: `enabled: bool` + `level: String`
- Add dynamic adjustment: tie to complexity scorer from Phase 6
- Simple queries → low/no reasoning budget; complex → deep reasoning
- Provider-specific mapping (Claude: thinking tokens, OpenAI: reasoning effort)

### 8D. Tool Execution Middleware
- **New file**: `crates/agentzero-core/src/tool_middleware.rs`
- Composable pre/post interceptors on tool calls (like provider `LlmLayer` pipeline)
- Enables: checkpointing (snapshot before file mutations), audit logging, rate limiting, timing — all as pluggable layers
- Replaces ad-hoc hooks with a consistent pattern

### 8E. Checkpoint/File Recovery
- **New file**: `crates/agentzero-tools/src/checkpoint.rs`
- Implemented as a tool middleware (8D): snapshot files before `write_file`/`apply_patch`
- Plain file copies in `.agentzero/checkpoints/<session>/<timestamp>/`
- Tools: `checkpoint_list`, `checkpoint_restore`, `checkpoint_diff`
- Pre-rollback snapshot for undo-the-undo

### 8F. Prompt Caching Layer
- **New file**: `crates/agentzero-providers/src/prompt_cache.rs`
- New `LlmLayer` that annotates system prompt + last N messages with Anthropic `cache_control`
- ~75% input token cost reduction
- Provider-specific, feature-gated (Anthropic only initially)

---

## Implementation Dependencies

```
Phase 1A (Trajectory)        ─── independent, start first
Phase 1B (Insights)          ─── depends on 1A
Phase 1C (Session Memory)    ─── independent
Phase 1D-1E (Spec 38 ext)   ─── independent
Phase 2  (Compression)       ─── independent
Phase 3  (A2A)               ─── independent
Phase 4  (Event Bus)         ─── independent
Phase 5  (Voice/STT)         ─── independent
Phase 6  (Cost Routing)      ─── independent
Phase 7  (Credential Pool)   ─── independent
Phase 8A (Cost Estimate)     ─── independent
Phase 8B (Injection Scan)    ─── independent
Phase 8C (Adaptive Thinking) ─── depends on Phase 6 (complexity scorer)
Phase 8D (Tool Middleware)   ─── independent
Phase 8E (Checkpoints)       ─── depends on 8D
Phase 8F (Prompt Cache)      ─── independent
```

Most phases can be developed concurrently — they touch different crates with minimal overlap.

---

## Verification

| Phase | Test |
|-------|------|
| 1A | Run agent, check `trajectories/*.jsonl` populated with correct outcome labels |
| 1B | Run `insights_report` tool, verify stats match trajectory data |
| 1C | End session, verify memory updated with learnings |
| 1D | Fail at same goal 3x with no matching tool, verify `tool_create` triggered |
| 1E | Fail a Shell tool 5x, verify evolver suggests strategy pivot |
| 2 | Long conversation → verify tool results truncated, middle turns summarized, token count drops |
| 3 | Agent A sends task to Agent B via A2A, receives streaming results |
| 4 | Agent publishes while subscriber offline → subscriber connects later → receives message |
| 5 | Say wake word → audio captured → transcribed → agent responds with TTS |
| 6 | "what time is it?" routes to Haiku; complex code gen routes to Opus |
| 7 | Exhaust one key with 429s, verify automatic rotation |
| 8A | Estimate before expensive call, verify warning when over threshold |
| 8B | Include context file with injection attempt, verify it's caught |
| 8C | Simple query gets low reasoning; complex gets deep |
| 8D-8E | Write a file, verify checkpoint exists, restore it |
| 8F | Verify cache_control annotations on Anthropic calls |
| All | `cargo clippy --all-targets` zero warnings, `cargo test` passes |
