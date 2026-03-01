---
title: Quick Start
description: Get AgentZero running in under 5 minutes — from build to first agent message.
---

This guide gets you from zero to a working agent in under 5 minutes.

## 1. Build

```bash
cargo build -p agentzero --release
```

## 2. Run Onboarding

The onboarding wizard creates your `agentzero.toml` config file:

```bash
cargo run -p agentzero -- onboard --interactive
```

This walks you through:
- Choosing a provider (OpenRouter, OpenAI, Anthropic, Ollama)
- Setting your API key
- Configuring memory backend (SQLite default)
- Setting security policy (allowed commands, filesystem scope)

For non-interactive setup with explicit flags:

```bash
cargo run -p agentzero -- onboard \
  --provider openrouter \
  --model anthropic/claude-sonnet-4-6 \
  --memory sqlite \
  --allowed-root . \
  --allowed-commands ls,pwd,cat,echo \
  --yes
```

## 3. Set Your API Key

```bash
export OPENAI_API_KEY="sk-..."
```

Or for OpenRouter:

```bash
export OPENAI_API_KEY="sk-or-v1-..."
```

## 4. Send a Message

```bash
cargo run -p agentzero -- agent -m "What is the capital of France?"
```

## 5. Explore Commands

```bash
# System health check
cargo run -p agentzero -- status

# Diagnostics
cargo run -p agentzero -- doctor models

# List providers
cargo run -p agentzero -- providers

# View config
cargo run -p agentzero -- config show

# Start HTTP gateway
cargo run -p agentzero -- gateway

# Start daemon
cargo run -p agentzero -- daemon
```

## 6. Quality Gates

Run the full quality check suite before committing changes:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p agentzero --release
```

## Common Workflows

### Agent Mode

Send a single message through the agent loop:

```bash
agentzero agent -m "List files in the current directory"
```

### Gateway Mode

Start the HTTP gateway for programmatic access:

```bash
agentzero gateway --host 127.0.0.1 --port 8080
```

Then interact via HTTP:

```bash
# Health check
curl http://127.0.0.1:8080/health

# Pair (get bearer token)
curl -X POST http://127.0.0.1:8080/v1/ping
```

### Service Mode

Install as a system service (systemd or OpenRC):

```bash
agentzero service install
agentzero service start
agentzero service status
```

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `missing OPENAI_API_KEY` | API key not set | `export OPENAI_API_KEY="your_key"` |
| `config file not found` | Missing `agentzero.toml` | Run `agentzero onboard` |
| Tool execution denied | Security defaults are fail-closed | Edit `[security]` in `agentzero.toml` |
| Provider timeout | Network or rate limit issue | Check `-vvv` debug logs |

```bash
# Debug with verbose output
cargo run -p agentzero -- -vvv agent -m "test"

# Run diagnostics
cargo run -p agentzero -- doctor models
```

## Next Steps

- [Configuration Reference](/agentzero/config/reference/) — Full annotated `agentzero.toml`
- [CLI Commands](/agentzero/reference/commands/) — Complete command reference
- [Architecture](/agentzero/architecture/) — How it all fits together
