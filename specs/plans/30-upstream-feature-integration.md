# Plan 30: Upstream Feature Integration

**Status:** Planned
**Source:** Upstream open PRs (surveyed 2026-03-21)
**Branch:** `feat/upstream-integrations` (umbrella; per-phase branches below)
**Sprints:** 62 (Phase 1), 63 (Phase 2), 64 (Phase 3), 65 (Phase 4), 66 (Phase 5A+B), 67 (Phase 5C+D)
**Sprint tracking:** `specs/SPRINT.md`

---

## Goal

Integrate 13 high-value features from upstream open PRs into AgentZero, adapted to our crate architecture and security model. Organized into 6 phases by effort/value/dependency order.

---

## Phase 1: Quick Wins (Small effort, high/medium value)

### 1A. Codex/Gemini CLI Harness Tools (upstream #4154)

**What:** Three new tools (`CodexCliTool`, `GeminiCliTool`, `OpenCodeCliTool`) that shell out to external CLI agent binaries (`codex -q`, `gemini -p`, `opencode`) with rate limiting, env sanitization, timeout/kill-on-drop, and output truncation.

**Why:** Multi-harness delegation with near-zero effort. Each tool is ~344 lines following our existing shell/process patterns exactly.

**New files:**
- `crates/agentzero-tools/src/codex_cli.rs` — `CodexCliTool` impl
- `crates/agentzero-tools/src/gemini_cli.rs` — `GeminiCliTool` impl
- `crates/agentzero-tools/src/opencode_cli.rs` — `OpenCodeCliTool` impl

**Modified files:**
- `crates/agentzero-tools/src/lib.rs` — add modules, add to `tool_tier()` as Full tier
- `crates/agentzero-infra/src/tools/mod.rs` — register in `default_tools_inner()`, gated by policy
- `crates/agentzero-config/src/model.rs` — add `[tools.cli_harness]` config section (enabled per-binary, disabled by default)

