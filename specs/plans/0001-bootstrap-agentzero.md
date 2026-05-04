# Bootstrap AgentZero

## Objective

Create the smallest secure local agent harness that can initialize a private project, run a local supervised chat session, inspect a repository, detect PII and secrets, enforce policy, and produce an audit report.

## Phase 0: Documentation Gate

- [x] Complete `specs/project.md`.
- [x] Add ADRs 0001-0010.
- [x] Add `specs/security-model.md`.
- [x] Update `specs/SPRINT.md`.
- [x] Run `just ci` in target repository.

## Phase 1: Rust Workspace

- [x] Add root `Cargo.toml` (workspace + `agentzero-core` top-level crate).
- [x] Add `crates/agentzero-cli` (CLI with doctor, demo, init, chat, run, policy, audit, vault).
- [x] Add `crates/agentzero-policy` (deny-by-default policy engine).
- [x] Add `crates/agentzero-audit` (JSONL audit logger).
- [x] Add `crates/agentzero-tools` (tool registry + 3 built-in schemas).
- [x] Add `crates/agentzero-skills` (skill manifest + validation).
- [x] Add `crates/agentzero-sandbox` (sandbox profile contracts).

## Phase 2: CLI Skeleton

- [ ] Implement `agentzero --help`.
- [ ] Implement `agentzero init --private`.
- [ ] Implement `agentzero doctor`.
- [ ] Implement `agentzero policy status`.
- [ ] Implement `agentzero audit tail`.
- [ ] Implement `agentzero vault` command namespace.

## Phase 3: Security Primitives

- [ ] Implement policy loading.
- [ ] Implement data classification enum.
- [ ] Implement redaction interface.
- [ ] Implement secret scanning interface.
- [ ] Implement audit event schema.
- [ ] Implement deny-by-default policy decisions.
- [ ] Implement approval scope model.

## Phase 4: Minimal Session Engine

- [ ] Implement local-only session mode.
- [ ] Implement model provider abstraction.
- [ ] Implement supervised tool invocation.
- [ ] Implement read/list/search tools.
- [ ] Implement proposed edit output.
- [ ] Implement shell approval stubs.

## Phase 5: First Demo

- [x] Add built-in `repo-security-audit` skill (scanner + report generator).
- [x] Patterns loaded from external `skills/repo-security-audit/patterns.toml`.
- [x] Run against this repository via `agentzero run repo-security-audit`.
- [x] Produce human-readable markdown audit report.
- [x] Add malicious fixture tests (12 scanner + 3 report tests).

## Definition of Done

- [x] `cargo test --workspace` passes (135 tests).
- [x] `cargo clippy --workspace -- -D warnings` passes.
- [x] `just ci` passes.
- [x] No TODOs or unimplemented production paths.
- [x] Sprint is updated.
