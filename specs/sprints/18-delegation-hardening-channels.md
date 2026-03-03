# AgentZero Sprint Plan — Sprint 18: Sub-Agent Delegation, Hardening, and Channel Activation

## Scope
Fix the sub-agent delegation execution path (5 bugs), harden test coverage across gateway auth/handlers and config policies, and activate real channel implementations behind feature gates.

References:
- `specs/sprints/17-provider-hardening-live-ops.md` (previous sprint)
- `specs/sprints/backlog.md` (deferred items)

## Sprint Cadence
- Sprint length: 1 week.
- Planning: Monday.
- Mid-sprint checkpoint: Wednesday.
- Review/retro: Friday.

## Tracking Conventions
- Each task uses one of: `[ ]` not started, `[-]` in progress, `[x]` done.

## Phase A: Sub-Agent Delegation — [x] Complete

### A1. Fix provider resolution in `build_delegate_agents` — [x]
- Added `provider_kind: String` field to `DelegateConfig`
- `build_delegate_agents()` resolves base URL via `find_provider()` catalog
- Sets `provider_kind` for dispatch and `provider` for actual base URL

### A2. Use `build_provider` factory in delegate tool — [x]
- Replaced hardcoded `OpenAiCompatibleProvider::new()` with `build_provider(kind, url, key, model)`
- Anthropic delegates now route through `AnthropicProvider` (Messages API)
- API key resolution uses provider-specific env vars before `OPENAI_API_KEY` fallback

### A3. Wire tools into agentic sub-agents — [x]
- Introduced `ToolBuilder` callback: `Arc<dyn Fn() -> Result<Vec<Box<dyn Tool>>> + Send + Sync>`
- `DelegateTool::new()` takes optional `ToolBuilder` + `ToolSecurityPolicy`
- `run_agentic()` builds full tool set via builder, filters by `allowed_tools`
- `filter_tools()` excludes `"delegate"` to prevent infinite chains

### A4. Inject system prompt and temperature — [x]
- System prompt prepended to user prompt in delegate execution
- Temperature passed through to provider config

### A5. Integration tests — [x]
- Single-shot delegation with mock provider
- Agentic delegation with tool filtering
- Depth limit enforcement
- Provider dispatch verification

## Phase B: Hardening & Test Coverage — [x] Complete

### B1. Gateway auth unit tests — [x]
- 17 unit tests covering all branches of `authorize_request`, `token_in_state`, `parse_bearer`
- Tests: open mode, bearer auth, paired tokens, always_require_pairing, edge cases

### B2. Gateway handler coverage — [x]
- 11 new handler tests (52 total gateway tests)
- Covers: dashboard HTML, metrics prometheus, v1_models, ping success, api_chat echo,
  v1_chat_completions OpenAI format, legacy_webhook, webhook unknown channel → 404,
  pair missing header, v1_models auth required

### B3. Config policy test coverage — [x]
- 5 new tests: enable_git from allowed_commands, enable_web_search, enable_browser,
  CIDR parse error, absolute allowed_root

### B4. Fix all-channels warnings — [x]
- `#[allow(unused_macros)]` on `channel_stub!`
- `#[allow(unused_imports)]` on re-export
- `tx` → `_tx` in whatsapp.rs
- `cargo build -p agentzero-channels --features all-channels` — zero warnings

### B5. Fix cli_discovery docstring — [x]
- Removed `list_tools` from docstring (operation doesn't exist)

## Phase C: Channel Activation — [x] Complete

### C1. `channels-standard` feature profile — [x]
- 10 channels: cli, telegram, discord, slack, mattermost, matrix, email, irc, nostr, webhook
- Pass-through features in CLI and binary Cargo.toml

### C2. Wire channel construction from config — [x]
- `ChannelInstanceConfig` struct with optional fields for all channel types
- `register_configured_channels(registry, configs)` — config-driven channel construction
- `register_one()` with `#[cfg(feature)]`-gated match arms for each channel type
- Constructors: Telegram, Discord, Slack, Mattermost, Matrix, Email, IRC, Nostr
- Tests: empty configs, unknown channel skip, telegram missing token, telegram success

### C3. Fix Slack Socket Mode dependency — [x]
- Added `tokio-tungstenite` to `channel-slack` feature gate

### C4. Channel tests — [x]
- Constructor validity, name identity, config error handling
- 140 channel tests with channels-standard enabled

## Quality Gates — [x] All Passing
- `cargo fmt --all` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings
- `cargo test --workspace --exclude agentzero-local --exclude agentzero-bench` — all pass
- `cargo build -p agentzero-channels --features all-channels` — zero warnings

## Files Modified

| File | Phase | Change |
|------|-------|--------|
| `crates/agentzero-delegation/src/lib.rs` | A1 | `provider_kind` field in `DelegateConfig` |
| `crates/agentzero-runtime/src/lib.rs` | A1 | `build_delegate_agents` URL resolution |
| `crates/agentzero-tools/src/delegate.rs` | A2-A5 | `build_provider`, ToolBuilder, tests |
| `crates/agentzero-tools/src/lib.rs` | A3 | ToolBuilder export |
| `crates/agentzero-infra/src/tools/mod.rs` | A3 | DelegateTool construction |
| `crates/agentzero-gateway/src/auth.rs` | B1 | 17 auth unit tests |
| `crates/agentzero-gateway/src/tests.rs` | B2 | 11 handler tests |
| `crates/agentzero-config/src/tests.rs` | B3 | 5 policy tests |
| `crates/agentzero-channels/src/channels/mod.rs` | B4 | Warning fixes, channel_setup module |
| `crates/agentzero-channels/src/channels/whatsapp.rs` | B4 | `tx` → `_tx` |
| `crates/agentzero-tools/src/cli_discovery.rs` | B5 | Docstring fix |
| `crates/agentzero-channels/Cargo.toml` | C1,C3 | channels-standard, Slack fix |
| `crates/agentzero-cli/Cargo.toml` | C1 | channels-standard pass-through |
| `bin/agentzero/Cargo.toml` | C1 | channels-standard pass-through |
| `crates/agentzero-channels/src/channels/channel_setup.rs` | C2 | NEW — config-driven channel construction |
| `crates/agentzero-channels/src/lib.rs` | C2 | Re-export channel_setup types |