**Security:**
- Each tool gates behind `ToolOperation::Act` (already exists)
- Env sanitization: strip `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, etc. before spawning
- Kill-on-drop via `tokio::process::Command` with `kill_on_drop(true)`
- Output truncation at configurable max bytes (default 64KB)
- Timeout enforcement (default 300s)

**Tests:** Unit tests for tool metadata, env sanitization, timeout enforcement, output truncation.

---

### 1B. Provider 429 Cooldown + Model Compatibility Filtering (upstream #4136)

**What:** Two improvements to provider resilience:
1. **429-specific cooldown** — when a provider returns HTTP 429, immediately skip it for the `Retry-After` duration (or 10s default) instead of waiting for 5 failures to trip the circuit breaker.
2. **Model compatibility filtering** — wire our existing `find_models_for_provider()` catalog into `FallbackProvider` selection so incompatible provider-model pairs are skipped without a network request.

**Why:** Our circuit breaker treats all failures equally — a predictable 429 needs 5 failures before tripping. The cooldown handles temporary rate limits while the circuit breaker handles persistent failures. Model filtering prevents wasted requests and noisy error logs.

**Modified files:**
- `crates/agentzero-providers/src/transport.rs` — add `CooldownState` struct alongside `CircuitBreaker`: `Option<Instant>` for cooldown expiry, activated on single 429, uses `parse_retry_after()` (already exists)
- `crates/agentzero-providers/src/fallback.rs` — check `CooldownState` before attempting provider; check `find_models_for_provider()` for model compatibility before attempting
- `crates/agentzero-providers/src/models.rs` — ensure `find_models_for_provider()` is pub and covers all provider types

**Tests:** Unit tests for cooldown activation/expiry, model filtering skip, Retry-After parsing integration.

---

### 1C. A2A Tool Interface + Spec Alignment (upstream #4166)

**What:** Add an `A2aTool` that wraps our existing `A2aAgentEndpoint` so the LLM can dynamically discover and call external A2A agents as a tool action. Also update our A2A types to match the latest spec.

**Why:** Currently agents can only talk to pre-configured swarm members. A tool-based interface lets agents discover and call arbitrary A2A agents at runtime.

**New files:**
- `crates/agentzero-tools/src/a2a.rs` — `A2aTool` with actions: `discover` (fetch agent card), `send` (send message, get task), `status` (poll task), `cancel` (cancel task)

**Modified files:**
- `crates/agentzero-core/src/a2a_types.rs` — update method names `tasks/send` → `message/send`, Part discriminator `"type"` → `"kind"` per latest spec
- `crates/agentzero-gateway/src/a2a.rs` — update JSON-RPC method routing to match new spec names; fix empty `url` field on Agent Card (line 69)
- `crates/agentzero-orchestrator/src/a2a_client.rs` — add `check_status()` and `cancel_task()` methods; add URL scheme validation (reject non-HTTP(S))
- `crates/agentzero-gateway/src/a2a.rs` — replace `std::sync::Mutex` with `tokio::sync::Mutex` on `A2aTaskStore`
- `crates/agentzero-tools/src/lib.rs` — add module, add `enable_a2a_tool: bool` to `ToolSecurityPolicy`
- `crates/agentzero-infra/src/tools/mod.rs` — register `A2aTool`
- `crates/agentzero-config/src/model.rs` — add `bearer_token` to `A2aConfig` for inbound auth

**Tests:** Tool action tests, URL validation, spec-aligned wire format tests.

---

### 1D. Provider Streaming Wiring (upstream #4175)

**What:** Connect our existing `StreamSink` receiver to `DraftTracker` so channel messages update token-by-token. Extract duplicated `ToolCallAccum` into a shared type. Add `supports_streaming()` to provider trait.

**Why:** We have all the primitives but they're not connected. Users on channels see nothing until the full response arrives.

**Modified files:**
- `crates/agentzero-core/src/types.rs` — add `supports_streaming() -> bool` to `Provider` trait (default `false`); extract `StreamToolCallAccumulator` struct from inline patterns
- `crates/agentzero-providers/src/anthropic.rs` — use shared `StreamToolCallAccumulator`, impl `supports_streaming() -> true`
- `crates/agentzero-providers/src/openai.rs` — use shared `StreamToolCallAccumulator`, impl `supports_streaming() -> true`
- `crates/agentzero-providers/src/fallback.rs` — delegate `complete_streaming()` and `complete_streaming_with_tools()` to inner provider
- `crates/agentzero-infra/src/runtime.rs` (or channel handler path) — spawn consumer task: `mpsc::unbounded_channel()` → `respond_streaming()` with sender, consumer reads receiver and calls `draft_tracker.update()` on each chunk, `draft_tracker.finalize()` on done

**Tests:** Integration test with mock streaming provider verifying draft updates arrive token-by-token.

---

### 1E. Per-Sender Rate Limiting (upstream #4138)

**What:** Propagate channel sender identity (Telegram user ID, Discord channel, etc.) into a task-local, then use it for per-sender rate-limit bucketing in the agent loop.

**Why:** Currently a single noisy sender on a shared channel can exhaust the global rate limit for all users.

**Modified files:**
- `crates/agentzero-core/src/types.rs` — add `sender_id: Option<String>` to `ToolContext`
- `crates/agentzero-channels/src/lib.rs` — populate `sender_id` from channel-specific identifiers when creating `ToolContext`
- `crates/agentzero-infra/src/runtime.rs` — create per-sender sliding window counter (reuse gateway `WindowCounter` pattern from `middleware.rs`), check before tool execution
- `crates/agentzero-config/src/model.rs` — add `max_actions_per_sender_per_hour: Option<u32>` to autonomy config

**Tests:** Unit tests for per-sender bucketing, fallback to global limit when no sender_id.

---

### 1F. Fallback Notification (upstream #4135)

**What:** When cross-provider fallback occurs, surface it to users via channel message footer and API response headers.

**Why:** Users don't know their response came from a different provider, which matters for transparency (different capabilities, pricing, behavior).

**Modified files:**
- `crates/agentzero-providers/src/fallback.rs` — add `task_local! { static FALLBACK_INFO: RefCell<Option<FallbackInfo>> }` with `FallbackInfo { original_provider, actual_provider, actual_model }`; set on cross-provider fallback
- `crates/agentzero-channels/src/lib.rs` — after response, check `FALLBACK_INFO`; if set, append footer: "Response from {actual_provider} ({actual_model}) — primary provider unavailable"
- `crates/agentzero-gateway/src/handlers.rs` — on API responses, add `X-Provider-Fallback: true` + `X-Provider-Used: {actual}` headers when fallback occurred

**Tests:** Unit tests for task-local lifecycle, footer formatting, header emission.

---

## Phase 2: A2UI / Live Canvas (Medium effort, high value)

### 2A. CanvasTool + CanvasStore (upstream #4163)

**What:** A new tool that lets agents push rich visual content (HTML, SVG, Markdown) to a web-visible canvas in real time. REST + WebSocket endpoints. Sandboxed iframe viewer.

**Why:** Agents can only output plain text today. Canvas enables dashboards, charts, rendered documents, interactive previews.

**New files:**
- `crates/agentzero-core/src/canvas.rs` — `CanvasStore` struct (lives in core to avoid gateway→tools dependency):
  - `Arc<RwLock<HashMap<String, Canvas>>>` keyed by canvas ID
  - Integration with `EventBus` for real-time push (not raw broadcast channels)
  - Methods: `list()`, `snapshot()`, `history()`, `render()`, `clear()`
  - Canvas scoped to run ID from `/v1/runs`
  - 256KB max content size per frame, max 100 history frames
  - Content-type allowlist: `text/html`, `image/svg+xml`, `text/markdown`, `text/plain`
- `crates/agentzero-tools/src/canvas.rs` — `CanvasTool` with actions:
  - `render` — push content to named canvas (content_type + content body)
  - `snapshot` — read current canvas state
  - `clear` — remove canvas content
  - (Skip `eval` — security risk, not needed for core use case)
- `crates/agentzero-gateway/src/canvas.rs` — HTTP + WebSocket handlers:
  - `GET /api/canvas` — list active canvases
  - `GET /api/canvas/:id` — get current content
  - `POST /api/canvas/:id` — create/update canvas
  - `DELETE /api/canvas/:id` — clear canvas
  - `GET /api/canvas/:id/history` — frame history
  - `WS /ws/canvas/:id` — real-time frame delivery (follow existing `ws_chat`/`ws_run_subscribe` auth pattern)
- `ui/src/pages/Canvas.tsx` — React viewer:
  - WebSocket connection with reconnect
  - Sandboxed iframe for content rendering
  - Canvas switcher (multiple canvases)
  - Frame history panel

**Modified files:**
- `crates/agentzero-core/src/lib.rs` — add `pub mod canvas;`
- `crates/agentzero-tools/src/lib.rs` — add module, `enable_canvas: bool` on `ToolSecurityPolicy`, tool tier = Extended
- `crates/agentzero-infra/src/tools/mod.rs` — register `CanvasTool`, gated by policy
- `crates/agentzero-gateway/src/state.rs` — add `canvas_store: Option<Arc<CanvasStore>>` to `GatewayState`
- `crates/agentzero-gateway/src/router.rs` — add 5 REST routes + 1 WS route
- `crates/agentzero-gateway/src/lib.rs` — add `mod canvas;`, instantiate store, wire into state
- `crates/agentzero-config/src/model.rs` — add `[tools.canvas]` section: `enabled`, `max_content_bytes`, `max_history_frames`
- `ui/src/App.tsx` — add `/canvas` route
- UI sidebar — add Canvas navigation entry

**Security (non-negotiable):**
- iframe `sandbox` attribute WITHOUT `allow-same-origin` (prevents content accessing parent frame)
- CSP headers on canvas content endpoint: `default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'`
- Server-side HTML sanitization before storage
- WebSocket auth via `authorize_with_scope()` with `Scope::CanvasRead`
- Rate limiting on `render` action (prevent flooding)

