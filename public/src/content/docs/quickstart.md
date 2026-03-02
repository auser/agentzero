---
title: Quick Start
description: Get AgentZero installed, configured, and running — from first install to production deployment.
---

This guide walks you through every step to get AgentZero running, from installation through production deployment.

## Prerequisites

You need **one** of the following:

- **curl + bash** — to install a pre-built binary (Linux, macOS, WSL)
- **Rust 1.80+** — to build from source (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

You also need an API key from at least one AI provider (OpenRouter, OpenAI, Anthropic, etc.). If you want to use a local model (Ollama, LM Studio), no API key is needed.

---

## 1. Install

### Option A: Pre-built binary (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash
```

This auto-detects your platform and installs to `~/.cargo/bin` (or `~/.local/bin`). Options:

```bash
# Install a specific version
curl -fsSL ... | bash -s -- --version 0.2.0

# Install to a custom directory
curl -fsSL ... | bash -s -- --dir /usr/local/bin

# Install shell completions (bash, zsh, fish)
curl -fsSL ... | bash -s -- --completions zsh
```

### Option B: Build from source

```bash
git clone https://github.com/auser/agentzero.git
cd agentzero
cargo build -p agentzero --release
```

The binary is at `target/release/agentzero`. Move it somewhere on your `$PATH`:

```bash
cp target/release/agentzero ~/.cargo/bin/
```

### Verify installation

```bash
agentzero --help
```

---

## 2. Configure

The `onboard` command creates your `agentzero.toml` config file in the current directory.

### Interactive wizard

```bash
agentzero onboard --interactive
```

This walks you through choosing a provider, model, memory backend, and security settings.

### Non-interactive (scripted)

```bash
agentzero onboard \
  --provider openrouter \
  --model anthropic/claude-sonnet-4-6 \
  --memory sqlite \
  --allowed-root . \
  --allowed-commands ls,pwd,cat,echo \
  --yes
```

The generated `agentzero.toml` contains three required sections:

```toml
[provider]
kind = "openrouter"
base_url = "https://openrouter.ai/api/v1"
model = "anthropic/claude-sonnet-4-6"

[memory]
backend = "sqlite"
sqlite_path = "./agentzero.db"

[security]
allowed_root = "."
allowed_commands = ["ls", "pwd", "cat", "echo"]
```

To view or modify your config later:

```bash
agentzero config show          # View effective config
agentzero config get provider.model   # Get a single value
agentzero config set provider.model gpt-4o  # Change a value
```

---

## 3. Authenticate

You need to give AgentZero credentials for your AI provider. Pick **any one** of the methods below — they all work equally well.

### Option A: `auth` command (recommended)

The built-in auth store saves your credentials securely and supports multiple providers and profiles:

```bash
# Paste an API key (prompts interactively if --token is omitted)
agentzero auth setup-token --provider openrouter
agentzero auth setup-token --provider anthropic --token sk-ant-...
agentzero auth setup-token --provider openai --token sk-...

# Or use OAuth login (for providers that support it)
agentzero auth login --provider openai-codex
```

Manage saved profiles:

```bash
agentzero auth list                          # List all profiles
agentzero auth status                        # Show active profile
agentzero auth use --provider anthropic --profile default  # Switch active profile
agentzero auth logout --provider openrouter  # Remove a profile
```

### Option B: Environment variable

Set `OPENAI_API_KEY` — this is the universal env var name for **all** cloud providers (OpenAI, OpenRouter, Anthropic, etc.):

```bash
export OPENAI_API_KEY="sk-..."       # OpenAI
export OPENAI_API_KEY="sk-or-v1-..." # OpenRouter
export OPENAI_API_KEY="sk-ant-..."   # Anthropic
```

### Option C: `.env` file

Create a `.env` file in the same directory as your `agentzero.toml`:

```bash
OPENAI_API_KEY=sk-or-v1-...
```

AgentZero loads `.env` and `.env.local` automatically.

### Local models (no credentials needed)

If you're using Ollama or another local provider, skip this step entirely:

```bash
agentzero onboard --provider ollama --model llama3.1:8b --yes
agentzero agent -m "Hello"
```

---

## 4. Send Your First Message

```bash
agentzero agent -m "What is the capital of France?"
```

Override the provider or model for a single request:

```bash
agentzero agent -m "Hello" --provider openai --model gpt-4o-mini
agentzero agent -m "Hello" --profile my-anthropic-profile
```

---

## 5. Verify Everything Works

```bash
# Quick status check (shows memory count)
agentzero status

# Diagnose model availability across providers
agentzero doctor models

# List supported providers
agentzero providers

# Refresh and list available models
agentzero models refresh
agentzero models list
```

---

## 6. Running in Production

AgentZero offers three ways to run as a server, from simple to fully managed:

### Gateway (foreground)

Start the HTTP gateway directly. Good for testing and development:

```bash
agentzero gateway --host 127.0.0.1 --port 8080
```

The gateway exposes these endpoints:

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check |
| `/api/chat` | POST | Send a chat message |
| `/v1/chat/completions` | POST | OpenAI-compatible completions API |
| `/v1/models` | GET | List available models |
| `/ws/chat` | GET | WebSocket chat |
| `/pair` | POST | Pair a client (get bearer token) |
| `/v1/ping` | POST | Connectivity check |
| `/metrics` | GET | Prometheus-style metrics |

Test it:

```bash
# Health check
curl http://127.0.0.1:8080/health

# Pair (get a bearer token for subsequent requests)
curl -X POST http://127.0.0.1:8080/pair
```

### Daemon (background process)

Run the gateway as a background daemon. Logs to `daemon.log` in your data directory:

```bash
# Start the daemon
agentzero daemon start --port 8080

# Check if it's running
agentzero daemon status

# View logs
agentzero daemon status --json   # shows log file path

# Stop the daemon
agentzero daemon stop
```

For debugging, run the daemon in the foreground:

```bash
agentzero daemon start --foreground
```

### Service (auto-start on boot)

Install AgentZero as a system service (systemd or OpenRC, auto-detected):

```bash
# Install and start
agentzero service install
agentzero service start

# Check status
agentzero service status

# Restart after config changes
agentzero service restart

# Remove completely
agentzero service stop
agentzero service uninstall
```

Force a specific init system:

```bash
agentzero service install --service-init systemd
agentzero service install --service-init openrc
```

---

## 7. Explore More Commands

```bash
# Interactive terminal dashboard
agentzero dashboard

# Manage scheduled tasks
agentzero cron list
agentzero cron add --id daily-check --schedule "0 9 * * *" --command "agentzero agent -m 'morning report'"

# Memory inspection
agentzero memory list
agentzero memory stats

# Shell completions
agentzero completions --shell zsh >> ~/.zshrc
agentzero completions --shell bash >> ~/.bashrc

# Check for updates
agentzero update check
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `missing API key for provider 'X'` | API key not set | `export OPENAI_API_KEY="your_key"` or `agentzero auth login` |
| `config file not found` | No `agentzero.toml` in current dir | Run `agentzero onboard` |
| Tool execution denied | Security defaults are fail-closed | Edit `[security]` in `agentzero.toml` |
| Provider timeout | Network or rate limit issue | Check with `-vvv` for debug logs |
| Daemon won't start | Port already in use | `agentzero daemon stop` then retry, or use a different `--port` |

Debug with verbose output:

```bash
# Increasing verbosity: -v (error) → -vv (info) → -vvv (debug) → -vvvv (trace)
agentzero -vvv agent -m "test"

# Run full model diagnostics
agentzero doctor models

# View recent trace events
agentzero doctor traces --limit 10
```

---

## Next Steps

- [Configuration Reference](/agentzero/config/reference/) — Full annotated `agentzero.toml`
- [CLI Commands](/agentzero/reference/commands/) — Complete command reference
- [Architecture](/agentzero/architecture/) — How it all fits together
