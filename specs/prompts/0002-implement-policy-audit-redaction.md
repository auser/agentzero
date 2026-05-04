# Prompt: Implement Policy, Audit, and Redaction Interfaces

You are working in AgentZero.

Read the project docs and ADRs first, especially:

- `specs/security-model.md`
- `specs/adrs/0002-local-first-model-routing.md`
- `specs/adrs/0003-policy-redaction-and-audit-wrap-every-action.md`
- `specs/adrs/0008-prompt-injection-and-untrusted-content-boundaries.md`
- `specs/adrs/0009-capability-based-secret-handles.md`

## Task

Implement the first typed interfaces for:

- data classification
- action kinds
- policy decision
- approval scope
- audit event
- redaction result
- secret scan result
- model routing decision

Do not implement model providers or tool execution yet.

## Requirements

- Everything must fail closed by default.
- No raw secret field should be printable by default.
- Audit events must be serializable.
- Policy decisions must include a reason.
- Redaction results must support token-preserving placeholders.
- Tests must cover allow, deny, redacted, and blocked decisions.
- No `unwrap`, `expect`, `panic`, `todo`, or `unimplemented` in production paths.

## Commands

```bash
cargo fmt
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
just ci
```
