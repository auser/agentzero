# Command Reference

This project currently exposes a small command surface:

- `agentzero onboard`
- `agentzero gateway`
- `agentzero status`
- `agentzero doctor`
- `agentzero agent -m "<message>"`

You can always inspect generated help:

```bash
cargo run -p agentzero -- --help
cargo run -p agentzero -- <command> --help
```

## Global Notes

- Binary name: `agentzero`
- Package name: `agentzero-cli`
- Default persistence: SQLite database at `./agentzero.db`
- API key source: `OPENAI_API_KEY`
- Memory backend selector: `AGENTZERO_MEMORY_BACKEND` (`sqlite` or `turso`)
- Global config override: `--config <path>` (highest precedence for config file path)
- Global verbosity:
- `-v` (or `--verbose`) -> `RUST_LOG=error`
- `-vv` -> `RUST_LOG=info`
- `-vvv` -> `RUST_LOG=debug`
- `-vvvv` (or higher) -> `RUST_LOG=trace`
- Numeric form: `--verbose 1..4` (for example, `--verbose 4` => trace)

### Exit Codes

- `0`: success (including `--help` / `--version`)
- `1`: runtime or execution failure
- `2`: CLI usage/argument parsing error

## `onboard`

Creates a starter config file (`agentzero.toml`) in the current directory.

### Usage

```bash
cargo run -p agentzero -- onboard
cargo run -p agentzero -- onboard --yes
cargo run -p agentzero -- onboard --provider openrouter --model anthropic/claude-3.5-sonnet
```

### Behavior

- In TTY mode, launches a colorful interactive onboarding wizard with:
- A branded header.
- Searchable prompt selection for provider, base URL, model, memory path, and security fields (type-to-filter with `custom` fallback).
- Section-by-section setup (provider, memory, security) with checkmark progress.
- Prompts for provider, memory, and security values (Enter accepts defaults).
- Writes `agentzero.toml` with the selected values.
- Writes global tool security policy sections under `[security.*]`.
- If config already exists, asks for overwrite confirmation.
- `--yes` skips prompts and uses non-interactive defaults/auto-accept behavior.
- Supports explicit flags and env-var fallback with precedence: `flag > env > default`.
- Prints the next command to run.

### Flags and Env Vars

- `--provider` or `AGENTZERO_PROVIDER`
- `--base-url` or `AGENTZERO_BASE_URL`
- `--model` or `AGENTZERO_MODEL`
- `--memory-path` or `AGENTZERO_MEMORY_PATH`
- `--allowed-root` or `AGENTZERO_ALLOWED_ROOT`
- `--allowed-commands` or `AGENTZERO_ALLOWED_COMMANDS`

### Examples

```bash
export AGENTZERO_PROVIDER=openrouter
export AGENTZERO_MODEL=anthropic/claude-3.5-sonnet
export AGENTZERO_ALLOWED_COMMANDS=ls,pwd,cat,echo
cargo run -p agentzero -- onboard

cargo run -p agentzero -- onboard \
  --provider anthropic \
  --model claude-3-5-sonnet-latest \
  --allowed-commands ls,pwd,cat
```

### Example Output

```text
AgentZero onboarding
Press Enter to accept defaults.
Provider base URL [https://api.openai.com]:
Provider model [gpt-4o-mini]:
Memory db path [./agentzero.db]:
Security allowed root [.]:
Allowed shell commands [ls,pwd,cat,echo]:
Created agentzero.toml
Set OPENAI_API_KEY and run: cargo run -p agentzero -- agent -m "hello"
```

## `status`

Shows a minimal runtime status summary.

### Usage

```bash
cargo run -p agentzero -- status
cargo run -p agentzero -- --config ./agentzero.toml status
cargo run -p agentzero -- status --json
```

### Behavior

- Uses the configured memory backend (`sqlite` by default).
- For SQLite, opens/creates `agentzero.db`.
- Reads the most recent memory entries.
- Prints number of recent entries (up to 5).
- `--json` emits machine-readable output.

### Examples

```bash
# Human-readable status
cargo run -p agentzero -- status

# Machine-readable status
cargo run -p agentzero -- status --json

# Status against explicit config path
cargo run -p agentzero -- --config ./agentzero.toml status
```

## `gateway`

Starts the HTTP gateway server.

### Usage

```bash
cargo run -p agentzero -- gateway
cargo run -p agentzero -- gateway --host 0.0.0.0 --port 8081
```

### Behavior

- Binds to `127.0.0.1:8080` by default.
- Exposes:
  - `GET /health` (service health probe)
  - `POST /v1/ping` (echo test endpoint)

### Startup Output (What You Are Seeing)

Depending on the runtime mode/build, startup output may include:

- A line showing the bound listen address, for example:
  - `Gateway listening on http://127.0.0.1:42617`
- A "dashboard" URL line pointing to the same host/port.
- A route summary showing available endpoints, for example:
  - `POST /pair` (pair a client with one-time code header)
  - `POST /webhook`
  - `POST /api/chat`
  - `POST /v1/chat/completions`
  - `GET /v1/models`
  - `GET /api/*`
  - `GET /ws/chat`
  - `GET /health`
  - `GET /metrics`
- A pairing banner with a one-time code and instruction to call:
  - `POST /pair` with header `X-Pairing-Code: <code>`
- A final `Press Ctrl+C to stop.` line.

If pairing is shown, the gateway is waiting for a client enrollment call before normal authenticated API usage.

### Examples

```bash
# Health probe
curl -s http://127.0.0.1:8080/health

# Ping endpoint
curl -s -X POST http://127.0.0.1:8080/v1/ping -H 'content-type: application/json' -d '{"message":"hi"}'

# Pairing endpoint
curl -s -X POST http://127.0.0.1:42617/pair \
  -H 'X-Pairing-Code: 406823'
```

