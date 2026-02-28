# AgentZero

Learning-focused, lightweight clone inspired by ZeroClaw architecture.

## Goals
- Learn clean architecture boundaries with traits-first design.
- Build incrementally with tests and quality gates from day one.
- Keep scope intentionally small before expanding.

## Phase 0 + 1 Included
- CLI commands: `onboard`, `agent`, `status`
- Core traits: `Provider`, `MemoryStore`, `Tool`
- Implementations:
  - OpenAI-compatible provider (infra)
  - SQLite memory store (infra)
  - Turso memory store (`crates/agentzero-memory-turso`, optional backend)
  - `read_file` tool with workspace path allowlist (infra)
  - `shell` tool with command allowlist (infra)
  - WASM plugin container scaffolding (`crates/agentzero-plugins-wasm`)

## Run
```bash
cargo run -p agentzero -- status
cargo run -p agentzero -- onboard
cargo run -p agentzero -- gateway
cargo run -p agentzero -- agent -m "hello"
```

`onboard` now runs an interactive setup wizard and writes `agentzero.toml`.

## Config Loading
`agentzero-config` resolves configuration with this precedence (highest last):

1. Defaults in typed config structs.
2. `agentzero.toml`.
3. Dotenv files in project root:
`.env` -> `.env.local` -> `.env.<environment>` (for example `.env.development`).
4. Process environment variables (`AGENTZERO_*`, legacy keys like `AGENTZERO_MODEL`).

Environment selection for `.env.<environment>` uses:
`AGENTZERO_ENV`, then `APP_ENV`, then `NODE_ENV`.

`OPENAI_API_KEY` is resolved from process env first, then the dotenv chain.

Security note:
- `security.allowed_commands` may be empty only in `agent.mode = "development"` (or `"dev"`); non-dev modes reject empty allowlists.

## Commands
See `docs/COMMANDS.md` for detailed command usage, options, behavior, and troubleshooting.

## Next
See `docs/ROADMAP.md` and `docs/adr/0001-scope.md`.
