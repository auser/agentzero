# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project follows Semantic Versioning.

## [Unreleased]

### Added
- Providers command improvements:
  - table output with padded columns
  - active/inactive colorization controls
  - JSON output mode (`providers --json`)

### Changed
- Provider module renamed and split:
  - crate renamed to `agentzero-providers`
  - provider catalog and provider implementation split into separate modules/files

## [0.1.0] - 2026-02-28

### Added
- Initial multi-crate workspace with CLI, runtime, config, core, tools, gateway, and security foundations.
- Interactive onboarding flow and initial command surfaces (`onboard`, `status`, `agent`, `gateway`, `doctor`, `providers`).
- Tool security policies, audit support, and baseline observability/bench harness.
