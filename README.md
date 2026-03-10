# AgentZero

AgentZero is a lightweight, Rust-first agent runtime and CLI built incrementally with strong module boundaries, security-first defaults, and test-driven delivery.

## Why this project

- Learn how to build an agent platform with clean crate boundaries.
- Keep the runtime minimal and understandable.
- Prioritize safety: scoped tools, allowlists, auditability, and documented threat model.

## Workspace layout (16 crates)

- `bin/agentzero`: thin binary entrypoint (`bins/cli.rs`) that calls `agentzero_cli::run()`
- `crates/agentzero-core`: core agent traits, orchestration, shared domain types, security policy, routing, delegation
- `crates/agentzero-config`: typed config model, loader, and policy validation
- `crates/agentzero-storage`: encrypted persistence (XChaCha20Poly1305 JSON stores, SQLCipher memory, queues, key management)
- `crates/agentzero-providers`: provider catalog and OpenAI-compatible provider implementation
- `crates/agentzero-auth`: authentication profile management
- `crates/agentzero-tools`: all tool implementations (filesystem, shell, web, media, cron, autonomy, hardware, skills)
- `crates/agentzero-infra`: infrastructure adapters, runtime orchestration, and tool wiring
- `crates/agentzero-channels`: channel backends (Nostr, iMessage) and leak-guard
- `crates/agentzero-plugins`: plugin lifecycle, WASM runtime (wasmi default, wasmtime via `wasm-jit`)
- `crates/agentzero-plugin-sdk`: WASM plugin SDK for third-party plugin authors
- `crates/agentzero-gateway`: HTTP gateway service
- `crates/agentzero-ffi`: C FFI bindings
- `crates/agentzero-cli`: CLI command parsing, dispatch, UX, and all command implementations
- `crates/agentzero-testkit`: shared test utilities
- `crates/agentzero-bench`: criterion benchmark suite

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

No extra feature flag required. Conversation history is encrypted at rest using SQLCipher (AES-256-CBC). The encryption key is automatically generated and stored at `~/.agentzero/.agentzero-data.key`, or can be provided via the `AGENTZERO_DATA_KEY` environment variable. Existing plaintext databases are automatically migrated to encrypted format on first use.

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
- All persisted state uses encrypted storage (XChaCha20Poly1305 for JSON stores, SQLCipher for SQLite).
- WASM plugins run in a sandboxed runtime with fuel metering, memory limits, and host-call allowlists.
- Security requirements and threat model are tracked in:
  - `docs/security/THREAT_MODEL.md` (detailed, per-threat entries with status and tests)
  - `site/src/content/docs/security/threat-model.md` (user-facing documentation site)
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
