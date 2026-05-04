# AgentZero

A documentation-first starter for building AgentZero: a local-first secure AI agent harness for private developer workflows.

This starter is designed to be copied into a new repository created from `auser/project-template`, or used directly as the seed for a new repo.

## Project Promise

AgentZero lets local AI agents work with private files, code, tools, secrets-adjacent workflows, and PII-bearing data without bypassing policy, redaction, audit, or runtime isolation.

## First Demo

```bash
agentzero init --private
agentzero chat --local
```

Then ask:

```text
Audit this repo for leaked secrets, PII exposure, unsafe AI calls, and suspicious agent/package instructions.
```

AgentZero should:

- read and search the repo locally
- classify data sensitivity
- detect secrets and PII
- block unsafe remote model calls
- treat untrusted content as untrusted content
- propose safe patches
- produce a redacted audit report

## Contents

- `AGENTS.md` — mandatory rules for agents and developers
- `specs/project.md` — project goals, invariants, scope, non-goals, guarantees
- `specs/security-model.md` — core safety model
- `specs/SPRINT.md` — first active sprint
- `specs/adrs/` — initial accepted ADR suite
- `specs/plans/` — bootstrap and implementation plans
- `specs/prompts/` — copy-paste prompts for Claude Code/Codex
- `skills/repo-security-audit/` — first built-in skill draft
- `Justfile` — docs and future Rust workflow commands

## Suggested Usage

```bash
cd /Users/auser/work/rust/mine
git clone https://github.com/auser/project-template agentzero
cd agentzero
just init-project PROJECT_NAME="AgentZero"

# Copy this starter over the template output.
cp -R /path/to/agentzero-starter/* .
just ci

git add .
git commit -m "docs: define AgentZero architecture foundation"
```

Then open the repo in Claude Code/Codex and start with:

```text
specs/prompts/0001-bootstrap-rust-workspace.md
```
