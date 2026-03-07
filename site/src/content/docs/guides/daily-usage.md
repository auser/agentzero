---
title: Daily Usage
description: Day-to-day commands and workflows for running, managing, and monitoring AgentZero.
---

This guide covers the commands and workflows you'll use every day once AgentZero is set up. For initial setup, see the [Quick Start](/quickstart/). For production hardening, see [Production Setup](/guides/production/). For the full command reference, see [CLI Commands](/reference/commands/).

---

## Getting Started Each Day

### Check system health

```bash
agentzero status                    # quick summary
agentzero daemon status             # if running as a daemon
agentzero doctor models             # verify provider connectivity
```

### Start the gateway (if not running as a service)

```bash
# Foreground (development)
agentzero gateway

# Background daemon (production)
agentzero daemon start
```

If you installed AgentZero as a system service, it starts automatically on boot — skip this step.

---

## Talking to the Agent

### Send a message

```bash
agentzero agent -m "List all TODO comments in the Rust files"
```

### Stream output as it arrives

```bash
agentzero agent -m "Summarize the README" --stream
```

### Override the model for a single request

```bash
agentzero agent -m "Quick question" --model gpt-4o-mini
agentzero agent -m "Complex analysis" --model anthropic/claude-sonnet-4-6
```

### Use a specific auth profile

```bash
agentzero agent -m "hello" --profile work-anthropic
```

### Use the gateway API

If the gateway is running, you can also interact via HTTP:

```bash
# Simple chat
curl -X POST https://agent.example.com/api/chat \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message": "What files changed today?"}'

# OpenAI-compatible endpoint (works with any OpenAI client library)
curl -X POST https://agent.example.com/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"model": "default", "messages": [{"role": "user", "content": "hello"}]}'
```

---

## Managing Conversations

### View recent memory

```bash
agentzero memory list                   # last 50 entries
agentzero memory list --limit 10        # last 10
agentzero memory list --offset 50 --limit 25  # paginate
```

### Look up a specific entry

```bash
agentzero memory get                    # most recent
agentzero memory get --key "project"    # by prefix match
```

### Check memory stats

```bash
agentzero memory stats
```

### Conversation branching

```bash
agentzero conversation list                  # list all conversations
agentzero conversation fork                  # fork current conversation into a new branch
agentzero conversation fork --name "explore" # fork with a custom name
agentzero conversation switch <id>           # switch to a different conversation
```

Forking is useful when you want to explore an alternative direction without losing the current thread.

### Clear old conversations

```bash
agentzero memory clear --key "old-session" --yes   # by prefix
agentzero memory clear --yes                        # everything (careful!)
```

---

## Configuration

### Inspect current config

```bash
agentzero config show              # secrets masked
agentzero config show --raw        # secrets visible
agentzero config get provider.model
```

### Change settings on the fly

```bash
agentzero config set provider.model "anthropic/claude-sonnet-4-6"
agentzero config set agent.max_tool_iterations 25
agentzero config set agent.parallel_tools true
```

Changes take effect on the next agent invocation. The daemon picks up config changes automatically.

---

## Authentication

### Set up a provider token

```bash
# Interactive (prompts for the key)
agentzero auth setup-token --provider openrouter

# Non-interactive
agentzero auth setup-token --provider anthropic --token sk-ant-...
```

### Manage multiple profiles

```bash
agentzero auth list                                          # all profiles
agentzero auth status                                        # active profile
agentzero auth use --provider anthropic --profile work       # switch
agentzero auth logout --provider openrouter                  # remove
```

### OAuth login (for providers that support it)

```bash
agentzero auth login --provider openai-codex    # opens browser
agentzero auth login --provider anthropic       # opens browser (claude.ai)
agentzero auth refresh --provider openai-codex  # refresh expired token
agentzero auth refresh --provider anthropic     # refresh expired token
```

---

## Models and Providers

### List providers

```bash
agentzero providers
```

### Browse available models

```bash
agentzero models list
agentzero models list --provider anthropic
agentzero models refresh --force        # update the cache
```

### Switch default model

```bash
agentzero models set gpt-4o
```

### Local models

```bash
agentzero local discover                # scan for Ollama, LM Studio, etc.
agentzero local health ollama           # check specific provider
agentzero models pull llama3.1:8b       # pull a model (Ollama)
```

---

## Tools

### See what's available

```bash
agentzero tools list                    # all registered tools
agentzero tools info read_file          # details on a specific tool
agentzero tools schema shell            # JSON schema for tool input
```

Tools are enabled/disabled via config. Key toggles:

