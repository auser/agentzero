# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project follows Semantic Versioning.

## [Unreleased]


## [0.4.2] - 2026-03-07

## [0.4.2] - 2026-03-07

### Added
- Populate changelog entries and auto-generate with git-cliff — Populate empty v0.4.0 and v0.4.1 release notes with curated content

### Fixed
- Mark agentzero-testkit as unpublishable to fix release — agentzero-testkit depends on agentzero-providers (publish = false),
## [0.4.1] - 2026-03-06

### Added
- **Per-project .env overrides** — Dotenv chain now scans both `~/.agentzero/` and CWD for `.env`, `.env.local`, and `.env.{AGENTZERO_ENV}` files; CWD files take priority over config-dir files for per-project overrides; duplicate loading avoided when CWD matches the config directory

### Fixed
- Add missing version specifiers to all workspace dependencies — 14 of 16 internal deps lacked versions, causing crates.io publish failures; `bump-versions` recipe now inserts versions into deps that lack them
- Push branch before tag in release recipe — version bump and changelog commits were stranded on local branch

## [0.4.0] - 2026-03-06

### Added
- **Orchestrator crate extraction** — Moves coordinator, agent_router, and swarm modules from `agentzero-gateway` to new `agentzero-orchestrator` crate, separating agent coordination logic from HTTP/WS transport for dependency-free reuse
- **Integration tests and e2e local LLM testing (Sprint 28)** — StaticProvider-based integration tests for agent chaining, privacy routing, pipeline execution, graceful shutdown, and correlation tracking; testkit helpers (`local_llm_provider`, `local_llm_available`, `wait_for_server`); 4 e2e tests against Ollama/tinyllama (`#[ignore]`); `e2e-tests` CI job; 1,750 tests passing
- **Conversation branching, multi-modal input, plugin registry refresh (Sprint 29)** — `MemoryEntry.conversation_id` and `ToolContext.conversation_id` fields; `MemoryStore` trait gains `recent_for_conversation`, `fork_conversation`, `list_conversations` with SQLite migration; `ContentPart` enum (`Text`/`Image`) on `ConversationMessage::User`; Anthropic `InputContentBlock::Image` and OpenAI `image_url` data URI support; `load_image_refs()` and `build_user_message()` wiring; CLI `conversation list/fork/switch` commands; `az plugin refresh --registry-url` command
- **HTTP registry fetch, plugin dependencies, audio input (Sprint 30)** — `load_registry_index()` and `refresh_registry_index()` accept `http(s)://` URLs; `install_from_url()` streams remote WASM packages; `PluginDependency { id, version_req }` with transitive resolution and cycle detection; `[AUDIO:path]` markers transcribed via Whisper-compatible API; `AudioConfig { api_url, api_key, language, model }`; graceful degradation when no API key configured; 13 unit tests for audio processing

### Fixed
- Clippy `len_zero` lint in test code; add `--all-targets` to pre-commit hook so test-only lints are caught
- Release recipe auto-fixes fmt/clippy in place instead of running read-only checks that hard-fail
- Bump plugin/fixture `Cargo.toml` versions missed by release recipe; extend recipe to handle standalone version lines

### Changed
- Pre-commit hook runs `cargo fmt --all` and `cargo clippy --fix --allow-staged` in place, re-stages with `git add -u`, then verifies with a clean clippy check
- `bump-versions` extracted as standalone recipe (`just bump-versions X.Y.Z`) from the release recipe for independent use
- Site documentation updated for Sprint 30 commands, architecture, and env vars

## [0.3.0] - 2026-03-05

### Added
- **Privacy end-to-end enforcement (Sprint 25)** — Memory entries carry `privacy_boundary` and `source_channel` fields with `recent_for_boundary()` filtering; channel messages propagate boundaries with `dispatch_with_boundary()` blocking `local_only` → non-local channels; Noise IK handshake for 1-round-trip fast reconnect; `agentzero privacy test` runs 8 diagnostic checks; integration wiring through `ToolContext.privacy_boundary` and leak guard `check_boundary()`
- **Production-ready privacy system (Sprint 24)** — Gateway initializes NoiseSessionStore, RelayMailbox, and key rotation on startup; client-side Noise handshake (`NoiseClientHandshake`, `NoiseClientSession`, `NoiseHttpTransport`); `GET /v1/privacy/info` endpoint; sealed envelope replay protection (nonce dedup, HTTP 409); local provider URL enforcement; network-level tool enforcement in `local_only` mode; plugin network isolation; per-component privacy boundaries (`PrivacyBoundary` enum with `resolve()` for agents, tools, channels); 6 Prometheus privacy metrics
- **Gateway production readiness (Sprint 23)** — Real Prometheus metrics with request instrumentation; dynamic `/v1/models` from provider catalog; WebSocket hardening (heartbeat ping/pong, idle timeout, binary frame rejection); structured `GatewayError` with 8 variants and JSON error responses; provider tracing spans on all 8 methods; storage test expansion (19 → 46 tests)
- Privacy CLI commands: `privacy status`, `privacy rotate-keys [--force]`, `privacy generate-keypair`, `privacy test [--json]`
- Noise Protocol handshake patterns: XX (mutual auth) and IK (known server key, fast reconnect)
- Per-component privacy boundaries for agents, tools, and channels with child-can't-exceed-parent enforcement
- Config validation: rejects `encrypted` mode without `noise.enabled`, boundary escalation, non-localhost URLs in `local_only`
- Responsive mobile navigation with hamburger menu for documentation site
- **Timing jitter for sealed envelope relay (Sprint 26)** — `JitterConfig` with configurable min/max delays for submit (10–100 ms) and poll (20–200 ms) to mitigate traffic analysis; wired through `SealedEnvelopeConfig` → `RelayMailbox::with_jitter()`
- **Privacy benchmarks (Sprint 26)** — Criterion 0.5 benchmarks for Noise keypair generation, XX/IK handshakes, encrypt/decrypt at 64B/1KB/64KB, sealed envelope seal+open, routing ID computation (11 functions behind `privacy` feature)
- **FFI privacy bindings (Sprint 26)** — `PrivacyBoundary`, `PrivacyInfo`, `PrivacyStatus` types exposed through UniFFI (Swift/Kotlin) and napi-rs (Node) for inspecting privacy state from mobile/Node apps

