# AGENTS.md

## Purpose

This file defines mandatory operating rules for AI agents and developers working in AgentZero.

AgentZero is documentation-first and security-first. Work is governed by explicit architectural decisions, project invariants, system guarantees, plans, and sprint state. Implementation must follow those rules.

## Read Order

Before making changes, always read:

1. `specs/project.md`
2. `specs/security-model.md`
3. `specs/adrs/`
4. `specs/plans/`
5. `specs/SPRINT.md`
6. any package-level documentation relevant to the area being changed

## Core Rules

- NEVER violate project invariants.
- ALWAYS conform to accepted ADRs.
- ALWAYS keep implementation aligned with `specs/project.md`.
- ALWAYS treat `specs/security-model.md`, `.agentzero/policy.yml`, and accepted security ADRs as mandatory enforcement contracts, not advisory documentation.
- ALWAYS update `specs/SPRINT.md` when work starts, changes, or completes.
- NEVER add architecture that is not reflected in project docs and ADRs.
- NEVER add temporary production shortcuts without explicit ADR coverage.
- NEVER introduce hidden operator bypasses or undocumented privileged paths.
- ALWAYS prefer explicit contracts over informal conventions.
- ALWAYS preserve security, redaction, auditability, and maintainability guarantees defined by the project.

## AgentZero-Specific Security Rules

- No tool, skill, package, ACP session, model provider, or runtime adapter may bypass policy evaluation.
- No remote model call may receive raw secrets.
- No remote model call may receive raw PII unless explicit policy allows it.
- Unknown data classification fails closed as private.
- Unknown package permission fails closed as denied.
- Unknown runtime safety fails closed as denied or requires MVM isolation.
- Unknown model destination fails closed as denied.
- Untrusted content must never become trusted instruction.
- No audit event may contain raw secrets.
- Offline mode must perform zero network calls.
- Package install scripts are denied by default.
- Native package execution is denied by default.
- MCP servers, browser automation, Python helpers, Node helpers, and native binaries require MVM or explicit ADR coverage.

## Architecture Discipline

When proposing or implementing changes:

- prefer project primitives over app-specific hacks
- prefer declared capabilities over ambient trust
- prefer explicit contracts over implicit behavior
- prefer additive changes over structural rewrites
- prefer observable infrastructure over hidden behavior
- prefer boring, testable implementations over magic
- keep plans, ADRs, and sprint state synchronized with code

## Definition of Done

No task is complete without tests and documentation alignment.

1. Tests cover changed behavior.
2. `cargo test --workspace` passes after Rust workspace exists.
3. `cargo clippy --workspace -- -D warnings` passes after Rust workspace exists.
4. `cargo check --workspace` passes after Rust workspace exists.
5. `just ci` passes.
6. `specs/SPRINT.md` reflects current status.
7. ADRs are updated if architecture, security, runtime behavior, APIs, or trust boundaries changed.
8. No TODOs, `unimplemented!`, panic stubs, or hidden bypasses exist in production paths.

## Worktree Workflow

Every feature, refactor, or non-trivial bug fix must be developed in a git worktree, never on the main checkout.

```bash
git worktree add ../agentzero-<feature-slug> -b feat/<feature-slug>
cd ../agentzero-<feature-slug>
```

## When to Stop and Escalate

Stop and request a design review if:

- a requested feature does not fit an existing extension point
- implementation would bypass an invariant
- a change affects policy, redaction, encryption, audit, data ownership, runtime isolation, ACP, package execution, model routing, or public APIs
- a change introduces a new compatibility surface
- a change requires a new ADR but none exists
