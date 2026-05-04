# Prompt: Implement Built-In `repo-security-audit` Skill

You are working in AgentZero.

Read:

- `skills/repo-security-audit/SKILL.md`
- `specs/adrs/0004-skills-as-first-class-capability-bundles.md`
- `specs/adrs/0008-prompt-injection-and-untrusted-content-boundaries.md`

## Task

Implement enough skill-loading behavior to support the built-in `repo-security-audit` skill.

## Requirements

- Load skill metadata progressively.
- Treat skill references and repo files as untrusted content unless explicitly trusted.
- Do not execute skill helper scripts yet.
- The skill can request read/list/search tools only.
- The skill must produce an audit report draft.
- The skill must never read denied sensitive files.

## Tests

Add tests for:

- skill metadata loads
- skill full instructions load only when selected
- denied files are not read
- suspicious instructions in repo content are treated as untrusted content
- audit report includes files scanned and blocked items

## Commands

```bash
cargo fmt
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
just ci
```
