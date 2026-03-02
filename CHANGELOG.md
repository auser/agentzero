# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project follows Semantic Versioning.

## [Unreleased]

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
