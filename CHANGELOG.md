# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project follows Semantic Versioning.

## [Unreleased]

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
