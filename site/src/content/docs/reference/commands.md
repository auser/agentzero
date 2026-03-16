---
title: CLI Commands
description: Complete reference for all agentzero CLI commands, subcommands, and flags.
---

## Global Options

```bash
agentzero [OPTIONS] <COMMAND>
```

| Flag | Description |
|---|---|
| `--data-dir <PATH>` | Override data/config directory (alias: `--config-dir`) |
| `--config <PATH>` | Override config file path |
| `-v, --verbose` | Increase verbosity: `-v`=error, `-vv`=info, `-vvv`=debug, `-vvvv`=trace |
| `--json` | Emit structured JSON output for any command |

### Exit Codes

| Code | Meaning |
|---|---|
| `0` | Success (including `--help`, `--version`) |
| `1` | Runtime or execution failure |
| `2` | CLI usage or argument parsing error |

---

## Core Commands

### `agent`

Send a single message through the agent loop. The agent processes your message, calls tools as needed, and returns a response.

```bash
agentzero agent -m "hello"
agentzero agent -m "List files in the current directory"
agentzero agent -m "Summarize this repo" --provider openai --model gpt-4o-mini
agentzero agent -m "hello" --profile my-anthropic-profile
agentzero agent -m "explain this function" --stream
```

| Flag | Description |
|---|---|
| `-m, --message <TEXT>` | **(Required)** Message to send |
| `-p, --provider <NAME>` | Override the configured provider (e.g., `openrouter`, `openai`, `ollama`) |
| `--model <ID>` | Override the configured model (e.g., `gpt-4o-mini`, `llama3.1:8b`) |
| `--profile <NAME>` | Use a specific auth profile by name (from `auth list`) |
| `--stream` | Stream tokens incrementally as they arrive |

### `agents`

Manage persistent named agents. Agents created here are stored in an encrypted JSON store and can be hot-loaded into a running coordinator. Keywords enable automatic routing via the `AgentRouter`.

#### `agents create`

```bash
agentzero agents create --name Aria --description "Travel planner" \
  --model claude-sonnet-4-20250514 --provider anthropic --keywords travel,booking
agentzero agents create --name Coder --model gpt-4o --allowed-tools shell,read_file,write_file --json
```

| Flag | Description |
|---|---|
| `--name <NAME>` | **(Required)** Agent name |
| `--description <TEXT>` | What this agent does |
| `--model <ID>` | Model identifier |
| `--provider <NAME>` | Provider (anthropic, openai, openrouter, etc.) |
| `--system-prompt <TEXT>` | System prompt / persona |
| `--keywords <LIST>` | Comma-separated routing keywords |
| `--allowed-tools <LIST>` | Comma-separated tool allowlist (empty = all) |
| `--json` | Emit JSON output |

#### `agents list`

```bash
agentzero agents list
agentzero agents list --json
```

#### `agents get`

```bash
agentzero agents get --id agent_abc123
agentzero agents get --id agent_abc123 --json
```

#### `agents update`

```bash
agentzero agents update --id agent_abc123 --name "New Name" --model gpt-4o
agentzero agents update --id agent_abc123 --keywords travel,flights,hotels
```

| Flag | Description |
|---|---|
| `--id <ID>` | **(Required)** Agent ID |
| `--name`, `--description`, `--model`, `--provider`, `--system-prompt` | Fields to update |
| `--keywords <LIST>` | Replace keywords (comma-separated) |
| `--allowed-tools <LIST>` | Replace tool allowlist (comma-separated) |
| `--json` | Emit JSON output |

#### `agents delete`

```bash
agentzero agents delete --id agent_abc123
```

#### `agents status`

```bash
agentzero agents status --id agent_abc123 --active
agentzero agents status --id agent_abc123 --stopped
```

| Flag | Description |
|---|---|
| `--id <ID>` | **(Required)** Agent ID |
| `--active` | Set agent to active |
| `--stopped` | Set agent to stopped |

---

### `onboard`

Generate a starter `agentzero.toml` config file in the current directory. The interactive wizard walks you through provider, model, memory, and security settings.

```bash
agentzero onboard                               # Quick setup (uses defaults or env vars)
agentzero onboard --interactive                  # Full wizard with prompts
agentzero onboard --force --provider openrouter --model anthropic/claude-sonnet-4-6 --yes
```

| Flag | Description |
|---|---|
| `--interactive` | Launch full onboarding wizard |
| `--force` | Overwrite existing config without confirmation |
| `--provider <NAME>` | Provider name (openai, openrouter, anthropic, ollama, etc.) |
| `--base-url <URL>` | Provider base URL |
| `--model <ID>` | Model identifier |
| `--memory <BACKEND>` | Memory backend: `sqlite` (default), `lucid`, `markdown`, `none` |
| `--memory-path <PATH>` | Database file path |
| `--allowed-root <PATH>` | Filesystem scope root |
| `--allowed-commands <LIST>` | Comma-separated command allowlist |
| `--api-key <KEY>` | API key (quick mode only, ignored with `--interactive`) |
| `--yes` | Skip prompts, auto-accept defaults |
| `--no-totp` | Disable OTP in quick setup (not recommended) |
| `--channels-only` | Reconfigure channels only (fast repair) |

### `status`

Show a minimal runtime status summary including recent memory count.

```bash
agentzero status
agentzero --json status
```

### `doctor`

Run diagnostics for model availability and trace inspection.

#### `doctor models`

Probe model catalogs across all configured providers. Shows provider status, model count, and cache freshness.