### Fixed
- Fix flaky `keyring_data_is_encrypted_on_disk` test — replace brittle 2-char substring check with longer plaintext field name assertions
- Fix flaky `set_config_value_creates_nested_keys` test — use unique temp dir to prevent parallel test collisions
- Resolve clippy `double_ended_iterator_last` lint for Rust 1.93
- Use vendored-openssl only on Windows, system OpenSSL elsewhere
- Noise middleware: empty-body requests with session header now get encrypted responses
- `IdentityKeyPair` no longer implements `Serialize` (prevents secret key leaks)

### Changed
- Privacy metrics (`record_key_rotation`, `record_encrypt_duration`) wired into actual code paths
- Pre-commit hook optimized: `cargo fmt --check` (read-only) instead of rewrite+re-stage

## [0.1.4] - 2026-03-02

## [0.1.3] - 2026-03-03

### Added
- Expand release build matrix: linux-armv7, linux-x86_64-musl, linux-aarch64-musl targets
- Installer auto-selects static musl binaries on Linux for better portability

### Fixed
- Fix stale v0.1.2 tag that pointed to a commit missing the changelog entry

## [0.1.2] - 2026-03-03

### Fixed
- Windows build: compare `HANDLE` with `.is_null()` instead of `== 0` to fix `E0308` mismatched-types errors
- Windows build: prefix unused `path` parameter in `enforce_private_permissions` with `_` to silence unused-variable warnings on non-Unix targets
- Add Windows support to `agentzero-daemon` via platform-conditional compilation (`#[cfg(unix)]` / `#[cfg(windows)]`)
- Resolve CI failures in checks, coverage, and security jobs
- Update `deny.toml` for `cargo-deny` config schema change
- Update path references from `public/` to `site/`
- Resolve CI/CD failures, upgrade wasmtime, and consolidate workflows

## [0.2.1] - 2026-03-02

### Fixed
- Eliminate flaky test failures from temp directory collisions (add PID to temp dir names)
- Skip TTY-dependent dashboard test when running in interactive terminal
- Use dynamic version in release verification benchmark test

### Added
- Channel setup module with `register_configured_channels` and `channels-standard` feature flag
- Expanded delegate tool with coordination, status tracking, and multi-agent support
- Gateway auth hardening with additional token validation and tests
- Config test coverage for policy flags (git, web_search, browser)
- `just release` now auto-bumps workspace version in Cargo.toml

## [0.2.0] - 2026-03-01

### Added
- Full tool parity: SOP tools (5), CLI discovery, proxy config, composio, pushover, hardware debug tools (3), WASM plugin tools (2)
- Full channel parity: 23 channels (Telegram, Discord, Slack, WhatsApp, Signal, iMessage, Matrix, Mattermost, IRC, Email, Nostr, Lark, Feishu, DingTalk, Nextcloud Talk, LinQ, WATI, QQ Official, Napcat, ACP, ClawdTalk, CLI)
- Template system: 8 templates with 3-tier precedence, discovery, validation, and CLI commands
- CLI completeness: skill new/audit/templates, 113 integration tests, gateway manual test script
- Persistence migration: all sensitive state uses encrypted stores
- Channel binding generalization: unified `channel add/remove` flow
- Workspace version consolidation: all crates use `version.workspace = true`
- Providers command improvements (table output, colorization, JSON mode)

### Changed
- Provider module renamed and split into `agentzero-providers` crate
- Removed `bind-telegram` special-case in favor of generic `channel add telegram`

## [0.1.0] - 2026-02-28

### Added
- Initial multi-crate workspace with CLI, runtime, config, core, tools, gateway, and security foundations.
- Interactive onboarding flow and initial command surfaces (`onboard`, `status`, `agent`, `gateway`, `doctor`, `providers`).
- Tool security policies, audit support, and baseline observability/bench harness.
