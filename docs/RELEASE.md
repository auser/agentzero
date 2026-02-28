# Release Process

This document defines the versioning and changelog process used for tagged releases.

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
