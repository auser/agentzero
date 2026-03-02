# AgentZero Sprint Plan — Sprint 17: Provider Hardening, Remaining Parity, and Live Operations

## Scope
Harden the provider layer (Anthropic native API, auth token lifecycle, transport resilience), close the last parity gaps carried forward from Sprint 14 (schedule tool, 4 remaining channels), and build the live operations foundation (gateway resilience, local model workflows, dashboard).

References:
- `specs/sprints/16-tool-channel-template-parity.md` (archived previous sprint)
- `specs/sprints/14-foundation-and-parity.md` (carry-forward items)
- `specs/sprints/backlog.md` (deferred multi-agent stack)
- `specs/research/001-multi-agent-stack.md` (multi-agent research)

## Sprint Cadence
- Sprint length: 1 week.
- Planning: Monday.
- Mid-sprint checkpoint: Wednesday.
- Review/retro: Friday.
- Rule: every merged PR updates this file.

## Tracking Conventions
- Each task uses one of: `[ ]` not started, `[-]` in progress, `[x]` done.
- Mark the acceptance criteria item as done in the same PR that implements the feature.
- If scope changes, update this file before coding.

## Dependencies and Critical Path
1. Phase A (Provider Hardening) — foundation; auth and transport must stabilize before live testing
2. Phase B (Remaining Parity) — independent of A; closes Sprint 14 carry-forwards
3. Phase C (Live Operations) — depends on A for stable providers; gateway and daemon must work end-to-end
4. Phase D (Documentation & Quality) — depends on A/B/C; captures changes in docs and tests

## Risks and Mitigations
- Risk: Anthropic Messages API response format drift.
  Mitigation: Pin API version header (`2023-06-01`), add response schema validation tests.
- Risk: OAuth token refresh requires live provider accounts for integration testing.
  Mitigation: Unit test with mock HTTP; document manual test procedure for live flows.
- Risk: MQTT/transcription channels require external broker/service.
  Mitigation: Test with mock transports; real integration tests are manual and documented.
- Risk: WhatsApp Web requires browser automation (Puppeteer/Playwright).
  Mitigation: Implement as stub with documented manual test; browser automation is future work.

---

## Phase A: Provider Hardening

### A1. Anthropic Native Provider
- [ ] Complete Anthropic Messages API implementation (`crates/agentzero-providers/src/anthropic.rs`)
- [ ] Support streaming responses (`stream: true` with SSE event parsing)
- [ ] Support tool use (tool_choice, tool definitions in Anthropic format)
- [ ] Support system prompt as top-level `system` field (not in messages array)
- [ ] Support vision/multimodal (image content blocks)
- [ ] Map Anthropic error responses to structured provider errors
- [ ] Add response validation tests (success, rate limit, auth error, malformed)

### A2. Transport Layer Resilience
- [ ] Add configurable request timeout per provider (`crates/agentzero-providers/src/transport.rs`)
- [ ] Add circuit breaker (open after N consecutive failures, half-open probe, close on success)
- [ ] Add request/response logging at trace level
- [ ] Add provider health check endpoint probing (used by `doctor models`)
- [ ] Wire transport config from `[provider]` section (`transport` field)

### A3. Auth Token Lifecycle
- [ ] Implement OAuth token refresh for OpenAI Codex (`crates/agentzero-auth/src/lib.rs`)
- [ ] Implement OAuth token refresh for Google Gemini
- [ ] Add token expiry detection before provider calls (auto-refresh if expired)
- [ ] Add `auth status` token health display (valid, expiring soon, expired)
- [ ] Add encrypted token storage migration (v1 → v2 format if needed)

