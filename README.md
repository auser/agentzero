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
- `crates/agentzero-memory`: unified memory backend crate (SQLite default + Turso/libsql via feature)
- `crates/agentzero-providers`: provider catalog and OpenAI-compatible provider implementation
- `crates/agentzero-tools`: filesystem/shell tool implementations
- `crates/agentzero-gateway`: HTTP gateway service
- `crates/agentzero-daemon`: daemon runtime state + lifecycle
- `crates/agentzero-service`: service lifecycle state and operations
- `crates/agentzero-health`: shared health/freshness assessment utilities
- `crates/agentzero-heartbeat`: encrypted heartbeat persistence for runtime components
- `crates/agentzero-doctor`: diagnostics collection/report model for doctor command
- `crates/agentzero-cron`: scheduled task state and lifecycle operations
- `crates/agentzero-hooks`: hook state and control operations
- `crates/agentzero-cost`: cost tracking domain primitives
- `crates/agentzero-coordination`: runtime coordination domain primitives
- `crates/agentzero-goals`: goals domain primitives
- `crates/agentzero-plugins`: plugin lifecycle + WASM runtime scaffolding
- `crates/agentzero-skills`: skills lifecycle with embedded skillforge + SOP functionality
- `crates/agentzero-rag`: local retrieval index + ingest/query primitives
- `crates/agentzero-multimodal`: shared media-kind inference primitives
- `crates/agentzero-hardware`: hardware discovery/introspection primitives
- `crates/agentzero-peripherals`: peripheral registry lifecycle primitives
- `crates/agentzero-security`: shared security policy + redaction utilities
- `crates/agentzero-update`: migration + self-update flows and state model
- `crates/agentzero-bench`: criterion benchmark suite
- `crates/agentzero-common`: common helpers/types

## Plugin packaging and install integrity

`crates/agentzero-plugins` now includes a packaging/install pipeline with built-in integrity checks:

- `package::package_plugin(...)` packages `manifest.json` + `.wasm` into a plugin archive.
- Packaging always regenerates and writes a SHA-256 checksum for the wasm module.
- `package::install_packaged_plugin(...)` validates manifest shape and verifies checksum before install.
- Tampered packages fail closed on install (checksum mismatch).

WASM runtime sandbox controls are also enforced in the same crate:

- execution timeout limits
- memory limits
- host-call import allowlist validation

## Quick start

### 1. Install

```bash
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash
```

Or with options:

```bash
# Install specific version to custom directory
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- \
  --version 0.1.0 --dir /usr/local/bin

# Install with shell completions
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --completions zsh

# Build from source (requires Rust 1.80+)
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash -s -- --from-source
```

Run `install.sh --help` for all options.

### 2. Run onboarding

```bash
agentzero onboard
```

This launches an interactive setup flow and writes `agentzero.toml`.

### 3. Set your API key

```bash
export OPENAI_API_KEY="<your-key>"
```

### 4. Run commands

```bash
agentzero status
agentzero doctor
agentzero providers
agentzero auth list
agentzero agent -m "hello"
agentzero gateway
```

### Build from source (development)

```bash
git clone https://github.com/auser/agentzero.git
cd agentzero
cargo build --release
cargo run -p agentzero -- --help
```

## CLI overview

Current primary commands:

- `agentzero onboard`
- `agentzero status`
- `agentzero doctor`
- `agentzero providers`
- `agentzero auth ...`
- `agentzero cron ...`
- `agentzero hooks ...`
- `agentzero daemon ...`
- `agentzero service ...`
- `agentzero tunnel ...`
- `agentzero plugin ...`
- `agentzero migrate ...`
- `agentzero update ...`
- `agentzero rag ...` (feature-gated)
- `agentzero hardware ...` (feature-gated)
- `agentzero peripheral ...` (feature-gated)
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
