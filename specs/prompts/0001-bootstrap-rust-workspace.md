# Prompt: Bootstrap AgentZero Rust Workspace

You are working in the AgentZero repository.

Before making changes, read:

1. `AGENTS.md`
2. `specs/project.md`
3. `specs/security-model.md`
4. `specs/SPRINT.md`
5. all files in `specs/adrs/`
6. `specs/plans/0001-bootstrap-agentzero.md`

Your task is to create the initial Rust workspace skeleton only. Do not implement business logic yet.

## Requirements

- Follow every accepted ADR.
- Preserve the minimal secure core boundary.
- Add crates:
  - `agentzero-cli`
  - `agentzero`
  - `agentzero-policy`
  - `agentzero-audit`
  - `agentzero-redaction`
  - `agentzero-models`
  - `agentzero-tools`
  - `agentzero-config`
- Add a root `Cargo.toml` workspace.
- Add minimal `lib.rs` / `main.rs` files that compile.
- Add command skeletons for:
  - `agentzero init`
  - `agentzero chat`
  - `agentzero run`
  - `agentzero doctor`
  - `agentzero policy`
  - `agentzero audit`
  - `agentzero vault`
- Do not add MVM, WASM, ACP, MCP, swarms, SDKs, gateway, marketplace, or package installation yet.
- Do not add TODOs, stubs that panic, or unimplemented production paths.
- Add basic tests proving the CLI parses the top-level commands.
- Update `specs/SPRINT.md` when done.

## Required Commands

Run:

```bash
cargo fmt
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
just ci
```

## Output

Summarize:

- files created
- crates created
- tests added
- ADRs followed
- commands run and results
