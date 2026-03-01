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

Send a single message through the agent loop.

```bash
agentzero agent -m "hello"
agentzero agent -m "List files in the current directory"
```

| Flag | Description |
|---|---|
| `-m, --message <TEXT>` | **(Required)** Message to send |

### `onboard`

Generate a starter `agentzero.toml` config file.

```bash
agentzero onboard                               # Quick setup
agentzero onboard --interactive                  # Full wizard
agentzero onboard --force --provider openrouter --model anthropic/claude-sonnet-4-6
```

| Flag | Description |
|---|---|
| `--interactive` | Launch full onboarding wizard |
| `--force` | Overwrite existing config without confirmation |
| `--provider <NAME>` | Provider name (openai, openrouter, anthropic) |
| `--base-url <URL>` | Provider base URL |
| `--model <ID>` | Model identifier |
| `--memory <BACKEND>` | Memory backend (sqlite, lucid, markdown, none) |
| `--memory-path <PATH>` | Database file path |
| `--allowed-root <PATH>` | Filesystem scope root |
| `--allowed-commands <LIST>` | Comma-separated command allowlist |
| `--yes` | Skip prompts, auto-accept defaults |
| `--no-totp` | Disable OTP in quick setup |
| `--channels-only` | Reconfigure channels only (fast repair) |

### `status`

Show runtime status summary.

```bash
agentzero status
agentzero --json status
```

### `doctor`

Run diagnostics and trace inspection.

```bash
agentzero doctor models                          # Probe provider model catalogs
agentzero doctor models --provider openrouter    # Specific provider only
agentzero doctor traces                          # List recent trace events
agentzero doctor traces --event tool --contains shell --limit 50
```

### `providers`

List supported AI providers.

```bash
agentzero providers
agentzero providers --json
agentzero providers --no-color
```

### `providers-quota`

Inspect provider rate limits and circuit breaker state.

```bash
agentzero providers-quota
agentzero providers-quota --provider openrouter --json
```

---

## Gateway & Operations

### `gateway`

Start the HTTP gateway server.

```bash
agentzero gateway
agentzero gateway --host 0.0.0.0 --port 8081
agentzero gateway --new-pairing
```

### `daemon`

Start the long-running autonomous runtime.

```bash
agentzero daemon
agentzero daemon --host 127.0.0.1 --port 8080
```

### `service`

Manage OS service lifecycle (systemd/OpenRC).

```bash
agentzero service install
agentzero service start
agentzero service stop
agentzero service restart
agentzero service status
agentzero service uninstall
agentzero service --service-init systemd install   # Force systemd
```

### `dashboard`

Launch interactive terminal dashboard.

```bash
agentzero dashboard
```

---

## Configuration

### `config`

Inspect and modify configuration.

```bash
agentzero config show                            # Effective config (secrets masked)
agentzero config show --raw                      # With secrets visible
agentzero config get provider.model              # Query single value
agentzero config set provider.model "gpt-4o"     # Set a value
agentzero config schema                          # Print TOML template
agentzero config schema --json                   # Print JSON schema
```

---

## Authentication

### `auth`

Manage provider subscription auth profiles.

```bash
agentzero auth login --provider openai-codex
agentzero auth paste-token --provider anthropic --token "sk-ant-..."
agentzero auth setup-token --provider openai-codex
agentzero auth refresh --provider openai-codex
agentzero auth use --provider anthropic --profile default
agentzero auth list
agentzero auth status
agentzero auth logout --provider openai-codex
```

---

## Memory & Knowledge

### `memory`

Inspect and manage conversation memory.

```bash
agentzero memory list --limit 50
agentzero memory get --key "session-abc"
agentzero memory stats
agentzero memory clear --yes
agentzero memory clear --key "old-session"
```

### `rag`

Local retrieval-augmented generation index.

```bash
agentzero rag ingest --id doc1 --text "Important context..."
agentzero rag ingest --id doc2 --file ./notes.md
agentzero rag query --query "What was the decision?" --limit 5
```

---

## Security & Safety

### `estop`

Emergency stop — halt agent execution immediately.

```bash
agentzero estop                                  # Kill all
agentzero estop --level network-kill             # Network only
agentzero estop --level domain-block --domain "*.example.com"
agentzero estop --level tool-freeze --tool shell
agentzero estop status                           # Current state
agentzero estop resume                           # Resume all
agentzero estop resume --otp 123456              # Resume with OTP
```