### A4. Provider Catalog Improvements
- [ ] Add model capability metadata to catalog (vision, tool_use, streaming, max_tokens)
- [ ] Wire model capabilities into agent loop (skip tool_use for models that don't support it)
- [ ] Add `models status` display of current model capabilities
- [ ] Add cache invalidation on provider config change

### A-Acceptance
- [ ] Anthropic provider handles chat, tool use, and streaming
- [ ] Transport circuit breaker opens on consecutive failures and recovers
- [ ] Auth token auto-refresh works for OAuth providers
- [ ] `cargo test --workspace` passes

---

## Phase B: Remaining Parity (Sprint 14 Carry-Forwards)

### B1. Schedule Tool
- [ ] `schedule.rs` — Unified scheduling tool (wraps cron_add/cron_list for agent use with natural language schedule parsing)
- [ ] Parse natural-language time expressions ("every 5 minutes", "daily at 9am", "next Tuesday")
- [ ] Map to cron expressions or one-time timestamps
- [ ] Add tests for common scheduling patterns

### B2. Remaining Channels
- [ ] `mqtt.rs` — MQTT channel (connect to broker, subscribe to topic, publish responses)
- [ ] `transcription.rs` — Transcription channel (audio-to-text pipeline, microphone input or file)
- [ ] `whatsapp_storage.rs` — WhatsApp persistent session storage (message history, media cache)
- [ ] `whatsapp_web.rs` — WhatsApp Web channel (browser-based session, QR pairing)

### B-Acceptance
- [ ] Schedule tool parses natural-language time expressions into cron/timestamps
- [ ] MQTT channel connects, subscribes, and publishes with mock broker
- [ ] Remaining channels have at least stub + descriptor + 1 test each
- [ ] `cargo test --workspace` passes

---

## Phase C: Live Operations

### C1. Gateway Resilience
- [ ] Add rate limiting middleware (token bucket per paired client)
- [ ] Add request size limits (max body size, max header size)
- [ ] Add graceful shutdown (drain active connections before exit)
- [ ] Add pairing code rotation (auto-expire after configurable TTL)
- [ ] Add CORS configuration for browser clients
- [ ] Wire `[gateway]` config: `require_pairing`, `allow_public_bind`

### C2. Local Model Workflows
- [ ] Implement Ollama model pull with streaming progress bar (`models pull`)
- [ ] Implement local model health check with model-specific probes (`local health`)
- [ ] Add auto-discovery on daemon start (scan for local providers)
- [ ] Add `local discover` timeout and retry logic

### C3. Daemon Hardening
- [ ] Add log rotation for `daemon.log` (max size, max files)
- [ ] Add PID file locking (prevent duplicate daemon instances)
- [ ] Add daemon crash recovery (auto-restart with backoff)
- [ ] Add daemon health endpoint (internal liveness check)

### C4. Dashboard Foundation
- [ ] Implement basic TUI layout with ratatui (status panel, log panel, metrics panel)
- [ ] Wire real-time status from daemon (if running) or local state
- [ ] Display active channels, provider status, memory stats
- [ ] Support keyboard navigation and quit

### C-Acceptance
- [ ] Gateway rate limits and request size enforcement tested
- [ ] `models pull` downloads with progress for Ollama
- [ ] Daemon survives crash and restarts gracefully
- [ ] Dashboard renders status and exits cleanly
- [ ] `cargo test --workspace` passes

---

## Phase D: Documentation & Quality

### D1. Public Documentation
- [ ] Update quickstart guide with auth command as primary flow
- [ ] Update CLI command reference with any new flags from A/B/C
- [ ] Add provider-specific setup guides (OpenAI, Anthropic, OpenRouter, Ollama)
- [ ] Add gateway deployment guide (standalone, behind reverse proxy, Docker)

### D2. Integration Testing
- [ ] Add gateway integration tests (health, pair, chat round-trip with mock provider)
- [ ] Add daemon start/stop integration test
- [ ] Add auth token round-trip test (setup-token → agent call → verify header)
- [ ] Document manual test procedures for live provider flows

### D3. Quality Gates
- [ ] All new code passes `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] All new code passes `cargo fmt --all`
- [ ] Test coverage: every new feature has success + negative path tests
- [ ] No new `unsafe` blocks without justification

### D-Acceptance
- [ ] Public docs are accurate and match current CLI behavior
- [ ] Gateway integration tests pass end-to-end
- [ ] All quality gates pass
- [ ] `cargo test --workspace` passes

---

## Files to Create or Modify

| File | Phase | Action |
|------|-------|--------|
| `crates/agentzero-providers/src/anthropic.rs` | A1 | Edit — Complete streaming, tool use, vision |
| `crates/agentzero-providers/src/transport.rs` | A2 | Edit — Circuit breaker, timeouts, logging |
| `crates/agentzero-auth/src/lib.rs` | A3 | Edit — Token refresh, expiry detection |
| `crates/agentzero-providers/src/catalog.rs` | A4 | Edit — Model capability metadata |
| `crates/agentzero-tools/src/schedule.rs` | B1 | Create — Unified schedule tool |
| `crates/agentzero-channels/src/channels/mqtt.rs` | B2 | Create — MQTT channel |
| `crates/agentzero-channels/src/channels/transcription.rs` | B2 | Create — Transcription channel |
| `crates/agentzero-channels/src/channels/whatsapp_storage.rs` | B2 | Create — WhatsApp session storage |
| `crates/agentzero-channels/src/channels/whatsapp_web.rs` | B2 | Create — WhatsApp Web channel |
| `crates/agentzero-gateway/src/lib.rs` | C1 | Edit — Rate limiting, CORS, graceful shutdown |
| `crates/agentzero-daemon/src/lib.rs` | C3 | Edit — Log rotation, PID locking, crash recovery |
| `crates/agentzero-cli/src/commands/dashboard.rs` | C4 | Edit — TUI implementation |
| `public/src/content/docs/quickstart.md` | D1 | Edit — Auth-first flow |
| `public/src/content/docs/reference/commands.md` | D1 | Edit — New flags |
| `crates/agentzero-infra/src/tools/mod.rs` | B1 | Edit — Register schedule tool |
| `specs/SPRINT.md` | All | Edit — Track progress |

## Definition of Done (All Phases)
- Code compiles and tests pass locally.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` pass.
- New tools/channels have at least one success-path and one negative-path test.
- Feature has docs updates where behavior changes.
- `specs/SPRINT.md` is updated in the same PR.

## Sprint Change Log
- 2026-03-01: Sprint 17 created. Carried forward from Sprint 14: schedule tool, mqtt channel, transcription channel, whatsapp_storage, whatsapp_web. New scope: provider hardening (Anthropic, transport, auth lifecycle), live operations (gateway, daemon, dashboard, local models), documentation.
****