**Feature gate:** `canvas` feature flag on `agentzero-gateway`. Excluded from embedded builds.

**Optional enhancement:** SQLite persistence for canvas frames via `agentzero-storage` (config toggle). Enables post-restart review.

**Tests:** Tool action tests, store CRUD, WebSocket auth, content-type validation, size limit enforcement, history truncation.

---

## Phase 3: Parallel & Background Delegation (Medium-large effort, high value)

### 3A. Background + Parallel Delegate Modes (upstream #4159)

**What:** Extend `DelegateTool` with three new modes (all opt-in, sync default preserved):
1. **Background** — `tokio::spawn` sub-agent, return `task_id` immediately
2. **Parallel** — array of agents, concurrent execution, blocking aggregate return
3. **Task lifecycle** — `check_result`, `list_results`, `cancel_task` actions

**Why:** Parent agents currently block on sub-agent completion. Background mode enables fire-and-forget research tasks, parallel mode enables fan-out patterns.

**New files:**
- `crates/agentzero-tools/src/task_manager.rs` — `TaskManager` struct:
  - `HashMap<String, BackgroundTask>` where `BackgroundTask = { join_handle, cancellation_token, status, result }`
  - `spawn_background()` — create child `CancellationToken`, spawn task, return task_id
  - `check_result(task_id)` — return status + result if complete
  - `list_results()` — enumerate all tasks with statuses
  - `cancel_task(task_id)` — trigger child token cancellation
  - `cancel_all()` — cascade cancel on session teardown
  - Result persistence to `{workspace}/delegate_results/{task_id}.json`