| Tool | Config to enable |
|---|---|
| `write_file`, `file_edit` | `security.write_file.enabled = true` |
| `git_operations` | `security.enable_git = true` |
| `web_search` | `web_search.enabled = true` |
| `browser` | `browser.enabled = true` |
| `mcp_tool` | `security.mcp.enabled = true` |

---

## Scheduled Tasks

### Add a recurring task

```bash
# Standard cron format
agentzero cron add --id daily-report \
  --schedule "0 9 * * *" \
  --command "agentzero agent -m 'Generate daily summary'"

# Every N interval
agentzero cron add-every --id hourly-check \
  --schedule "1h" \
  --command "agentzero agent -m 'Check system status'"

# One-time future task
agentzero cron once --id reminder \
  --schedule "2026-03-05T14:00:00" \
  --command "agentzero agent -m 'Meeting reminder'"
```

### Manage tasks

```bash
agentzero cron list
agentzero cron pause --id daily-report
agentzero cron resume --id daily-report
agentzero cron update --id daily-report --schedule "0 10 * * *"
agentzero cron remove --id daily-report
```

---

## Monitoring

### Health and metrics

```bash
curl -s http://127.0.0.1:42617/health     # {"status":"ok"}
curl -s http://127.0.0.1:42617/metrics     # Prometheus format
```

### Daemon status

```bash
agentzero daemon status
agentzero daemon status --json    # PID, uptime, port, host
```

### Runtime traces

```bash
agentzero doctor traces                     # last 20 events
agentzero doctor traces --limit 5           # last 5
agentzero doctor traces --event "tool_call" # filter by type
agentzero doctor traces --contains "error"  # search text
```

### Cost tracking

```bash
agentzero cost status              # spending summary
agentzero cost status --json       # for dashboards
agentzero cost reset               # reset counters
```

### Logs

Daemon logs are at `{data_dir}/daemon.log` (auto-rotated at 10 MB, 5 files kept):

```bash
tail -f ~/.agentzero/daemon.log
```

---

## Emergency Operations

### Stop all agent activity

```bash
agentzero estop                             # full stop (kill-all)
agentzero estop --level network-kill        # cut network only
agentzero estop --level tool-freeze --tool shell --tool write_file  # freeze specific tools
```

### Check and resume

```bash
agentzero estop status
agentzero estop resume                      # resume everything
agentzero estop resume --network            # resume network only
agentzero estop resume --otp 123456         # if OTP required
```

---

## Channels (Telegram, Discord, Slack)

### Set up a channel

```bash
agentzero channel add telegram      # interactive setup
agentzero channel list              # show configured channels
agentzero channel doctor            # run diagnostics
```

### Start channel listeners

```bash
agentzero channel start             # launch all configured channels
```

### Remove a channel

```bash
agentzero channel remove telegram
```

---

## Updates

```bash
agentzero update check              # check for new versions
agentzero update apply              # install latest
agentzero update apply --version 0.3.0  # install specific version
agentzero update rollback           # roll back if something breaks
```

---

## Debugging

### Increase verbosity

```bash
agentzero -v agent -m "test"        # errors only
agentzero -vv agent -m "test"       # info
agentzero -vvv agent -m "test"      # debug
agentzero -vvvv agent -m "test"     # trace (very verbose)
```

### JSON output for scripting

Every command supports `--json`:

```bash
agentzero daemon status --json
agentzero memory list --json
agentzero auth list --json
agentzero cost status --json
```

### Run diagnostics

```bash
agentzero doctor models             # probe all providers
agentzero doctor traces --limit 10  # recent runtime events
agentzero channel doctor            # channel connectivity
```

### Shell completions

```bash
# Add to your shell profile for tab completion
agentzero completions --shell zsh >> ~/.zshrc
agentzero completions --shell bash >> ~/.bashrc
agentzero completions --shell fish >> ~/.config/fish/config.fish
```

---

## Quick Reference

| What you want to do | Command |
|---|---|
| Send a message | `agentzero agent -m "..."` |
| Stream response | `agentzero agent -m "..." --stream` |
| Check health | `agentzero status` |
| Start the server | `agentzero daemon start` |
| Stop the server | `agentzero daemon stop` |
| View config | `agentzero config show` |
| Change a setting | `agentzero config set key value` |
| List tools | `agentzero tools list` |
| View memory | `agentzero memory list` |
| Clear memory | `agentzero memory clear --yes` |
| Switch model | `agentzero models set model-name` |
| Add auth token | `agentzero auth setup-token --provider name` |
| Schedule a task | `agentzero cron add --id name --schedule "..." --command "..."` |
| Emergency stop | `agentzero estop` |
| Check costs | `agentzero cost status` |
| View traces | `agentzero doctor traces` |
| Update AgentZero | `agentzero update apply` |
