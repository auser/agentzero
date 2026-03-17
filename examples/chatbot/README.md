# Chatbot Example

The simplest possible AgentZero setup: a single agent with basic tools and minimal configuration.

## Architecture

```
User --> agentzero agent --> LLM Provider --> Tool execution --> Response
```

One agent, one provider, read-only tools. No multi-agent orchestration, no channels, no gateway.

## Setup

1. Set your API key:
   ```bash
   export ANTHROPIC_API_KEY="sk-..."
   # Or: export OPENAI_API_KEY="sk-..."
   # Or: export OPENROUTER_API_KEY="sk-..."
   ```

2. Run a single message:
   ```bash
   agentzero agent -m "List the files in this directory" --config examples/chatbot/config.toml
   ```

3. Or use zero-config mode (no config file needed):
   ```bash
   agentzero run "List the files in this directory"
   ```

## What's included

- `config.toml` — Minimal 4-section config (provider, memory, agent, security)
- Read-only shell commands (ls, cat, grep, etc.)
- SQLite conversation memory
- No write access, no network tools, no channels

## Next steps

- Add write tools: set `security.write_file.enabled = true` in config
- Add git: set `security.git.enabled = true`
- Add web search: set `web_search.enabled = true`
- Try multi-agent: see `../multi-agent-team/`
- Deploy as a service: see `../business-office/`