**Modified files:**
- `crates/agentzero-core/src/types.rs`:
  - Add `cancellation_token: Option<CancellationToken>` to `ToolContext` (coexist with `AtomicBool` for backward compat; migration path: new code uses token, old code continues with bool)
  - Add `task_id: Option<String>` to `ToolContext`
- `crates/agentzero-tools/src/delegate.rs`:
  - Add `action` field to `Input`: enum `Delegate` (default) | `CheckResult` | `ListResults` | `CancelTask`
  - Add `background: Option<bool>` to `Input`
  - Add `agents: Option<Vec<String>>` to `Input` (parallel mode)
  - Accept `Arc<TaskManager>` in constructor
  - Background path: spawn via `task_manager.spawn_background()`, return task_id
  - Parallel path: `tokio::JoinSet` over agents, respect existing `Semaphore` (max 4 concurrent)
  - `check_result`/`list_results`/`cancel_task` delegate to `TaskManager`
- `crates/agentzero-infra/src/tools/mod.rs` — construct `TaskManager`, pass to `DelegateTool`
- `crates/agentzero-infra/src/runtime.rs` — wire `TaskManager` per-session; add teardown hook calling `task_manager.cancel_all()`
- `crates/agentzero-tools/src/delegate_coordination_status.rs` — deprecate in favor of `TaskManager` (keep for backward compat, wire to read from `TaskManager`)

**Concurrency concerns:**
- Budget tracking: pre-allocate token/cost budget to background tasks at spawn time; accept eventual consistency on aggregation
- Security inheritance: background tasks inherit `AutonomyPolicy` intersection via `ctx.clone()`; `OutputScanner` (leak guard) must be explicitly forwarded
- Orphan prevention: session teardown cascades via `CancellationToken::cancel()`; recursive (child spawning sub-children inherits tokens)
- Depth control: existing `max_depth` + `validate_delegation()` applies to background tasks identically

**Dependency:** `tokio-util` crate for `CancellationToken` (add to `Cargo.toml` if not already present).

**Tests:** Background spawn + check_result, parallel fan-out, cancel_task, session teardown cascade, budget pre-allocation, depth limit enforcement.

---

## Phase 4: Deterministic SOP Engine (Large effort, high value)

### 4A. SOP Engine Rewrite (upstream #4160)