## `doctor`

Runs local diagnostics to verify config integrity, workspace readiness, daemon state, environment variables, and common CLI tooling.

### Usage

```bash
cargo run -p agentzero -- doctor
```

### Behavior

- Prints sectioned checks for:
- `config` (config file/load/provider/model/API key/memory backend)
- `workspace` (existence/writability/core files/disk-space probe)
- `daemon` (state-file presence check)
- `environment` (git + shell/home env)
- `cli-tools` (git, python3, node, npm, cargo, rustc versions)
- Shows a summary count of `ok`, `warnings`, and `errors`.

### Examples

```bash
# Standard diagnostics
cargo run -p agentzero -- doctor

# Diagnostics with explicit config path
cargo run -p agentzero -- --config ./agentzero.toml doctor
```

## `agent`

Sends one user message through the minimal agent loop and prints the assistant response.

### Usage

```bash
cargo run -p agentzero -- agent -m "hello"
```

### Examples

```bash
# Basic single-turn chat
cargo run -p agentzero -- agent -m "hello"

# Tool invocation shortcut
cargo run -p agentzero -- agent -m "tool:shell pwd"

# Use explicit config path
cargo run -p agentzero -- --config ./agentzero.toml agent -m "summarize README.md"
```

### Required Flags

- `-m, --message <MESSAGE>`: the prompt to send.

### Behavior

- Requires `OPENAI_API_KEY`.
- Uses OpenAI-compatible endpoint `https://api.openai.com/v1/chat/completions`.
- Uses default model `gpt-4o-mini`.
- Stores user + assistant turns in configured memory backend.
- Initializes tools:
  - `read_file` (restricted by `[security]` + `[security.read_file]` in `agentzero.toml`)
  - `write_file` (disabled by default; enable via `[security.write_file].enabled = true`)
  - `shell` (restricted by `[security]` + `[security.shell]` in `agentzero.toml`)
- `mcp` (enabled only when `[security.mcp].enabled = true` and allowlisted servers are configured)
- `plugin_exec` (enabled only when `[security.plugin].enabled = true`)
- Audit trail (`[security.audit]`) records step-by-step execution events when enabled.
- Runtime metrics are collected for request counters and latency histograms and emitted as a lightweight log snapshot.

### Tool Invocation Shortcut

The current prototype supports a simple inline tool syntax:

```text
tool:<tool_name> <tool_input>
```

Example:

```bash
cargo run -p agentzero -- agent -m "tool:shell pwd"
```

MCP example (requires server allowlist config):

```bash
export AGENTZERO_MCP_SERVERS='{"filesystem":{"command":"npx","args":["-y","@modelcontextprotocol/server-filesystem","/absolute/workspace/path"]}}'
cargo run -p agentzero -- agent -m 'tool:mcp {"server":"filesystem","tool":"read_file","arguments":{"path":"README.md"}}'
```

Required config:

```toml
[security.mcp]
enabled = true
allowed_servers = ["filesystem"]
```

Process-plugin tool example (optional):

```bash
export AGENTZERO_PLUGIN_TOOL='{"command":"cat","args":[]}'
cargo run -p agentzero -- agent -m 'tool:plugin_exec hello'
```

Required config:

```toml
[security.plugin]
enabled = true
```

Write-file tool example (optional, strict mode):

```bash
cargo run -p agentzero -- agent -m 'tool:write_file {"path":"notes/out.txt","content":"hello","overwrite":false,"dry_run":true}'
```

Required config:

```toml
[security.write_file]
enabled = true
max_write_bytes = 65536
```

Audit logging:

```toml
[security.audit]
enabled = true
path = "./agentzero-audit.log"
```

## Turso Backend (Optional)

Enable Turso support at build time:

```bash
cargo run -p agentzero --features memory-turso -- status
```

Use Turso at runtime:

```bash
export AGENTZERO_MEMORY_BACKEND=turso
export TURSO_DATABASE_URL="libsql://<your-db>.turso.io"
export TURSO_AUTH_TOKEN="<your-token>"
cargo run -p agentzero --features memory-turso -- agent -m "hello from turso"
```

## Troubleshooting

| Symptom | Likely Cause | Fix |
|---|---|---|
| `missing OPENAI_API_KEY` when running `agent` | API key is not set in environment or dotenv chain | `export OPENAI_API_KEY="your_key_here"` and rerun |
| `config file not found` in `doctor` output | `agentzero.toml` is missing at the resolved path | Run `cargo run -p agentzero -- onboard` or pass `--config <path>` |
| `config load failed` in `doctor` output | Invalid or incomplete TOML config | Fix config fields (provider URL/model, memory backend/path), then rerun `doctor` |
| `state file not found ... daemon_state.json` in `doctor` output | Daemon runtime is not started yet | Start daemon/service flow when available; for local dev this can be expected |
| `directory is not writable` in `doctor` output | Workspace path is read-only or has permission issues | Fix filesystem permissions or run in a writable workspace |
| `status --json` works but `status` fails to open memory backend | Memory backend env/config mismatch | Verify `AGENTZERO_MEMORY_BACKEND` and backend-specific env vars (e.g. Turso URL/token) |
| `onboard` does not overwrite existing config | Existing `agentzero.toml` and overwrite not confirmed | Re-run interactively and confirm overwrite, or remove/rename existing config first |
| Tool invocation fails with policy errors | Security policy defaults are fail-closed | Update `[security.*]` sections in `agentzero.toml` (allowlist root/commands, enable optional tools) |

Quick checks:

```bash
cargo run -p agentzero -- doctor
cargo run -p agentzero -- --config ./agentzero.toml status --json
```
