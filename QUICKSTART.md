# Quick Start — Developer Edition

Get AgentZero running on your machine and start using it while you develop.

## 1. Build

```bash
cargo build -p agentzero --release --features candle
```

The `candle` feature enables local LLM inference via [Candle](https://github.com/huggingface/candle) (pure Rust, no C++ compiler needed). The binary lands at `./target/release/agentzero`. Optionally copy it somewhere on your PATH:

```bash
cp target/release/agentzero ~/.cargo/bin/
```

## 2. Configure

```bash
cp agentzero.dev.toml agentzero.toml
```

That's it. The dev config uses the `candle` provider which runs Qwen2.5-Coder-3B entirely in-process. On first run it auto-downloads the GGUF model (~2GB) and tokenizer to `~/.agentzero/models/`.

No API key. No external server. No cost.

## 3. First run

```bash
# Simple test
agentzero agent -m "What tools do you have available?"

# Something useful
agentzero agent -m "Find all TODO comments in crates/ and summarize them"

# Watch the tool loop in action
agentzero -vvv agent -m "Summarize the recent git log"
```

## 4. Switch to a cloud provider (optional)

For better results on complex tasks, swap in Anthropic or OpenRouter.

Edit `agentzero.toml` — change the `[provider]` section and set your API key:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
# or
export OPENAI_API_KEY="sk-or-v1-..."   # for OpenRouter
```

Or save it permanently:

```bash
agentzero auth setup-token --provider anthropic
```

## 5. Gateway

Turn the agent into an HTTP API:

```bash
agentzero gateway
```

Test it:

```bash
# Health check
curl http://127.0.0.1:42617/health

# Chat (after pairing — see the pairing code in gateway startup output)
TOKEN=$(curl -s -X POST http://localhost:42617/pair \
  -H "X-Pairing-Code: YOUR_CODE" | jq -r '.token')

curl -s -X POST http://localhost:42617/api/chat \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message": "Hello!"}' | jq .
```

The gateway also exposes an OpenAI-compatible endpoint at `/v1/chat/completions`, WebSocket at `/ws/chat`, and Prometheus metrics at `/metrics`.

## 6. Dashboard

Interactive TUI for monitoring the agent:

```bash
agentzero dashboard
```

## 7. Channel bot

Connect to Telegram, Discord, or Slack by adding a channel block to `agentzero.toml`:

```toml
[channels.telegram]
bot_token = "YOUR_TELEGRAM_BOT_TOKEN"
```

Then start the gateway (`agentzero gateway`) and the bot is live. See `site/src/content/docs/guides/channels.md` for full setup instructions.

## Troubleshooting

```bash
# Run diagnostics
agentzero doctor

# Check provider connectivity
agentzero doctor models

# Verbose output (up to -vvvv)
agentzero -vvv agent -m "test"

# List available tools
agentzero tools list

# Health check
agentzero status
```

## Further reading

- [Configuration reference](site/src/content/docs/config/reference.md)
- [First hour walkthrough](site/src/content/docs/guides/first-hour.md)
- [Multi-agent swarms](site/src/content/docs/guides/multi-agent.md)
- [Production deployment](site/src/content/docs/guides/production.md)
- [Plugin authoring](site/src/content/docs/guides/plugins.md)
- [Example configs](examples/)