**What:** Replace our flat JSON SOP store with a proper engine supporting deterministic execution mode (bypass LLM for step transitions), typed steps with I/O schemas, approval checkpoints with timeout, state persistence + resume, and cost tracking.

**Why:** A 10-step deterministic SOP saves 10-50 seconds wall-clock + $0.10-0.50 per execution by eliminating LLM round-trips for step transitions. Critical for cron-scheduled and CI/CD SOPs.

**New files:**
- `crates/agentzero-tools/src/sop/mod.rs` — module root
- `crates/agentzero-tools/src/sop/types.rs` — type definitions:
  - `SopExecutionMode` enum: `Supervised` (LLM-mediated, default) | `Deterministic`
  - `SopStepKind` enum: `Execute` | `Checkpoint`
  - `StepSchema` struct: JSON Schema fragments for input/output validation
  - `SopRunStatus` enum: `Running` | `PausedCheckpoint` | `Completed` | `Failed`
  - `SopRunAction` enum: `DeterministicStep` | `CheckpointWait` | `SupervisedStep`
  - `DeterministicRunState`: serializable run state for persistence + resume
  - `DeterministicSavings`: `llm_calls_saved` counter per run
- `crates/agentzero-tools/src/sop/engine.rs` — `SopEngine`:
  - `start_deterministic_run()` — begin sequential execution
  - `advance_deterministic_step()` — pipe output of step N → input of step N+1
  - `resume_deterministic_run()` — resume from persisted state
  - `persist_deterministic_state()` / `load_deterministic_state()` — workspace directory serialization
- `crates/agentzero-tools/src/sop/dispatch.rs` — action routing:
  - `DeterministicStep` — pipe outputs sequentially, no LLM
  - `CheckpointWait` — pause, require approval within `approval_timeout_secs`
  - `SupervisedStep` — delegate to LLM (existing path)
- `crates/agentzero-tools/src/sop/audit.rs` — audit logging for step transitions, checkpoint decisions, gate evaluations
- `crates/agentzero-tools/src/sop/metrics.rs` — metrics collection, `DeterministicSavings` tracking, run duration

**Modified files:**
- `crates/agentzero-tools/src/skills/sop.rs` — extend `SopStep` with:
  - `kind: SopStepKind`
  - `input_schema: Option<StepSchema>`
  - `output_schema: Option<StepSchema>`
  - `output: Option<serde_json::Value>`
- `crates/agentzero-tools/src/sop_tools.rs` — update all 5 tools:
  - `SopExecuteTool` — accept `deterministic: bool`, dispatch to engine
  - `SopAdvanceTool` — handle piped outputs in deterministic mode
  - `SopApproveTool` — add timeout enforcement
  - `SopStatusTool` — include savings tracking, checkpoint state
  - `SopListTool` — include execution mode in listing
- `crates/agentzero-config/src/model.rs` — add `[sop]` config section:
  - `sops_dir: PathBuf` (default `./sops`)
  - `default_execution_mode: String` ("supervised" | "deterministic")
  - `max_concurrent_total: u32` (default 4)
  - `approval_timeout_secs: u64` (default 300)
  - `max_finished_runs: u32` (default 100)

**Migration:** Existing SOPs continue working unchanged in supervised mode. Deterministic mode is opt-in per-SOP via `deterministic: true` in SOP definition.

**Tests:** Engine lifecycle (start/advance/complete), checkpoint pause/approve/timeout, state persist/resume, deterministic savings counting, schema validation, dispatch routing.

---

## Phase 5: Channel Enhancements (Medium effort, medium value)

### 5A. Automatic Media Understanding Pipeline (upstream #4158)

**What:** A `MediaPipeline` that auto-transcribes audio, tags images for vision processing, and annotates video attachments as they arrive through any channel.

**Why:** Agents receiving audio or images through channels can't process them automatically today.

**New files:**
- `crates/agentzero-channels/src/media.rs` — `MediaPipeline`:
  - `process_attachment(attachment) -> MediaAttachment` — routes by MIME type
  - Audio: transcribe via existing Whisper-compatible API → add transcript text
  - Image: tag for vision processing → add alt-text / description
  - Video: extract keyframes + transcribe audio track
  - Config-disabled by default (`[channels.media_pipeline] enabled = false`)

