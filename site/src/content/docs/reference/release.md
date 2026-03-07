---
title: Release Process
description: Versioning and changelog process used for tagged releases.
---

This document defines the versioning and changelog process used for tagged releases.

## Release History

### [0.4.0] — 2026-03-06

#### Added

- **HTTP registry fetch** — `az plugin install --url <https://...>` and `az plugin refresh --registry-url <https://...>` now accept `https://` and `http://` URLs in addition to `file://`. The registry index loader and refresher both support remote URLs.
- **Plugin dependency resolution** — `PluginManifest` has an optional `dependencies: Vec<PluginDependency>` field (defaults to empty). Each `PluginDependency` carries an `id` and a `version_req` semver string. Running `az plugin install --registry-url <url>` resolves and installs all transitive dependencies before the top-level plugin. Circular dependency chains are detected and reported as errors.
- **Audio input** — User messages may contain `[AUDIO:/path/to/file.wav]` markers. The agent transcribes each marker via a configurable OpenAI-compatible endpoint before forwarding the message to the LLM. Supported formats: `flac`, `mp3`, `mp4`, `m4a`, `ogg`, `opus`, `wav`, `webm` (max 25 MB). If `[audio] api_key` is not set, markers are stripped silently with a warning. Default endpoint: Groq Whisper (`whisper-large-v3`).
- **`[audio]` config section** — New `agentzero.toml` section with `api_url`, `api_key`, `language`, and `model` fields.

#### Changed

- Workspace version bumped to `0.4.0` across all publishable crates.

---

## Versioning Policy

- Use Semantic Versioning: `MAJOR.MINOR.PATCH`.
- During this stage, all publishable crates in this workspace are versioned in lockstep.
- Pre-release tags (for example `1.2.0-rc.1`) are allowed when needed.

## Changelog Policy

- `CHANGELOG.md` is required and must contain:
  - `## [Unreleased]` section for in-flight work.
  - A versioned section for each release in the form:
    - `## [X.Y.Z] - YYYY-MM-DD`
- Every user-visible behavior change should be listed under `Added`, `Changed`, or `Fixed`.

## Pre-Release Checklist

1. Ensure all intended changes are merged.
2. Run quality gates:
   - `cargo fmt --all`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - `scripts/run-coverage.sh --output-dir coverage` (requires `cargo-llvm-cov`)
3. Verify release metadata and changelog entry for the target version:
   - `scripts/verify-release-version.sh --version X.Y.Z`
4. Tag and publish using CI release workflow.

## GitHub Workflows

- `ci.yml`
  - Pull request and push validation (`fmt`, `clippy`, `test`, security checks).
- `cd.yml`
  - Continuous delivery on `main`: runs quality checks and uploads release-build artifacts for Linux/macOS/Windows.
- `release.yml`
  - Triggered by `v*.*.*` tags (or manual dispatch with version input).
  - Verifies changelog/version consistency, builds artifacts on Linux/macOS/Windows, and publishes GitHub Release assets.

## Practical Notes

- If you bump crate versions, keep all workspace crate versions aligned.
- Keep `Unreleased` at the top and move entries into the new release heading at cut time.
