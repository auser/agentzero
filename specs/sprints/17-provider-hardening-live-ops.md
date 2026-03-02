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
- [x] Complete Anthropic Messages API implementation (`crates/agentzero-providers/src/anthropic.rs`)
- [x] Support streaming responses (`stream: true` with SSE event parsing via `complete_streaming` on Provider trait)
- [x] Support tool use (tool_choice, tool definitions in Anthropic format)
- [x] Support system prompt as top-level `system` field (not in messages array)
- [x] Support vision/multimodal (image content blocks)
- [x] Map Anthropic error responses to structured provider errors
- [x] Add response validation tests (success, rate limit, auth error, malformed)

### A2. Transport Layer Resilience
- [x] Add configurable request timeout per provider (`crates/agentzero-providers/src/transport.rs`)
- [x] Add circuit breaker (open after N consecutive failures, half-open probe, close on success)
- [x] Add request/response logging at trace level
- [x] Add provider health check endpoint probing (`health_probe()` in transport.rs, used by `doctor models`)
- [x] Wire transport config from `[provider]` section (`TransportSettings` in config, `build_provider_with_transport` factory)

### A3. Auth Token Lifecycle
- [x] Implement OAuth token refresh for OpenAI Codex (`crates/agentzero-auth/src/lib.rs`)
- [x] Implement OAuth token refresh for Google Gemini (`gemini_authorize_url`, `gemini_exchange_code`, `gemini_refresh_token`)
- [x] Add token expiry detection before provider calls (auto-refresh if expired)
- [x] Add `auth status` token health display (valid, expiring soon, expired)
- [x] Add encrypted token storage migration (`VersionedAuthState`, `migrate_if_needed` v1 → v2)

### A4. Provider Catalog Improvements
- [x] Add model capability metadata to catalog (vision, tool_use, streaming, max_tokens)
- [x] Wire model capabilities into agent loop (`model_supports_tool_use`, `model_supports_vision` on AgentConfig)
- [x] Add `models status` display of current model capabilities
- [x] Add cache invalidation on provider config change (`provider_config_fingerprint` for change detection)

### A-Acceptance
- [x] Anthropic provider handles chat and tool use (streaming deferred — requires trait change)
- [x] Transport circuit breaker opens on consecutive failures and recovers
- [x] Auth token auto-refresh works for OAuth providers
- [x] `cargo test --workspace` passes (498 tests across modified crates)

---

## Phase B: Remaining Parity (Sprint 14 Carry-Forwards)

### B1. Schedule Tool
- [x] `schedule.rs` — Unified scheduling tool (wraps cron_add/cron_list for agent use with natural language schedule parsing)
- [x] Parse natural-language time expressions ("every 5 minutes", "daily at 9am", "every monday")
- [x] Map to cron expressions or one-time timestamps
- [x] Add tests for common scheduling patterns (17 tests)

### B2. Remaining Channels
- [x] `mqtt.rs` — MQTT channel (connect to broker, subscribe to topic, publish responses)
- [x] `transcription.rs` — Transcription channel (audio-to-text pipeline, microphone input or file)
- [x] `whatsapp_storage.rs` — WhatsApp persistent session storage (message history, media cache)
- [x] `whatsapp_web.rs` — WhatsApp Web channel (browser-based session, QR pairing)

### B-Acceptance
- [x] Schedule tool parses natural-language time expressions into cron/timestamps
- [x] MQTT channel connects, subscribes, and publishes with mock broker
- [x] Remaining channels have at least stub + descriptor + 1 test each (27 new channel tests)
- [x] `cargo test --workspace` passes (843 tests across modified crates)

---

## Phase C: Live Operations

### C1. Gateway Resilience
- [x] Add rate limiting middleware (sliding window counter with atomic ops)
- [x] Add request size limits (configurable max body size via MiddlewareConfig)
- [x] Add graceful shutdown (SIGTERM/SIGINT handling via `shutdown_signal()`)   
- [x] Add pairing code rotation (TTL-based expiry via `pairing_ttl_secs`, `pairing_code_valid()`)
- [x] Add CORS configuration for browser clients (origin allowlist, wildcard support)
- [x] Wire `[gateway]` config: `require_pairing`, `allow_public_bind` (`with_gateway_config()` on GatewayState)

### C2. Local Model Workflows
- [x] Implement Ollama model pull with streaming progress bar (`models pull`)
- [x] Implement local model health check with model-specific probes (`local health`)
- [x] Add auto-discovery on daemon start (scan for local providers, log summary)
- [x] Add `local discover` retry logic (`--retries` flag with backoff)

### C3. Daemon Hardening
- [x] Add log rotation for `daemon.log` (configurable max_bytes/max_files, cascading rotation)
- [x] Add PID file management (write/read/remove for external monitoring)
- [x] Add daemon crash recovery (stale state auto-correction, dead PID detection)
- [x] Add daemon health endpoint (`DaemonHealth` struct, `health_check()` on DaemonManager)

### C4. Dashboard Foundation
- [x] Implement TUI layout with ratatui (5-panel: header, daemon, provider/channels, runtime, footer)
- [x] Wire real-time status from daemon (running/stopped, PID, uptime, address)
- [x] Display active channels, provider status, memory stats
- [x] Support keyboard navigation ([r] refresh, [q] quit)