**Modified files:**
- `crates/agentzero-channels/src/lib.rs` — add `MediaAttachment` struct to `ChannelMessage`:
  - `mime_type: String`, `url: Option<String>`, `data: Option<Vec<u8>>`, `transcript: Option<String>`, `description: Option<String>`
- All channel implementations (~27 files) — minor additions to populate `attachments: Vec<MediaAttachment>` on `ChannelMessage` where the platform provides media. Most channels pass `vec![]`.
- `crates/agentzero-channels/src/pipeline.rs` — hook `MediaPipeline::process()` into existing message pipeline
- `crates/agentzero-config/src/model.rs` — add `[channels.media_pipeline]` config

**Tests:** Pipeline routing by MIME type, transcript attachment, graceful fallback on transcription failure.

---

### 5B. Discord History Logging and Search (upstream #4182)

**What:** Shadow listener that logs Discord messages to SQLite, plus a search tool for keyword search over history with human-readable name resolution.

**Why:** Persistent Discord context with searchable history. Name cache solves the opaque snowflake ID problem.

**New files:**
- `crates/agentzero-channels/src/channels/discord_history.rs` — `DiscordHistoryChannel`: shadow listener, logs messages without responding
- `crates/agentzero-tools/src/discord_search.rs` — `DiscordSearchTool`: keyword search over logged history
- SQLite schema additions in `crates/agentzero-storage/` — `discord_messages` table, `discord_name_cache` table (24h TTL refresh)

**Modified files:**
- `crates/agentzero-channels/src/lib.rs` — add to channel catalog
- `crates/agentzero-tools/src/lib.rs` — add module
- `crates/agentzero-infra/src/tools/mod.rs` — register search tool
- `crates/agentzero-config/src/model.rs` — add `[channels.discord_history]` config

**Tests:** Message logging, name resolution, search queries, TTL cache refresh.

---

### 5C. Voice Wake Word Detection (upstream #4162)

**What:** A `VoiceWakeChannel` with energy-based VAD, state machine (Listening → Triggered → Capturing → Processing), WAV encoding, and transcription-based wake word matching. Uses `cpal` for cross-platform audio.

**Why:** Differentiating feature for personal agents — voice-activated without cloud always-on listening.

**New files:**
- `crates/agentzero-channels/src/channels/voice_wake.rs` — `VoiceWakeChannel`:
  - `cpal` audio capture
  - Energy-based VAD with configurable threshold
  - State machine for capture lifecycle
  - WAV encoding of captured audio
  - Transcription via existing Whisper API
  - Wake word matching against configurable phrase list

**Modified files:**
- `crates/agentzero-channels/src/lib.rs` — add to channel catalog with `channel-voice-wake` feature gate
- `crates/agentzero-config/src/model.rs` — add `[channels.voice_wake]` config: `wake_words`, `energy_threshold`, `capture_timeout_secs`

**Caveats:**
- `cpal` brings heavy native dependencies (CoreAudio/ALSA/WASAPI) — conflicts with binary size reduction goals
- Must be strictly feature-gated, excluded from embedded builds
- Consider implementing after binary size reduction work stabilizes

**Tests:** VAD state machine transitions, wake word matching, capture timeout.

---

### 5D. Gmail Pub/Sub Push Notifications (upstream #4164)

**What:** Push-based Gmail channel using Google Pub/Sub webhooks, Gmail History API, 6-day subscription renewal, sender allowlist, HTML stripping, RFC 2822 reply encoding.

**Why:** Push is superior to polling for real-time email response. However, requires Google Cloud infrastructure.

**New files:**
- `crates/agentzero-channels/src/channels/gmail_push.rs` — `GmailPushChannel`:
  - Pub/Sub webhook handler
  - Gmail History API message fetching
  - Subscription management with 6-day auto-renewal (via cron/scheduler)
  - Sender allowlist
  - HTML stripping + RFC 2822 reply encoding
