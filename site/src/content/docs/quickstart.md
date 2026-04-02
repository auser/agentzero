---
title: Quick Start
description: Install AgentZero and talk to your first agent in under 60 seconds.
---

Three commands. That's it.

```bash
# 1. Install
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash

# 2. Set up (interactive wizard — picks your provider, model, everything)
agentzero onboard --interactive

# 3. Talk to your agent
agentzero agent -m "What can you do?"
```

Your agent is running with 58+ built-in tools, encrypted memory, and secure defaults.

---

## What just happened?

The `onboard` wizard created your config, set up your API key, and configured security — all in one step. Your agent can now read files, run shell commands, search the web, manage git repos, and chain tools automatically to complete multi-step tasks.

Try something real:

```bash
agentzero agent -m "Find all TODO comments in this project and summarize them"
```

---

## Stream output in real-time

```bash
agentzero agent --stream -m "Write a haiku about Rust"
```

---

## Use a local model (no API key needed)

**Option A: Candle (pure Rust, in-process — recommended)**

Build with the `candle` feature and set the provider:

```bash
cargo build -p agentzero --release --features candle
```

```toml
# agentzero.toml
[provider]
kind = "candle"
model = "qwen2.5-coder-3b"
```

On first run it auto-downloads the model (~2 GB) and tokenizer. No external server, no C++ compiler, no API key.

**Option B: Ollama (external server)**

```bash
ollama pull llama3.1:8b
agentzero onboard --provider ollama --model llama3.1:8b --yes
agentzero agent -m "Hello"
```

---

## Start the gateway server

Turn your agent into an API that any app can talk to:

```bash
agentzero gateway
```

Now you have a running server with OpenAI-compatible endpoints, WebSocket chat, interactive API docs at `/docs`, and Prometheus metrics — all out of the box.

```bash
# Test it
curl http://127.0.0.1:3000/health

# Chat via the API
curl -X POST http://127.0.0.1:3000/api/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "Hello!"}'
```

---

## Connect to Telegram, Discord, or Slack

Add a channel with one config block and restart:

```bash
agentzero config set channels.telegram.bot_token "YOUR_TOKEN"
agentzero gateway
```

Your agent is now live on Telegram. Same for Discord and Slack — see the [Channels guide](/guides/channels/) for details.

---

## Run it forever

```bash
# As a background daemon
agentzero daemon start

# Or as a system service (auto-starts on boot)
agentzero service install && agentzero service start
```

---

## Keep going

| Want to...                          | Go here                                        |
|------------------------------------|-------------------------------------------------|
| Set up an always-on bot            | [Always-On Agent in 5 Minutes](/guides/always-on/) |
| Connect MCP tools                  | [MCP Servers](/guides/mcp/)                     |
| Run multi-agent swarms             | [Multi-Agent Patterns](/guides/multi-agent/)    |
| Deploy to production               | [Production Setup](/guides/production/)         |
| Use the Python/TypeScript SDK      | [Client SDKs](/guides/sdks/)                    |
| Add WASM plugins                   | [Plugin Authoring](/guides/plugins/)            |
| See all CLI commands               | [CLI Reference](/reference/commands/)           |
| Customize everything               | [Configuration Reference](/config/reference/)   |

:::tip[Something not working?]
Run `agentzero doctor` — it checks your config, provider connectivity, model availability, and memory health in one shot.
:::
