---
title: Command Reference
description: Complete reference for all agentzero CLI commands, flags, and usage examples.
---

This project currently exposes a small command surface:

- `agentzero onboard`
- `agentzero gateway`
- `agentzero daemon ...`
- `agentzero status`
- `agentzero dashboard`
- `agentzero doctor`
- `agentzero service ...`
- `agentzero providers`
- `agentzero approval ...`
- `agentzero identity ...`
- `agentzero coordination ...`
- `agentzero cost ...`
- `agentzero goals ...`
- `agentzero estop ...`
- `agentzero channel ...`
- `agentzero integrations ...`
- `agentzero models`
- `agentzero auth ...`
- `agentzero cron ...`
- `agentzero hooks ...`
- `agentzero skill ...`
- `agentzero tunnel ...`
- `agentzero plugin ...`
- `agentzero migrate ...`
- `agentzero update ...`
- `agentzero config ...`
- `agentzero memory ...`
- `agentzero completions --shell <...>`
- `agentzero rag ...`
- `agentzero hardware ...` (feature-gated)
- `agentzero peripheral ...` (feature-gated)
- `agentzero agent -m "<message>"`

You can always inspect generated help:

```bash
cargo run -p agentzero -- --help
cargo run -p agentzero -- <command> --help
```

## Global Notes

- Binary name: `agentzero`
- Package name: `agentzero-cli`
- Default persistence: SQLite database at `~/.agentzero/agentzero.db` (unless overridden in config)
- API key source: `OPENAI_API_KEY`
- Memory backend selector: `AGENTZERO_MEMORY_BACKEND` (`sqlite` or `turso`)
- Global config override: `--config <path>` (highest precedence for config file path)
- Global data dir override: `--data-dir <path>` (alias: `--config-dir`) or `AGENTZERO_DATA_DIR`
- Data dir precedence: `--data-dir` > `AGENTZERO_DATA_DIR` > `data_dir` in config > default `~/.agentzero`
- Global verbosity:
- `-v` (or `--verbose`) -> `RUST_LOG=error`
- `-vv` -> `RUST_LOG=info`
- `-vvv` -> `RUST_LOG=debug`
- `-vvvv` (or higher) -> `RUST_LOG=trace`
- Numeric form: `--verbose 1..4` (for example, `--verbose 4` => trace)
- Global JSON mode: `--json` wraps any command output into a structured JSON object:
- `{ "ok": <bool>, "command": "<name>", "result": { ... }, "error": "<message?>" }`

### Exit Codes

- `0`: success (including `--help` / `--version`)
- `1`: runtime or execution failure
- `2`: CLI usage/argument parsing error

## `onboard`

Creates a starter config file (`agentzero.toml`) in the active data directory (default: `~/.agentzero`).

### Usage

```bash
cargo run -p agentzero -- onboard
cargo run -p agentzero -- onboard --interactive
cargo run -p agentzero -- onboard --force --provider openrouter --model anthropic/claude-3.5-sonnet
```

### Behavior

- Default mode is quick setup (non-interactive seed/write flow).
- `--interactive` launches the full onboarding wizard.
- If config already exists, asks for overwrite confirmation unless `--force` is provided.
- Supports explicit flags and env-var fallback with precedence: `flag > env > default`.

## `status`

Shows a minimal runtime status summary.

### Usage

```bash
cargo run -p agentzero -- status
cargo run -p agentzero -- --json status
```

## `gateway`

Starts the HTTP gateway server.

### Usage

```bash
cargo run -p agentzero -- gateway
cargo run -p agentzero -- gateway --host 0.0.0.0 --port 8081
```

### Endpoints

- `GET /health` (service health probe)
- `POST /v1/ping` (echo test endpoint)
- `POST /v1/webhook/:channel` (channel dispatch)

## `agent`

Sends one user message through the minimal agent loop and prints the assistant response.

### Usage

```bash
cargo run -p agentzero -- agent -m "hello"
```

### Required Flags

- `-m, --message <MESSAGE>`: the prompt to send.

## `doctor`

Runs targeted diagnostics and trace inspection.

### Usage

```bash
cargo run -p agentzero -- doctor models
cargo run -p agentzero -- doctor traces
cargo run -p agentzero -- doctor traces --event tool --contains shell --limit 50
```

## Troubleshooting

| Symptom | Likely Cause | Fix |
|---|---|---|
| `missing OPENAI_API_KEY` when running `agent` | API key is not set | `export OPENAI_API_KEY="your_key_here"` |
| `config file not found` in `doctor` output | `agentzero.toml` is missing | Run `cargo run -p agentzero -- onboard` |
| Tool invocation fails with policy errors | Security policy defaults are fail-closed | Update `[security.*]` sections in `agentzero.toml` |

Quick checks:

```bash
cargo run -p agentzero -- doctor
cargo run -p agentzero -- --config ./agentzero.toml status
```