```bash
agentzero doctor models                          # Probe all known providers
agentzero doctor models --provider openrouter    # Specific provider only
agentzero doctor models --use-cache              # Prefer cached catalogs
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | Probe specific provider only |
| `--use-cache` | Prefer cached catalogs when available |

#### `doctor traces`

List recent tool execution traces and model replies in reverse chronological order. Reads from `{data_dir}/trace_events.jsonl`.

```bash
agentzero doctor traces                          # Last 20 events
agentzero doctor traces --event tool --contains shell --limit 50
agentzero doctor traces --id abc123              # Show specific event
```

| Flag | Description |
|---|---|
| `--id <ID>` | Show specific trace event by id |
| `--event <TYPE>` | Filter by event type |
| `--contains <TEXT>` | Case-insensitive text match across message/payload |
| `--limit <N>` | Maximum events to display (default: 20) |

### `providers`

List all supported AI providers with their status and configuration.

```bash
agentzero providers                              # Table output
agentzero providers --json                       # Machine-readable
agentzero providers --no-color                   # Disable ANSI colors
```

| Flag | Description |
|---|---|
| `--json` | Emit machine-readable JSON output |
| `--no-color` | Disable ANSI color in table output |

### `providers-quota`

Inspect provider rate limits, API key status, and circuit breaker state.

```bash
agentzero providers-quota
agentzero providers-quota --provider openrouter --json
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | Specific provider to inspect |
| `--json` | Emit machine-readable JSON output |

---

## Gateway & Operations

### `gateway`

Start the HTTP gateway server in the foreground. Blocks until stopped. Exposes REST, WebSocket, and webhook endpoints for programmatic access.

```bash
agentzero gateway                                # Defaults: 127.0.0.1:42617
agentzero gateway --host 0.0.0.0 --port 8081    # Custom bind address
agentzero gateway --new-pairing                  # Clear paired tokens, fresh pairing code
```

| Flag | Description |
|---|---|
| `--host <HOST>` | Interface to bind (default: `127.0.0.1`) |
| `-p, --port <PORT>` | Port to bind (default: `42617`) |
| `--new-pairing` | Clear all paired tokens and generate a fresh pairing code |

**Endpoints:**

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check |
| `/metrics` | GET | Prometheus-style metrics |
| `/pair` | POST | Pair a client (get bearer token) |
| `/api/chat` | POST | Send a chat message |
| `/v1/chat/completions` | POST | OpenAI-compatible completions API |
| `/v1/models` | GET | List available models |
| `/v1/ping` | POST | Connectivity check |
| `/v1/webhook/:channel` | POST | Channel-specific webhook |
| `/ws/chat` | GET | WebSocket chat |

### `daemon`

Manage the background daemon process. The daemon runs the gateway server as a detached process with logging.

#### `daemon start`

Start the daemon. By default spawns a background process and returns immediately. Logs to `{data_dir}/daemon.log`.

```bash
agentzero daemon start                           # Background, default 127.0.0.1:42617
agentzero daemon start --port 9000               # Custom port
agentzero daemon start --foreground              # Foreground (for systemd/debugging)
```

| Flag | Description |
|---|---|
| `--host <HOST>` | Interface to bind (default: `127.0.0.1`) |
| `-p, --port <PORT>` | Port to bind (default: `42617`) |
| `--foreground` | Run in foreground instead of daemonizing |

#### `daemon stop`

Stop the running daemon process.

```bash
agentzero daemon stop
```

#### `daemon status`

Show whether the daemon is running, its PID, bind address, and log file location.

```bash
agentzero daemon status
agentzero daemon status --json
```

| Flag | Description |
|---|---|
| `--json` | Emit JSON with `running`, `host`, `port`, `pid`, `started_at_epoch_seconds` |

### `service`

Install and manage AgentZero as an OS service for automatic startup on boot. Auto-detects systemd or OpenRC.

```bash
agentzero service install                        # Install service
agentzero service start                          # Start service
agentzero service stop                           # Stop service
agentzero service restart                        # Restart service
agentzero service status                         # Show install/running state
agentzero service uninstall                      # Remove service
agentzero service --service-init systemd install # Force systemd
```

| Flag | Description |
|---|---|
| `--service-init <INIT>` | Init system: `auto` (default), `systemd`, `openrc` |

### `dashboard`

Launch an interactive terminal dashboard for real-time monitoring.

```bash
agentzero dashboard
```

---

## Configuration

### `config`

Inspect and modify the `agentzero.toml` configuration file.

#### `config show`

Print the effective configuration as JSON. Secrets are masked by default.

```bash
agentzero config show                            # Secrets masked
agentzero config show --raw                      # Secrets visible
```

| Flag | Description |
|---|---|
| `--raw` | Emit raw JSON without masking secrets |

#### `config get`

Query a single config value by dot-separated path.

```bash
agentzero config get provider.model
agentzero config get agent.max_tool_iterations
```

#### `config set`

Set a config value in `agentzero.toml`. Type is auto-inferred (bool, int, float, string).

```bash
agentzero config set provider.model "gpt-4o"
agentzero config set agent.max_tool_iterations 20
```

#### `config schema`

Print the config template or JSON schema.

```bash
agentzero config schema                          # TOML template
agentzero config schema --json                   # JSON schema
```

| Flag | Description |
|---|---|
| `--json` | Emit JSON schema instead of TOML template |

---

## Backup & Restore

### `backup`

Export and restore encrypted data stores. Backups are copied as-is (never decrypted) with SHA-256 integrity verification.

#### `backup export`

Export all encrypted store files to a directory with a checksummed manifest.

```bash
agentzero backup export /path/to/backup-dir
```

| Flag | Description |
|---|---|
| `<output-dir>` | **(Required)** Directory to write backup files and manifest |

Exports 10 known stores (api-keys, cost data, identities, coordination status, goals, estop state, auth profiles, hooks, channel config). Each file is copied with its raw encrypted bytes and a SHA-256 checksum recorded in `manifest.json`. An integrity chain hash (SHA-256 of all individual hashes) ensures tamper detection.

#### `backup restore`

Restore encrypted store files from a backup directory. Validates manifest version, verifies all checksums, and enforces file permissions (0600 on Unix).

```bash
agentzero backup restore /path/to/backup-dir
agentzero backup restore /path/to/backup-dir --force
```

| Flag | Description |
|---|---|
| `<archive-path>` | **(Required)** Directory containing backup files and manifest |
| `--force` | Overwrite existing store files (default: skip if present) |

---

## Authentication

### `auth`

Manage provider authentication profiles. Supports OAuth (browser-based), API key paste, and multi-profile switching. Credentials are stored in an encrypted auth store at `{data_dir}/auth/`.

