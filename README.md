# AgentZero

AgentZero is a lightweight, Rust-first agent runtime and CLI inspired by OpenClaw, built incrementally with strong module boundaries, security-first defaults, and test-driven delivery.

## Why this project

- Learn how to build an agent platform with clean crate boundaries.
- Keep the runtime minimal and understandable.
- Prioritize safety: scoped tools, allowlists, auditability, and documented threat model.

## Workspace layout

- `bin/agentzero`: thin binary entrypoint (`bins/cli.rs`) that calls `agentzero_cli::run()`
- `crates/agentzero-cli`: CLI command parsing, dispatch, UX, and command implementations
- `crates/agentzero-core`: core agent traits, orchestration, and shared domain types
- `crates/agentzero-config`: typed config model, loader, and policy validation
- `crates/agentzero-infra`: infrastructure adapters and tool wiring (currently transitional)
- `crates/agentzero-memory-sqlite`: SQLite memory backend
- `crates/agentzero-memory-turso`: Turso/libsql memory backend (feature-gated)
- `crates/agentzero-providers`: provider catalog and OpenAI-compatible provider implementation
- `crates/agentzero-tools`: filesystem/shell tool implementations
- `crates/agentzero-gateway`: HTTP gateway service
- `crates/agentzero-plugins-wasm`: WASM plugin runtime scaffolding
- `crates/agentzero-security`: shared security policy + redaction utilities
- `crates/agentzero-bench`: benchmark harness
- `crates/agentzero-common`: common helpers/types

## Quick start

### 1. Prerequisites

- Rust 1.80+
- Cargo

### 2. Build

```bash
cargo build
```

### 3. Run onboarding

```bash
cargo run -p agentzero -- onboard
```

This launches an interactive setup flow and writes `agentzero.toml`.

### 4. Set your API key

```bash
export OPENAI_API_KEY="<your-key>"
```

### 5. Run commands

```bash
cargo run -p agentzero -- status
cargo run -p agentzero -- doctor
cargo run -p agentzero -- providers
cargo run -p agentzero -- auth list
cargo run -p agentzero -- agent -m "hello"
cargo run -p agentzero -- gateway
```

## CLI overview

Current primary commands:

- `agentzero onboard`
- `agentzero status`
- `agentzero doctor`
- `agentzero providers`
- `agentzero auth ...`
- `agentzero agent -m "..."`
- `agentzero gateway`

For full reference, examples, output modes, and troubleshooting:

- `docs/COMMANDS.md`

## Onboard flags + env vars

`onboard` supports explicit typed option overrides with precedence:

`flag > env > default`

Supported inputs:

- `--provider` or `AGENTZERO_PROVIDER`
- `--base-url` or `AGENTZERO_BASE_URL`
- `--model` or `AGENTZERO_MODEL`
- `--memory-path` or `AGENTZERO_MEMORY_PATH`
- `--allowed-root` or `AGENTZERO_ALLOWED_ROOT`
- `--allowed-commands` or `AGENTZERO_ALLOWED_COMMANDS`

Example:

```bash
export AGENTZERO_PROVIDER=openrouter
export AGENTZERO_MODEL=anthropic/claude-3.5-sonnet
export AGENTZERO_ALLOWED_COMMANDS=ls,pwd,cat,echo
cargo run -p agentzero -- onboard
```

## Config

Default config file: `agentzero.toml`.

Config is resolved from typed defaults, file, dotenv/environment layers, and command options. See:

- `crates/agentzero-config`
- `docs/COMMANDS.md`

## Memory backends

### SQLite (default)

No extra feature flag required.

### Turso (optional)

Build with Turso support:

```bash
cargo run -p agentzero --features memory-turso -- status
```

At runtime, set:

- `AGENTZERO_MEMORY_BACKEND=turso`
- `TURSO_DATABASE_URL`
- `TURSO_AUTH_TOKEN`

## Security posture

- Tool access is scoped and allowlist-driven.
- Redaction utilities are centralized in `agentzero-security`.
- Security requirements and threat model are tracked in:
  - `docs/security/THREAT_MODEL.md`
  - `docs/security/DEPENDENCY_POLICY.md`

## Quality gates

Recommended local checks:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p agentzero --release
scripts/check-binary-size.sh --binary target/release/agentzero --max-bytes 20000000
scripts/verify-release-version.sh --version 0.1.0
# requires cargo-llvm-cov
scripts/run-coverage.sh --output-dir coverage
# requires cargo-audit + cargo-deny
scripts/run-security-audits.sh
scripts/verify-dependency-policy.sh
```

## Project planning

- Sprint execution plan: `specs/SPRINT.md`
- Architecture scope and constraints: `docs/adr/0001-scope.md`
- High-level roadmap: `docs/ROADMAP.md`

## Status

This is an active learning/build project. Interfaces and crate boundaries are evolving toward a full modular clone architecture, including security, gateway, plugins, hooks, skills, tunnel, auth, cron, doctor, and RAG tracks.