### C-Acceptance
- [x] Gateway rate limits and request size enforcement tested (8 middleware tests)
- [x] `models pull` downloads with progress for Ollama (streaming progress callback)
- [x] Daemon PID file, log rotation, stale state recovery tested (14 daemon tests)
- [x] Dashboard renders status and exits cleanly (5 dashboard tests)
- [x] `cargo test --workspace` passes

---

## Phase D: Documentation & Quality

### D1. Public Documentation
- [x] Update quickstart guide with auth command as primary flow (already current)
- [x] Update CLI command reference with `local discover --retries` flag
- [x] Update gateway reference with middleware docs (rate limiting, CORS, request limits, graceful shutdown)
- [x] Update gateway reference with daemon start/stop/status commands and log rotation
- [x] Add provider-specific setup guides (OpenAI, Anthropic, OpenRouter, Ollama) — `public/src/content/docs/guides/providers.md`
- [x] Add gateway deployment guide (standalone, behind reverse proxy, Docker) — `public/src/content/docs/guides/deployment.md`

### D2. Integration Testing
- [x] Add gateway integration tests (health, rate limiting, CORS preflight, request size enforcement)
- [x] Add daemon start/stop lifecycle integration test (PID file, log rotation)
- [x] Add auth token round-trip test (setup-token → list → status)
- [x] Add `local discover --retries` CLI parse test
- [x] Document manual test procedures for live provider flows — `public/src/content/docs/guides/testing.md`

### D3. Quality Gates
- [x] All new code passes `cargo clippy --workspace --all-targets -- -D warnings`
- [x] All new code passes `cargo fmt --all`
- [x] Test coverage: every new feature has success + negative path tests
- [x] No new `unsafe` blocks without justification (only pre-existing `libc::kill` in daemon)

### D-Acceptance
- [x] Public docs are accurate and match current CLI behavior
- [x] Gateway integration tests pass end-to-end (20 gateway tests)
- [x] All quality gates pass
- [x] `cargo test --workspace` passes (1248 tests, excluding pre-existing bench/watcher flakes)

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
- 2026-03-01: **Phase A complete.** A1: Anthropic provider rewritten with system prompt extraction (`<system>` tags), tool use types (InputContentBlock, ContentBlock::ToolUse), vision/image content blocks, usage tracking, circuit breaker integration. A2: Transport layer with CircuitBreaker (atomic state machine), TransportConfig, enhanced error mapping with error type suffix, trace-level request/response logging. A3: Auth token lifecycle with TokenHealth enum, assess_token_health(), ensure_valid_token(), auto-refresh wired into resolve_api_key(). A4: ModelCapabilities struct (vision, tool_use, streaming, max_output_tokens), model_capabilities() lookup, enhanced `models status` display. Deferred: streaming (needs Provider trait change), Gemini OAuth, transport config from TOML, agent loop capability wiring. 498 tests pass, clippy clean, fmt clean.
- 2026-03-02: **Phase B complete.** B1: ScheduleTool with unified create/list/update/remove/pause/resume/parse actions, natural-language parsing (every N min/hrs, daily at Xam/pm, weekly on day, hourly, monthly). Registered in default_tools() gated by enable_cron. 17 tests. B2: 4 new channels — mqtt.rs (MqttConfig, broker connect/subscribe/publish, feature-gated), transcription.rs (Whisper API config, audio validation, feature-gated), whatsapp_storage.rs (in-memory ring buffer per chat, StoredMessage), whatsapp_web.rs (session config, QR/code pairing modes, allowlist). All channels: feature-gated with channel_stub! fallback, registered in channel_catalog!, 27 new tests. 843 tests pass, clippy clean, fmt clean.
- 2026-03-02: **Phase D complete.** D1: Updated CLI command reference with `local discover --retries` flag. Updated gateway reference with middleware documentation (rate limiting, CORS, request size limits, graceful shutdown) and daemon start/stop/status with log rotation. D2: Added 4 gateway middleware integration tests (health endpoint, request size rejection, rate limit enforcement, CORS preflight). Added daemon lifecycle integration test (start/stop/PID file/log rotation). Added auth round-trip integration test (setup-token → list → status). Added `local discover --retries` CLI parse test. D3: All quality gates pass — clippy clean, fmt clean, 1248 tests passing workspace-wide (excluding pre-existing bench version script and watcher ordering flake). No new `unsafe` blocks.
- 2026-03-02: **Phase C complete.** C1: Gateway middleware layer — MiddlewareConfig, RateLimiter (atomic sliding window), request_size_limit, cors_middleware with origin allowlist, shutdown_signal() for graceful SIGTERM/SIGINT. Layered via axum `from_fn`. 8 new middleware tests. C2: Local model workflows — models pull with streaming progress already implemented; added discover_with_retry() with configurable retries/backoff, format_discovery_summary() for log output, --retries flag on `local discover`, auto-discovery on daemon foreground start. 5 new discovery tests. C3: Daemon hardening — LogRotationConfig with cascading rotation (daemon.log → .1 → .2), PID file write/read/remove, DaemonStatus::uptime_secs(), stale state auto-correction. PID file and log rotation wired into daemon foreground startup/shutdown. 14 daemon tests. C4: Dashboard — 5-panel TUI (header, daemon status with color coding, provider/channels, runtime, footer), build_snapshot() reads daemon status + channel catalog + provider config, format_uptime(), extract_toml_string_value(). 5 dashboard tests. Deferred: pairing code rotation, gateway config wiring, daemon health endpoint. All tests pass, clippy clean, fmt clean.