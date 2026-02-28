# AGENTS.md

## Purpose
Project-level operating rules for all contributors and coding agents working in this repository.

## Required Workflow Rules

### 1) Tests are mandatory for every functionality change
- Every feature change must include tests in the same PR.
- Every bug fix must include a regression test that fails before and passes after the fix.
- Required minimum:
- At least one success-path test.
- At least one negative-path test.
- No merge if tests are missing.
- Agent enforcement:
- Codex must add/update tests for every code change it makes.
- If a test is truly not feasible, Codex must explicitly state why and propose the nearest practical regression check.

### 2) Keep sprint plan current at all times
- `specs/SPRINT.md` is the source of truth for execution status.
- When starting work:
- Mark task status from `[ ]` to `[-]`.
- When finishing work:
- Mark task status from `[-]` to `[x]`.
- Update acceptance criteria status in the same PR.
- If scope changes, update `specs/SPRINT.md` before implementation.

### 3) Definition of done enforcement
- A task is done only if:
- Code is implemented.
- Tests exist and pass.
- Docs are updated (if behavior changes).
- `specs/SPRINT.md` is updated.

### 4) Quality gates (must pass before merge)
- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

### 5) Architecture and scope discipline
- Follow current scope in `docs/adr/0001-scope.md`.
- Do not add daemon/channels/hardware/plugin/RAG work unless explicitly scheduled in sprint docs.
- Any major scope/module expansion requires ADR update.

### 6) Crate boundary policy (major module per crate)
- When practical, each major functionality module must live in its own crate.
- Examples of major modules:
- config, provider implementation, memory backend, tools by risk domain, observability, runtime orchestration.
- Avoid large “misc infra” crates that accumulate unrelated concerns.
- Allowed exception:
- very small glue logic that would add churn if split; exception must be documented in `specs/SPRINT.md`.

### 7) Security is P0 and blocks feature work
- Security tasks in `Sprint 0` are highest priority and must be completed before non-critical expansion.
- Any new feature that increases attack surface requires:
- threat model update (`docs/security/THREAT_MODEL.md`)
- security tests (success + abuse/negative paths)
- explicit policy checks (fail-closed behavior)
- No merge for security-sensitive functionality without tests and policy enforcement.

## Preferred PR Checklist
- [ ] Functionality implemented
- [ ] Success-path tests added
- [ ] Negative-path tests added
- [ ] `specs/SPRINT.md` task + acceptance updated
- [ ] Docs updated (`docs/COMMANDS.md`/README as needed)
- [ ] `fmt`, `clippy`, and `test` all pass
