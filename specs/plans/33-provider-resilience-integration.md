# Sprint 62: Provider Resilience & Integration — Remaining Gaps

## Context

Most of Sprint 62 was already implemented in prior sprints. This plan covers only the remaining gaps identified by code audit on 2026-03-24.

## Already Implemented (mark as [x] in SPRINT.md)

### Phase A: CLI Harness Tools
- `CodexCliTool` in `crates/agentzero-tools/src/codex_cli.rs`
- `GeminiCliTool` in `crates/agentzero-tools/src/gemini_cli.rs`
- `OpenCodeCliTool` in `crates/agentzero-tools/src/opencode_cli.rs`
- Shared env sanitization via `BLOCKED_ENV_PREFIXES` in each tool
- `enable_cli_harness: bool` on `ToolSecurityPolicy`
- All registered in `default_tools_inner()` under `tools-full` feature

### Phase B: 429 Cooldown (partially)
- `CooldownState` struct in `transport.rs` with `enter_cooldown()`, `is_cooled_down()`, `clear()`
- `FallbackProvider` has `cooldowns: Vec<CooldownState>` parallel array
- `is_rate_limit_error()` detection + cooldown activation in fallback loop
- `provider_supports_model()` exists in `models.rs`

### Phase D: Streaming Wiring (partially)
- `StreamToolCallAccumulator` extracted to `agentzero-core/src/types.rs`
- `supports_streaming()` on `Provider` trait (default false, true for Anthropic/OpenAI)
- `DraftTracker` exists in `agentzero-channels/src/drafts.rs`

### Phase E: Per-Sender Rate Limiting
- `SenderRateLimiter` fully implemented in `agentzero-infra/src/sender_rate_limiter.rs` (4 tests)
- `sender_id: Option<String>` on `ToolContext`

### Phase F: Fallback Notification
- `FallbackInfo` task-local in `fallback.rs`
- `append_fallback_footer()` in `agentzero-channels/src/lib.rs` (3 tests)
- `X-Provider-Fallback` / `X-Provider-Used` headers in gateway `handlers.rs`

### Phase C: A2A Tool (partially)
- `A2aTool` in `agentzero-tools/src/a2a.rs` with discover/send/status/cancel
- URL scheme validation
- `message/send` alias accepted alongside `tasks/send` in gateway
- `bearer_token` in `A2aConfig`
- `tokio::sync::Mutex` (not std) on `A2aTaskStore`

## Remaining Work

### 1. Model Compatibility Filtering in FallbackProvider
**File:** `crates/agentzero-providers/src/fallback.rs`
**Gap:** `provider_supports_model()` exists but is NOT called in the fallback loop. When FallbackProvider tries provider N, it doesn't check if the model is compatible with that provider.
**Fix:** In `try_providers()` (or each `complete*` method), call `provider_supports_model(label, model)` before attempting. Skip incompatible pairs with `info!` log. Need to thread model name through — currently the model is embedded in the provider config, not visible to FallbackProvider.
**Complexity:** Medium — need to expose model name from providers or pass it into FallbackProvider.

### 2. A2A Client Extensions
**File:** `crates/agentzero-orchestrator/src/a2a_client.rs`
**Gap:** `A2aAgentEndpoint` only has `new()` and `fetch_agent_card()`. Missing `check_status(task_id)` and `cancel_task(task_id)`.
**Fix:** Add two async methods that POST JSON-RPC `tasks/get` and `tasks/cancel` to the A2A agent URL.

### 3. Agent Card URL Population
**File:** `crates/agentzero-gateway/src/a2a.rs`
**Gap:** Agent Card `url` field is hardcoded to `"http://localhost"`.
**Fix:** Read `public_url` from gateway config (already exists for webhook registration) and populate Agent Card.

### 4. A2A Inbound Auth Enforcement
**File:** `crates/agentzero-gateway/src/a2a.rs`
**Gap:** `bearer_token` config field exists but may not be enforced on inbound `/a2a` requests.
**Fix:** Check `state.config.a2a.bearer_token` and verify `Authorization: Bearer <token>` header.

### 5. DraftTracker Streaming Consumer Wiring
**File:** `crates/agentzero-channels/src/streaming.rs` or dispatch loop
**Gap:** Verify DraftTracker is actually wired in the channel handler path to deliver token-by-token draft updates.
**Fix:** In channel dispatch, spawn task reading `StreamChunk` from receiver, calling `DraftTracker::update()`.

## Verification

- `cargo clippy --workspace --lib` — 0 warnings
- `cargo test --workspace` — all tests pass
- New tests for: model compat skip in fallback, A2A client check_status/cancel, Agent Card URL, inbound auth rejection
