---
title: Always-On Agent in 5 Minutes
description: Go from zero to an AI agent that responds on Telegram, Discord, or Slack — with tools enabled.
---

Get an always-on AI agent responding on a messaging platform with file access, shell commands, and web search. Total time: about 5 minutes.

## 1. Install

```bash
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash
```

## 2. Get a Telegram bot token

Open Telegram, message [@BotFather](https://t.me/BotFather), send `/newbot`, follow the prompts. Copy the bot token it gives you.

## 3. Set your API key

[OpenRouter](https://openrouter.ai/) gives you access to all models with one key:

```bash
export OPENAI_API_KEY="sk-or-v1-..."
```

Or save it permanently:

```bash
echo 'OPENAI_API_KEY=sk-or-v1-...' >> ~/.agentzero/.env
```

## 4. Create the config

Save this as `agentzero.toml` in your working directory:

```toml
[provider]
kind = "openrouter"
model = "anthropic/claude-sonnet-4-6"

[security]
allowed_root = "."
allowed_commands = ["ls", "pwd", "cat", "echo", "grep", "find", "git", "cargo", "python3", "node"]
enable_git = true

[security.write_file]
enabled = true

[web_search]
enabled = true
provider = "duckduckgo"

[channels.telegram]
bot_token = "YOUR_TELEGRAM_BOT_TOKEN"

[gateway]
port = 42617
```

Replace `YOUR_TELEGRAM_BOT_TOKEN` with the token from step 2.

## 5. Start the agent

```bash
agentzero daemon start
```

Check it's running:

```bash
agentzero daemon status
```

## 6. Message your bot

Open Telegram, find your bot, send a message. The agent will respond with full tool access — it can read files, run commands, search the web, and write files in your working directory.

---

## Use Discord or Slack instead

Replace the `[channels.telegram]` block with one of these:

### Discord

```toml
[channels.discord]
bot_token = "YOUR_DISCORD_BOT_TOKEN"
```

Create a bot at the [Discord Developer Portal](https://discord.com/developers/applications), enable the Message Content intent, and invite it to your server.

### Slack

```toml
[channels.slack]
bot_token = "xoxb-YOUR-SLACK-BOT-TOKEN"
app_token = "xapp-YOUR-SLACK-APP-TOKEN"
```

Create a Slack app at [api.slack.com/apps](https://api.slack.com/apps) with Socket Mode enabled.

---

## Use a local model (no API key)

Swap the provider section and drop the API key:

```toml
[provider]
kind = "ollama"
model = "llama3.1:8b"
```

Make sure [Ollama](https://ollama.ai/) is running locally first (`ollama serve`).

---

## Enable more tools

Add any of these to your `agentzero.toml`:

```toml
# Browser automation
[browser]
enabled = true

# MCP servers (define servers in ~/.agentzero/mcp.json)
[security.mcp]
enabled = true

# HTTP requests
[http_request]
enabled = true
```

---

## Next steps

- Run as a system service that survives reboots: `agentzero service install && agentzero service start`
- Add [delegate agents](/agentzero/guides/multi-agent/) for specialized tasks
- Set up [scheduled tasks](/agentzero/reference/commands/) with `agentzero cron add`
- See the full [Configuration Reference](/agentzero/config/reference/) for all options