- `crates/agentzero-gateway/src/gmail_webhook.rs` — webhook endpoint

**Modified files:**
- `crates/agentzero-channels/src/lib.rs` — add to channel catalog
- `crates/agentzero-gateway/src/router.rs` — add webhook route
- `crates/agentzero-config/src/model.rs` — add `[channels.gmail_push]` config
- `crates/agentzero-auth/` — OAuth token management for Gmail API

**Caveats:**
- Requires Google Cloud project with Pub/Sub enabled
- Requires publicly routable gateway URL
- Our existing IMAP `EmailChannel` covers the common case
- Recommend as "power user" feature behind feature flag

**Tests:** Webhook payload parsing, subscription renewal, sender allowlist filtering, HTML stripping.

---

## Phase 6: Cross-Cutting Improvements

### 6A. Shared StreamToolCallAccumulator

Already covered in Phase 1D. Extract duplicated `ToolCallAccum` from `anthropic.rs` and `openai.rs` into `agentzero-core/src/types.rs` as a shared `StreamToolCallAccumulator`. This enables new providers to reuse the pattern without copy-paste.

### 6B. A2A Gaps (both implementations)

Neither our nor the upstream A2A implementation has:
- SSE streaming for real-time task updates (`tasks/sendSubscribe`)
- Webhook push notifications
- mTLS or OAuth authentication
- Agent registry / DNS-based discovery
- Persistent task store (both use in-memory HashMap)
- Task TTL / LRU eviction

These are tracked as future work beyond this plan. Persistent task store should use `agentzero-storage` (SQLite/Turso) when implemented.

---

## Dependency Graph

```
Phase 1 (all independent, can parallelize)
├── 1A: CLI Harness Tools
├── 1B: 429 Cooldown + Model Filtering
├── 1C: A2A Tool + Spec Alignment
├── 1D: Provider Streaming Wiring
├── 1E: Per-Sender Rate Limiting
└── 1F: Fallback Notification

Phase 2 (independent of Phase 1)
└── 2A: A2UI Live Canvas

Phase 3 (benefits from 1D streaming, but not hard dependency)
└── 3A: Background + Parallel Delegate

Phase 4 (independent)
└── 4A: Deterministic SOP Engine

Phase 5 (5A should precede 5C; others independent)
├── 5A: Media Pipeline ──→ 5C: Voice Wake Word
├── 5B: Discord History
└── 5D: Gmail Push

Phase 6 (cross-cutting, do alongside)
├── 6A: StreamToolCallAccumulator (part of 1D)
└── 6B: A2A gaps (future work)
```

---

## Estimated Scope

| Phase | Items | New Files | Modified Files | Approx Lines |
|-------|-------|-----------|----------------|-------------|
| 1 | 6 items | ~5 | ~20 | ~2,500 |
| 2 | 1 item | ~4 | ~8 | ~2,000 |
| 3 | 1 item | ~1 | ~5 | ~1,500 |
| 4 | 1 item | ~6 | ~4 | ~3,000 |
| 5 | 4 items | ~6 | ~35 | ~4,000 |
| 6 | 2 items | 0 | ~3 | ~200 |
| **Total** | **15 items** | **~22** | **~75** | **~13,200** |

---

## Open Questions

1. **CancellationToken migration scope** — Phase 3 adds `CancellationToken` alongside `AtomicBool`. Should we fully migrate all `is_cancelled()` call sites in a follow-up, or keep both indefinitely?
2. **Canvas persistence** — Should canvas frames persist to SQLite by default, or only when explicitly configured?
3. **Media pipeline default** — Should the media pipeline be opt-in or opt-out? Transcription costs money per API call.
4. **CLI harness security** — Should CLI harness tools require explicit per-binary allowlisting in config, or should a single `enable_cli_harness` flag unlock all?
5. **SOP migration** — Do we need a migration path for existing SOPs stored in `.agentzero/sops.json`, or can we start fresh with the new engine?