#### `auth login`

Start an interactive login flow. Opens a browser for OAuth providers (OpenAI Codex, Anthropic) or prompts for an API key (Gemini).

```bash
agentzero auth login                             # Interactive provider selection
agentzero auth login --provider openai-codex     # OAuth browser flow
agentzero auth login --provider anthropic        # OAuth browser flow (claude.ai)
agentzero auth login --provider gemini           # API key paste
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | Provider: `openai-codex`, `gemini`, `anthropic`. Prompts if omitted |
| `--profile <NAME>` | Profile name to save under (default: `default`) |
| `--device-code` | Use OAuth device-code flow (planned) |

#### `auth setup-token`

Save an API key or token for a provider. Prompts interactively if `--token` is omitted.

```bash
agentzero auth setup-token --provider openrouter
agentzero auth setup-token --provider anthropic --token sk-ant-...
agentzero auth setup-token --provider openai --token sk-... --profile work
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | **(Required)** Provider id |
| `--token <TOKEN>` | Token value. Prompts interactively if omitted |
| `--profile <NAME>` | Profile name (default: `default`) |
| `--activate` | Set as active profile after saving (default: `true`) |

#### `auth paste-token`

Alias for `setup-token`. Same flags and behavior.

#### `auth paste-redirect`

Complete an OAuth flow by pasting the redirect URL or authorization code.

```bash
agentzero auth paste-redirect --provider openai-codex
agentzero auth paste-redirect --provider anthropic --input "https://..."
agentzero auth paste-redirect --provider gemini --input "https://..."
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | **(Required)** Provider id |
| `--profile <NAME>` | Profile name (default: `default`) |
| `--input <URL_OR_CODE>` | Redirect URL or raw auth code. Prompts if omitted |

#### `auth refresh`

Refresh an expired OAuth access token using a stored refresh token.

```bash
agentzero auth refresh --provider openai-codex
agentzero auth refresh --provider anthropic
agentzero auth refresh --provider gemini --profile work
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | **(Required)** Provider id |
| `--profile <NAME>` | Profile name to refresh |

#### `auth use`

Set the active auth profile for a provider. The active profile is used when no `--profile` override is given.

```bash
agentzero auth use --provider anthropic --profile work
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | **(Required)** Provider id |
| `--profile <NAME>` | **(Required)** Profile name or full profile id |

#### `auth list`

Show all configured auth profiles.

```bash
agentzero auth list
agentzero auth list --json
```

| Flag | Description |
|---|---|
| `--json` | JSON array with `name`, `provider`, `active`, `has_refresh_token`, timestamps |

#### `auth status`

Show the active profile, token type, and expiry information for each provider.

```bash
agentzero auth status
agentzero auth status --json
```

| Flag | Description |
|---|---|
| `--json` | JSON with `active_profile`, `active_provider`, `total_profiles`, expiry info |

#### `auth logout`

Remove an authentication profile.

```bash
agentzero auth logout --provider openrouter
agentzero auth logout --provider anthropic --profile work
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | **(Required)** Provider id |
| `--profile <NAME>` | Profile name (default: `default`) |

---

## Built-in Tools

The agent has access to built-in tools that it calls automatically during the agent loop. Tools are gated by the `[security]` section in `agentzero.toml`. There is no CLI command to invoke tools directly — they are used by the agent when processing messages.

### Always Available

| Tool | Description |
|---|---|
| `read_file` | Read files within the configured `allowed_root` |
| `shell` | Execute shell commands (restricted to `allowed_commands`) |
| `glob_search` | Find files by glob pattern |
| `content_search` | Search file contents by text or regex |
| `memory_store` | Persist a key-value memory entry |
| `memory_recall` | Retrieve stored memory entries |
| `memory_forget` | Delete memory entries |
| `image_info` | Read image metadata (dimensions, format, EXIF) |
| `docx_read` | Extract text from Microsoft Word documents |
| `pdf_read` | Extract text from PDF files |
| `screenshot` | Capture a screenshot |
| `task_plan` | Create and manage structured task plans |
| `process_tool` | Manage system processes |
| `subagent_spawn` | Start a sub-agent with a specific task |
| `subagent_list` | List active sub-agents |
| `subagent_manage` | Stop or manage sub-agent lifecycle |
| `cli_discovery` | Check if shell commands exist on PATH, get runtime info |
| `proxy_config` | Configure proxy settings |
| `delegate_coordination_status` | Track delegation status across agents |
| `sop_list` | List available standard operating procedures |
| `sop_status` | Show status of an active SOP |
| `sop_advance` | Advance to the next step in an SOP |
| `sop_approve` | Approve the current SOP step |
| `sop_execute` | Execute an SOP |

### Conditionally Available

These tools are disabled by default and must be enabled in `agentzero.toml`:

| Tool | Config Flag | Description |
|---|---|---|
| `write_file` | `security.write_file.enabled = true` | Write files within allowed root |
| `apply_patch` | `security.write_file.enabled = true` | Apply unified diffs to files |
| `file_edit` | `security.write_file.enabled = true` | Make targeted edits to files |
| `git_operations` | `security.enable_git = true` | Run git commands |
| `cron_add` | `security.enable_cron = true` | Add a scheduled task |
| `cron_list` | `security.enable_cron = true` | List scheduled tasks |
| `cron_remove` | `security.enable_cron = true` | Remove a scheduled task |
| `cron_update` | `security.enable_cron = true` | Update a scheduled task |
| `cron_pause` | `security.enable_cron = true` | Pause a scheduled task |
| `cron_resume` | `security.enable_cron = true` | Resume a paused task |
| `web_search` | `web_search.enabled = true` | Search the web (DuckDuckGo, Brave, Jina) |
| `browser` | `browser.enabled = true` | Browse web pages |
| `browser_open` | `browser.enabled = true` | Open a URL in the browser |
| `mcp__{server}__{tool}` | `security.mcp.enabled = true` + `mcp.json` | MCP server tools (one per remote tool) |
| `process_plugin` | `security.plugin.enabled = true` | Execute process-based plugins |
| `composio` | `composio.enabled = true` | Use Composio integrations |
| `pushover` | `security.enable_pushover = true` | Send push notifications via Pushover |
| `model_routing_config` | Automatically enabled when model router is configured | Configure model routing rules |
| `delegate` | Automatically enabled when delegate agents are configured in `[agents.*]` | Delegate tasks to other agents |

