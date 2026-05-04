# Prompt: Implement `agentzero init --private`

You are working in AgentZero.

Read:

- `specs/project.md`
- `specs/security-model.md`
- `specs/adrs/0001-minimal-secure-core.md`
- `specs/adrs/0002-local-first-model-routing.md`
- `specs/adrs/0003-policy-redaction-and-audit-wrap-every-action.md`

## Task

Implement `agentzero init --private`.

It must create:

```text
.agentzero/
  settings.toml
  policy.yml
  models.json
  prompts/
  skills/
  audit/
  sessions/
  vault/
AGENTS.md if missing
```

## Default Private Policy

The generated `.agentzero/policy.yml` must express:

- autonomy: supervised
- network: deny by default
- shell: ask by default
- filesystem reads: current project allowed except sensitive paths
- filesystem writes: ask by default
- remote model calls: deny for secrets, redact or deny for PII
- audit: enabled
- offline mode compatible

## Tests

Add tests for:

- init creates expected files
- init is idempotent
- private policy denies network by default
- sensitive path patterns are present
- existing `AGENTS.md` is not overwritten

## Commands

```bash
cargo fmt
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
just ci
```