### `approval`

Evaluate approval requirements for high-risk actions.

```bash
agentzero approval evaluate --actor agent --action shell --risk high
agentzero approval evaluate --actor agent --action shell --risk high --decision allow --approver admin
```

---

## Scheduling & Hooks

### `cron`

Manage scheduled tasks.

```bash
agentzero cron list
agentzero cron add --id backup --schedule "0 2 * * *" --command "backup-db"
agentzero cron add-every --id heartbeat --schedule "5m" --command "ping"
agentzero cron once --id migrate --schedule "2026-03-01T00:00:00" --command "migrate"
agentzero cron pause --id backup
agentzero cron resume --id backup
agentzero cron remove --id backup
```

### `hooks`

Manage lifecycle hooks.

```bash
agentzero hooks list
agentzero hooks enable --name pre-tool
agentzero hooks disable --name pre-tool
agentzero hooks test --name pre-tool
```

---

## Channels & Integrations

### `channel`

Manage messaging channels.

```bash
agentzero channel list
agentzero channel add
agentzero channel bind-telegram
agentzero channel doctor
agentzero channel start
agentzero channel remove
```

### `integrations`

Browse and validate integrations.

```bash
agentzero integrations list
agentzero integrations list --category ai
agentzero integrations search --query "slack"
agentzero integrations info
```

---

## Plugins & Skills

### `plugin`

WASM plugin developer lifecycle.

```bash
agentzero plugin new --id my-plugin
agentzero plugin validate --manifest manifest.json
agentzero plugin test --manifest manifest.json --wasm plugin.wasm --execute
agentzero plugin package --manifest manifest.json --wasm plugin.wasm --out out.tar.gz
agentzero plugin install --package out.tar.gz
agentzero plugin list
agentzero plugin remove --id my-plugin
agentzero plugin dev --manifest manifest.json --wasm plugin.wasm --iterations 3
```

### `skill`

Manage skills (composable agent behaviors).

```bash
agentzero skill list
agentzero skill install --name research --source local
agentzero skill test --name research
agentzero skill remove --name research
```

---

## Models

### `models`

Manage provider model catalogs.

```bash
agentzero models refresh                         # Refresh default provider
agentzero models refresh --all --force           # Force refresh all
agentzero models list                            # List cached models
agentzero models list --provider ollama
agentzero models set anthropic/claude-sonnet-4-6
agentzero models status
```

---

## Identity & Coordination

### `identity`

Manage actor identities and roles.

```bash
agentzero identity upsert --id agent-1 --name "Primary Agent" --kind agent
agentzero identity get --id agent-1
agentzero identity add-role --id agent-1 --role admin
```

### `coordination`

Inspect runtime coordination status.

```bash
agentzero coordination status
agentzero coordination set --active-workers 4 --queued-tasks 10
```

### `cost`

Inspect accumulated runtime cost.

```bash
agentzero cost status
agentzero cost record --tokens 1500 --usd 0.003
agentzero cost reset
```

### `goals`

Manage runtime goals.

```bash
agentzero goals list
agentzero goals add --id ship-v1 --title "Ship v1.0"
agentzero goals complete --id ship-v1
```

---

## Utility

### `tunnel`

Manage secure tunnels.

```bash
agentzero tunnel start --protocol https --remote example.com:443 --local-port 8443
agentzero tunnel status
agentzero tunnel stop
```

### `migrate`

Migrate from external runtimes.

```bash
agentzero migrate openclaw --source /path/to/openclaw
agentzero migrate openclaw --source /path/to/openclaw --dry-run
```

### `update`

Self-update operations.

```bash
agentzero update --check
agentzero update check
agentzero update apply --version 1.2.0
agentzero update rollback
agentzero update status
```

### `completions`

Generate shell completions.

```bash
agentzero completions --shell bash
agentzero completions --shell zsh
agentzero completions --shell fish
```

### `hardware` (feature-gated)

```bash
agentzero hardware discover
agentzero hardware info --chip STM32F401RETx
agentzero hardware introspect
```

### `peripheral` (feature-gated)

```bash
agentzero peripheral list
agentzero peripheral add --id sensor-1 --kind temperature --connection /dev/ttyUSB0
agentzero peripheral flash --id sensor-1 --firmware firmware.bin
```