### Tool Security Configuration

```toml
[security]
allowed_root = "."                         # File access boundary
allowed_commands = ["ls", "pwd", "cat"]    # Shell command allowlist

[security.read_file]
max_read_bytes = 262144                    # 256 KiB default
allow_binary = false

[security.write_file]
enabled = false                            # Disabled by default
max_write_bytes = 65536                    # 64 KiB limit

[security.shell]
max_args = 32
max_arg_length = 4096
max_output_bytes = 65536
forbidden_chars = ";&|><$`\n\r"

[security.mcp]
enabled = false
allowed_servers = []

[security.url_access]
block_private_ip = true
allow_loopback = false

[security.otp]
enabled = false
gated_actions = ["shell", "file_write", "browser_open", "browser", "memory_forget"]
```

---

## Skills

### `skill`

Skills are composable agent behaviors written in TypeScript, Rust, Go, or Python. They are stored in an encrypted state file at `{data_dir}/skills-state.json`.

#### `skill list`

List all installed skills with their enabled status and source.

```bash
agentzero skill list
agentzero skill list --json
```

| Flag | Description |
|---|---|
| `--json` | Emit JSON array of skill records |

#### `skill install`

Install a skill by name and source.

```bash
agentzero skill install --name research --source local
agentzero skill install --name code-review --source builtin
```

| Flag | Description |
|---|---|
| `--name <NAME>` | **(Required)** Skill name (alphanumeric, hyphens, underscores) |
| `--source <SOURCE>` | Source of the skill (default: `local`) |

#### `skill test`

Run a basic validation check on an installed skill. Shows source and enabled status.

```bash
agentzero skill test --name research
```

| Flag | Description |
|---|---|
| `--name <NAME>` | **(Required)** Skill name |

#### `skill remove`

Remove an installed skill.

```bash
agentzero skill remove --name research
```

| Flag | Description |
|---|---|
| `--name <NAME>` | **(Required)** Skill name |

#### `skill new`

Scaffold a new skill project with a manifest and source file.

```bash
agentzero skill new my-skill                     # TypeScript (default)
agentzero skill new my-skill --template rust     # Rust
agentzero skill new my-skill --template python --dir ./skills
```

| Flag | Description |
|---|---|
| `name` | **(Required, positional)** Name for the new skill |
| `--template <LANG>` | Scaffold template: `typescript`/`ts` (default), `rust`/`rs`, `go`, `python`/`py` |
| `--dir <PATH>` | Target directory (default: workspace root) |

Generated files:
- `skill.json` — manifest with name, version, template, entry point
- `src/main.{ts,rs,go,py}` — scaffolded source file

#### `skill audit`

Audit an installed skill for security and compatibility. Checks manifest validity, source trust, and permission scope.

```bash
agentzero skill audit --name research
agentzero skill audit --name research --json
```

| Flag | Description |
|---|---|
| `--name <NAME>` | **(Required)** Skill name |
| `--json` | Emit JSON with checks: `manifest_valid`, `source_trusted`, `permissions_scoped` |

#### `skill templates`

List available scaffold templates with their entry points.

```bash
agentzero skill templates
```

---

## Plugins

### `plugin`

Plugins are sandboxed WebAssembly modules that extend AgentZero's capabilities. They run in an isolated WASM runtime (wasmtime) with configurable resource limits.

#### `plugin new`

Scaffold a plugin manifest template.

```bash
agentzero plugin new --id my-plugin
agentzero plugin new --id my-plugin --version 1.0.0 --entrypoint main --force
```

| Flag | Description |
|---|---|
| `--id <ID>` | **(Required)** Plugin identifier |
| `--version <VER>` | Plugin version (default: `0.1.0`) |
| `--entrypoint <NAME>` | WASM entrypoint function (default: `run`) |
| `--wasm-file <PATH>` | WASM file reference (default: `plugin.wasm`) |
| `--out-dir <DIR>` | Output directory (default: current directory) |
| `--force` | Overwrite existing manifest |

#### `plugin validate`

Validate a plugin manifest for correctness. Checks id, version, entrypoint, WASM file extension, SHA256 format, and API version compatibility.

```bash
agentzero plugin validate --manifest manifest.json
```

| Flag | Description |
|---|---|
| `--manifest <PATH>` | **(Required)** Path to `manifest.json` |

#### `plugin test`

Run plugin preflight checks and optional execution in a sandboxed environment. Test isolation: 5s timeout, 5MB module limit, 64MB memory, no network, no filesystem writes.

```bash
agentzero plugin test --manifest manifest.json --wasm plugin.wasm
agentzero plugin test --manifest manifest.json --wasm plugin.wasm --execute
```

| Flag | Description |
|---|---|
| `--manifest <PATH>` | **(Required)** Path to `manifest.json` |
| `--wasm <PATH>` | **(Required)** Path to compiled `.wasm` module |
| `--execute` | Execute the entrypoint after preflight (default: preflight only) |

#### `plugin dev`

Run a local development loop: validate, preflight, and optionally execute with deterministic fixtures.

```bash
agentzero plugin dev --manifest manifest.json --wasm plugin.wasm
agentzero plugin dev --manifest manifest.json --wasm plugin.wasm --iterations 5
```

| Flag | Description |
|---|---|
| `--manifest <PATH>` | **(Required)** Path to `manifest.json` |
| `--wasm <PATH>` | **(Required)** Path to `.wasm` module |
| `--iterations <N>` | Number of loop iterations (default: `1`) |
| `--execute` | Execute entrypoint in addition to preflight (default: `true`) |

#### `plugin package`

Package a plugin into an installable tar archive. Computes SHA256 checksum and embeds it in the manifest.

```bash
agentzero plugin package --manifest manifest.json --wasm plugin.wasm --out my-plugin.tar
```

| Flag | Description |
|---|---|
| `--manifest <PATH>` | **(Required)** Path to `manifest.json` |
| `--wasm <PATH>` | **(Required)** Path to `.wasm` module |
| `--out <PATH>` | **(Required)** Output archive path |

#### `plugin install`

Install a packaged plugin archive. Verifies checksum integrity, then installs to `{data_dir}/plugins/{id}/{version}/`.

```bash
agentzero plugin install --package my-plugin.tar
agentzero plugin install --package my-plugin.tar --install-dir ./plugins
```

| Flag | Description |
|---|---|
| `--package <PATH>` | **(Required)** Path to plugin archive (`.tar`) |
| `--install-dir <DIR>` | Installation directory (default: `{data_dir}/plugins`) |

#### `plugin list`

List installed plugins with their version and install location.

```bash
agentzero plugin list
agentzero plugin list --json
```

| Flag | Description |
|---|---|
| `--json` | Emit JSON with `install_root` and `plugins` array |
| `--install-dir <DIR>` | Installation directory (default: `{data_dir}/plugins`) |

#### `plugin remove`

Remove an installed plugin. Can remove a specific version or all versions.

```bash
agentzero plugin remove --id my-plugin                    # All versions
agentzero plugin remove --id my-plugin --version 0.1.0    # Specific version
```

| Flag | Description |
|---|---|
| `--id <ID>` | **(Required)** Plugin identifier |
| `--version <VER>` | Specific version to remove (default: all versions) |
| `--install-dir <DIR>` | Installation directory (default: `{data_dir}/plugins`) |

#### `plugin enable`

Enable a disabled plugin without reinstalling.

```bash
agentzero plugin enable --id my-plugin
```

| Flag | Description |
|---|---|
| `--id <ID>` | **(Required)** Plugin identifier |

#### `plugin disable`

Disable a plugin without removing it.

```bash
agentzero plugin disable --id my-plugin
```

| Flag | Description |
|---|---|
| `--id <ID>` | **(Required)** Plugin identifier |

#### `plugin search`

Search the plugin registry.

```bash
agentzero plugin search "web scraping"
```

| Flag | Description |
|---|---|
| `--registry-url <URL>` | Registry URL (default: configured `registry_url`) |

#### `plugin outdated`

Check for available plugin updates.

```bash
agentzero plugin outdated
```

| Flag | Description |
|---|---|
| `--registry-url <URL>` | Registry URL (default: configured `registry_url`) |

#### `plugin update`

Update installed plugins to latest versions.

```bash
agentzero plugin update                      # all plugins
agentzero plugin update --id my-plugin       # specific plugin
```

| Flag | Description |
|---|---|
| `--id <ID>` | Update only this plugin (default: all) |
| `--registry-url <URL>` | Registry URL |
| `--install-dir <DIR>` | Installation directory |

#### `plugin refresh`

Force-refresh the cached registry index.

```bash
agentzero plugin refresh
agentzero plugin refresh --registry-url https://custom-registry.example.com/index.json
```

| Flag | Description |
|---|---|
| `--registry-url <URL>` | Registry URL to fetch (default: configured `registry_url`) |

### Plugin Manifest Format

```json
{
  "id": "my-plugin",
  "version": "0.1.0",
  "entrypoint": "run",
  "wasm_file": "plugin.wasm",
  "wasm_sha256": "a1b2c3...",
  "capabilities": ["tool.call"],
  "hooks": ["before_tool_call"],
  "min_runtime_api": 1,
  "max_runtime_api": 1,
  "allowed_host_calls": []
}
```

### WASM Isolation Defaults

| Limit | Default | Test/Dev |
|---|---|---|
| Max execution time | 30s | 5s |
| Max module size | 5 MB | 5 MB |
| Max memory | 256 MB | 64 MB |
| Network access | No | No |
| Filesystem writes | No | No |

### `tools`

Inspect registered tools, their descriptions, and JSON schemas. Useful for debugging which tools the agent has access to and verifying schema definitions.

#### `tools list`

List all registered tools.

```bash
agentzero tools list
agentzero tools list --with-schema
agentzero tools list --json
```

| Flag | Description |
|---|---|
| `--with-schema` | Only show tools that have a JSON input schema defined |
| `--json` | Output as JSON array |

#### `tools info`

Show details for a specific tool.

```bash
agentzero tools info read_file
agentzero tools info shell
```

| Argument | Description |
|---|---|
| `<NAME>` | **(Required)** Tool name to inspect |

#### `tools schema`

Print the JSON input schema for a specific tool.

```bash
agentzero tools schema read_file
agentzero tools schema shell --pretty
```

| Argument / Flag | Description |
|---|---|
| `<NAME>` | **(Required)** Tool name |
| `--pretty` | Pretty-print the JSON schema |

---

## Memory & Knowledge

### `memory`

Inspect and manage the agent's conversation memory store. Backend is configured in `agentzero.toml` (`sqlite`, `lucid`, `markdown`, or `none`).

#### `memory list`

List memory entries with pagination.

```bash
agentzero memory list
agentzero memory list --limit 100 --offset 50
agentzero memory list --json
```

| Flag | Description |
|---|---|
| `--limit <N>` | Maximum entries to return (default: `50`) |
| `--offset <N>` | Pagination offset (default: `0`) |
| `--category <CAT>` | Filter by category (reserved) |
| `--session <SID>` | Filter by session (reserved) |
| `--json` | Emit machine-readable JSON output |

#### `memory get`

Retrieve a memory entry by key prefix. Returns the most recent match.

```bash
agentzero memory get --key "session-abc"
agentzero memory get --json                      # Most recent entry
```

| Flag | Description |
|---|---|
| `--key <KEY>` | Key or prefix to match (most recent if omitted) |
| `--json` | Emit machine-readable JSON output |

#### `memory stats`

Show memory store statistics.

```bash
agentzero memory stats
agentzero memory stats --json
```

| Flag | Description |
|---|---|
| `--json` | Emit machine-readable JSON output |

#### `memory clear`

Delete memory entries. Can target specific keys or clear all.

```bash
agentzero memory clear --yes                     # Clear all (skip confirmation)
agentzero memory clear --key "old-session"       # Delete by key prefix
```

| Flag | Description |
|---|---|
| `--key <KEY>` | Delete entries matching this key prefix |
| `--category <CAT>` | Filter by category (reserved) |
| `--yes` | Skip confirmation prompt |
| `--json` | Emit machine-readable JSON output |

### `conversation`

Manage conversation branches. Conversations allow you to fork and switch between parallel conversation threads.

#### `conversation list`

List all named conversations.

```bash
agentzero conversation list
agentzero conversation list --json
```

| Flag | Description |
|---|---|
| `--json` | Emit machine-readable JSON output |

#### `conversation fork`

Fork an existing conversation into a new branch.

```bash
agentzero conversation fork main experiment-1
agentzero conversation fork main experiment-1 --json
```

| Argument | Description |
|---|---|
| `<FROM>` | **(Required)** Source conversation ID to fork from |
| `<TO>` | **(Required)** New conversation ID for the fork |

| Flag | Description |
|---|---|
| `--json` | Emit machine-readable JSON output |

#### `conversation switch`

Switch the active conversation. Use an empty string to switch back to the global conversation.

```bash
agentzero conversation switch experiment-1
agentzero conversation switch ""              # back to global
```

| Argument | Description |
|---|---|
| `<ID>` | **(Required)** Conversation ID to switch to |

---

### `rag`

Local retrieval-augmented generation index. Ingest documents and query them by semantic similarity. Requires the `rag` feature.

#### `rag ingest`

Add a document to the RAG index.

```bash
agentzero rag ingest --id doc1 --text "Important context about the project..."
agentzero rag ingest --id doc2 --file ./notes.md
```

| Flag | Description |
|---|---|
| `--id <ID>` | **(Required)** Document identifier |
| `--text <TEXT>` | Inline text content |
| `--file <PATH>` | File path to ingest (alternative to `--text`) |
| `--json` | Emit machine-readable JSON output |

#### `rag query`

Query the RAG index for relevant documents.

```bash
agentzero rag query --query "What was the decision?" --limit 5
```

| Flag | Description |
|---|---|
| `--query <TEXT>` | **(Required)** Query text |
| `--limit <N>` | Maximum matches (default: `5`) |
| `--json` | Emit machine-readable JSON output |

---

## Security & Safety

### `estop`

Emergency stop — immediately halt agent execution at various severity levels. Can require OTP to resume.

```bash
agentzero estop                                  # Default: kill-all
agentzero estop --level kill-all --require-otp   # Require OTP to resume
agentzero estop --level network-kill             # Block all network access
agentzero estop --level domain-block --domain "*.example.com" --domain "evil.com"
agentzero estop --level tool-freeze --tool shell --tool write_file
```

| Flag | Description |
|---|---|
| `--level <LEVEL>` | Severity: `KillAll`, `NetworkKill`, `DomainBlock`, `ToolFreeze` |
| `--domain <PATTERN>` | Domain pattern(s) for domain-block (repeatable) |
| `--tool <NAME>` | Tool name(s) for tool-freeze (repeatable) |
| `--require-otp` | Require TOTP code to resume |

#### `estop status`

Show current emergency stop state, level, blocked domains/tools, and timestamp.

```bash
agentzero estop status
```

#### `estop resume`

Resume from emergency stop. Can resume partially (specific domains or tools) or fully.

```bash
agentzero estop resume                           # Resume all
agentzero estop resume --otp 123456              # Resume with OTP
agentzero estop resume --network                 # Resume network only
agentzero estop resume --domain "*.example.com"  # Unblock specific domain
agentzero estop resume --tool shell              # Unfreeze specific tool
```

| Flag | Description |
|---|---|
| `--network` | Resume only network kill |
| `--domain <PATTERN>` | Unblock specific domain(s) (repeatable) |
| `--tool <NAME>` | Unfreeze specific tool(s) (repeatable) |
| `--otp <CODE>` | OTP code (prompted if required but omitted) |

### `approval`

Evaluate approval requirements for high-risk actions using the approval policy engine.

```bash
agentzero approval evaluate --actor agent --action shell --risk high
agentzero approval evaluate --actor agent --action shell --risk high --decision allow --approver admin --reason "deployment"
```

| Flag | Description |
|---|---|
| `--actor <NAME>` | Actor requesting the action |
| `--action <NAME>` | Action being evaluated |
| `--risk <LEVEL>` | Risk level of the action |
| `--approver <NAME>` | Who approved the action |
| `--decision <DECISION>` | Decision: `allow` or `deny` |
| `--reason <TEXT>` | Reason for the decision |
| `--json` | Emit machine-readable JSON output |

### `privacy`

Manage privacy mode, key rotation, and run diagnostics. Requires the `privacy` Cargo feature flag.

#### `privacy status`

Show current privacy mode and feature status.

```bash
agentzero privacy status
agentzero privacy status --json
```

| Flag | Description |
|---|---|
| `--json` | Emit machine-readable JSON output |

Output includes: privacy mode, Noise Protocol status, sealed envelopes status, key rotation status and interval.

#### `privacy rotate-keys`

Rotate identity keypair. Checks if the rotation interval has elapsed; use `--force` for immediate rotation.

```bash
agentzero privacy rotate-keys
agentzero privacy rotate-keys --force
agentzero privacy rotate-keys --json
```

| Flag | Description |
|---|---|
| `--force` | Force immediate rotation regardless of interval |
| `--json` | Emit machine-readable JSON output |

Output includes: new epoch number, key fingerprint, whether rotation occurred, next rotation timestamp.

#### `privacy generate-keypair`

Generate a new identity keypair without activating it. Use `rotate-keys` to activate a new key.

```bash
agentzero privacy generate-keypair
agentzero privacy generate-keypair --json
```

| Flag | Description |
|---|---|
| `--json` | Emit machine-readable JSON output |

#### `privacy test`

Run 8 diagnostic checks on the privacy subsystem: config validation, boundary resolution, memory isolation, sealed envelope round-trip, Noise XX handshake, Noise IK handshake, channel locality, encrypted store round-trip.

```bash
agentzero privacy test
agentzero privacy test --json
```

| Flag | Description |
|---|---|
| `--json` | Emit machine-readable JSON output with per-check pass/fail |

Returns exit code `1` if any checks fail.

---

## Models & Local Providers

### `models`

Manage provider model catalogs. Models are cached with a 12-hour TTL.

#### `models refresh`

Fetch the latest model list from providers.

```bash
agentzero models refresh                         # Refresh configured provider
agentzero models refresh --all --force           # Force refresh all providers
agentzero models refresh --provider anthropic
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | Refresh specific provider (default: configured provider) |
| `--all` | Refresh all providers that support live discovery |
| `--force` | Force live refresh, ignore fresh cache |

#### `models list`

List cached models for a provider.

```bash
agentzero models list
agentzero models list --provider ollama
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | List models for this provider (default: configured provider) |

#### `models set`

Set the default model in your config file.

```bash
agentzero models set anthropic/claude-sonnet-4-6
agentzero models set gpt-4o-mini
```

#### `models status`

Show current provider, default model, and cache freshness.

```bash
agentzero models status
```

#### `models pull`

Download a model from a local provider (Ollama and similar). Shows a progress bar.

```bash
agentzero models pull llama3.1:8b
agentzero models pull llama3.1:8b --provider ollama
```

| Flag | Description |
|---|---|
| `--provider <NAME>` | Provider to pull from (default: configured local provider) |

### `local`

Discover and manage local AI model services (Ollama, llama.cpp, LM Studio, vLLM, SGLang).

#### `local discover`

Scan default ports for running local AI services.

```bash
agentzero local discover
agentzero local discover --timeout-ms 5000 --json
agentzero local discover --retries 2    # Retry unreachable providers with backoff
```

| Flag | Description |
|---|---|
| `--timeout-ms <MS>` | Probe timeout in milliseconds (default: `2000`) |
| `--retries <N>` | Retry unreachable providers up to N times with backoff (default: `0`) |
| `--json` | Emit machine-readable JSON output |

#### `local status`

Show status of the configured local provider.

```bash
agentzero local status
agentzero local status --json
```

| Flag | Description |
|---|---|
| `--json` | Emit machine-readable JSON output |

#### `local health`

Run a health check against a specific local provider endpoint.

```bash
agentzero local health ollama
agentzero local health vllm --url http://gpu-server:8000
```

| Flag | Description |
|---|---|
| `provider` | **(Required, positional)** Provider name: `ollama`, `llamacpp`, `lmstudio`, `vllm`, `sglang` |
| `--url <URL>` | Custom base URL (overrides default) |

---

## Scheduling & Hooks

### `cron`

Manage scheduled tasks. Tasks are stored in `{data_dir}/cron-tasks.json`.

#### `cron list`

```bash
agentzero cron list
agentzero cron list --json
```

#### `cron add`

Add a scheduled task with a cron expression.

```bash
agentzero cron add --id backup --schedule "0 2 * * *" --command "agentzero agent -m 'run backup'"
```

| Flag | Description |
|---|---|
| `--id <ID>` | **(Required)** Task identifier |
| `--schedule <EXPR>` | **(Required)** Cron expression |
| `--command <CMD>` | **(Required)** Command to execute |

#### `cron add-at`

Add a task scheduled at a specific time.

```bash
agentzero cron add-at --id deploy --schedule "2026-03-15T10:00:00" --command "deploy.sh"
```

#### `cron add-every`

Add a recurring task with a cadence expression.

```bash
agentzero cron add-every --id heartbeat --schedule "5m" --command "agentzero status"
```

#### `cron once`

Add a one-time scheduled task.

```bash
agentzero cron once --id migrate --schedule "2026-03-01T00:00:00" --command "migrate"
```

#### `cron update`

Update an existing task's schedule or command.

```bash
agentzero cron update --id backup --schedule "0 3 * * *"
agentzero cron update --id backup --command "new-backup-script"
```

| Flag | Description |
|---|---|
| `--id <ID>` | **(Required)** Task identifier |
| `--schedule <EXPR>` | New schedule (optional) |
| `--command <CMD>` | New command (optional) |

#### `cron pause` / `cron resume` / `cron remove`

```bash
agentzero cron pause --id backup
agentzero cron resume --id backup
agentzero cron remove --id backup
```

### `hooks`

Manage lifecycle hooks that run at specific points in the agent execution cycle.

#### `hooks list`

```bash
agentzero hooks list
agentzero hooks list --json
```

#### `hooks enable` / `hooks disable`

```bash
agentzero hooks enable --name pre-tool
agentzero hooks disable --name pre-tool
```

#### `hooks test`

Run a hook test to verify it executes correctly.

```bash
agentzero hooks test --name pre-tool
```

---

## Channels & Integrations

### `channel`

Manage messaging channels (Telegram, Discord, Slack, etc.). Channel credentials are stored in an encrypted state file.

#### `channel add`

Add a new channel. Prompts for channel-specific configuration (API keys, chat IDs, etc.).

```bash
agentzero channel add                            # Interactive prompt
agentzero channel add telegram                   # Add by name
agentzero channel add discord
```

#### `channel list`

```bash
agentzero channel list
```

#### `channel doctor`

Run channel diagnostics — checks configured channels and dispatch engine.

```bash
agentzero channel doctor
```

#### `channel start`

Launch configured channels and test connectivity.

```bash
agentzero channel start
```

#### `channel remove`

```bash
agentzero channel remove                         # Interactive prompt
agentzero channel remove telegram                # Remove by name
```

### `integrations`

Browse and validate available third-party integrations.

```bash
agentzero integrations list
agentzero integrations list --category ai --status enabled
agentzero integrations search --query "slack"
agentzero integrations info
```

| Flag | Description |
|---|---|
| `--category <CAT>` | Filter by category |
| `--status <STATUS>` | Filter by status |
| `--query <TEXT>` | Search query |

---

## Templates

### `template`

Manage template files that define agent behavior. Templates are Markdown files loaded at agent startup (e.g., `AGENTS.md`, `IDENTITY.md`, `SOUL.md`, `TOOLS.md`).

#### `template list`

List all template files with their status and source location.

```bash
agentzero template list
agentzero template list --json
```

| Flag | Description |
|---|---|
| `--json` | JSON array with `name`, `session`, `found`, `source` |

Available templates: `AGENTS.md`, `BOOT.md`, `BOOTSTRAP.md`, `HEARTBEAT.md`, `IDENTITY.md`, `SOUL.md`, `TOOLS.md`, `USER.md`

#### `template show`

Display the content of a specific template.

```bash
agentzero template show IDENTITY
agentzero template show soul                     # Case-insensitive
```

#### `template init`

Scaffold template files with default content.

```bash
agentzero template init                          # All templates
agentzero template init --name IDENTITY          # Single template
agentzero template init --dir ./templates --force
```

| Flag | Description |
|---|---|
| `--name <NAME>` | Single template to scaffold (default: all) |
| `--dir <DIR>` | Target directory (default: workspace root) |
| `--force` | Overwrite existing files |

#### `template validate`

Check that template files exist and have content.

```bash
agentzero template validate
```

---

## Identity & Coordination

### `identity`

Manage actor identities and roles. Identities track who (human, agent, service) is performing actions.

```bash
agentzero identity upsert --id agent-1 --name "Primary Agent" --kind agent
agentzero identity get --id agent-1
agentzero identity get --id agent-1 --json
agentzero identity add-role --id agent-1 --role admin
```

| Subcommand | Description | Key Flags |
|---|---|---|
| `upsert` | Create or update an identity | `--id`, `--name`, `--kind` (Human/Agent/Service), `--json` |
| `get` | Show identity by id | `--id`, `--json` |
| `add-role` | Add a role to an identity | `--id`, `--role`, `--json` |

### `coordination`

Inspect and update the multi-agent coordination runtime.

```bash
agentzero coordination status
agentzero coordination status --json
agentzero coordination set --active-workers 4 --queued-tasks 10
```

| Subcommand | Description | Key Flags |
|---|---|---|
| `status` | Show worker/task counts | `--json` |
| `set` | Update counts | `--active-workers`, `--queued-tasks` |

### `cost`

Track accumulated API usage cost.

```bash
agentzero cost status
agentzero cost status --json
agentzero cost record --tokens 1500 --usd 0.003
agentzero cost reset
```

| Subcommand | Description | Key Flags |
|---|---|---|
| `status` | Show cost summary | `--json` |
| `record` | Record token usage | `--tokens`, `--usd` |
| `reset` | Reset cost summary | — |

### `goals`

Manage runtime goals for tracking agent progress.

```bash
agentzero goals list
agentzero goals list --json
agentzero goals add --id ship-v1 --title "Ship v1.0"
agentzero goals complete --id ship-v1
```

| Subcommand | Description | Key Flags |
|---|---|---|
| `list` | List goals | `--json` |
| `add` | Add a goal | `--id`, `--title` |
| `complete` | Mark goal complete | `--id` |

---

## Utility

### `tunnel`

Manage secure tunnels for exposing local services.

```bash
agentzero tunnel start --protocol https --remote example.com:443 --local-port 8443
agentzero tunnel start --name my-tunnel --protocol http --remote api.example.com:80 --local-port 3000
agentzero tunnel status
agentzero tunnel status --json
agentzero tunnel stop
```

| Subcommand | Key Flags |
|---|---|
| `start` | `--name` (default: `default`), `--protocol` (http/https/ssh), `--remote` (host:port), `--local-port` |
| `stop` | `--name` |
| `status` | `--name`, `--json` |

### `migrate`

Migrate data from external runtimes.

```bash
agentzero migrate import --source /path/to/source
agentzero migrate import --source /path/to/source --dry-run
```

| Flag | Description |
|---|---|
| `--source <PATH>` | Source directory to import from |
| `--dry-run` | Validate and preview without writing |

### `update`

Self-update operations. Check for new versions, apply updates, or roll back.

```bash
agentzero update --check                         # Quick check
agentzero update check --channel stable --json   # Detailed check
agentzero update apply --version 1.2.0           # Apply specific version
agentzero update rollback                        # Roll back to previous
agentzero update status                          # Show update state
```

| Subcommand | Key Flags |
|---|---|
| `check` | `--channel` (default: `stable`), `--json` |
| `apply` | `--version`, `--json` |
| `rollback` | `--json` |
| `status` | `--json` |

### `completions`

Generate shell completion scripts. Pipe to your shell's completion file.

```bash
agentzero completions --shell bash >> ~/.bashrc
agentzero completions --shell zsh >> ~/.zshrc
agentzero completions --shell fish > ~/.config/fish/completions/agentzero.fish
```

| Flag | Description |
|---|---|
| `--shell <SHELL>` | Shell: `bash`, `zsh`, `fish`, `elvish`, `powershell` |

### `hardware` (feature-gated)

Discover hardware boards and inspect chip details. Requires the `hardware` feature.

```bash
agentzero hardware discover                      # Scan for boards
agentzero hardware info --chip STM32F401RETx     # Chip details
agentzero hardware introspect                    # Board introspection
```

### `peripheral` (feature-gated)

Manage peripheral devices. Requires the `hardware` feature.

```bash
agentzero peripheral list
agentzero peripheral list --json
agentzero peripheral add --id sensor-1 --kind temperature --connection /dev/ttyUSB0
agentzero peripheral flash --id sensor-1 --firmware firmware.bin
agentzero peripheral flash-nucleo
agentzero peripheral setup-uno-q --host 192.168.0.48
```

| Subcommand | Description | Key Flags |
|---|---|---|
| `list` | List registered peripherals | `--json` |
| `add` | Register a peripheral | `--id`, `--kind`, `--connection`, `--json` |
| `flash` | Flash firmware | `--id`, `--firmware`, `--json` |
| `flash-nucleo` | Flash Nucleo board profile | `--json` |
| `setup-uno-q` | Setup Uno Q | `--host`, `--json` |
