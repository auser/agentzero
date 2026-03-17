# Edge Deployment Example

Lightweight AgentZero configuration for resource-constrained devices like Raspberry Pi.

## Architecture

```
[Edge Device]                    [Cloud/Server]
  AgentZero                       LLM Provider
  (minimal tools)  ----HTTP---->  (API endpoint)
  SQLite memory
  Gateway :8080
```

## Design principles

- **Small footprint**: Use the cheapest, fastest model (Haiku)
- **Minimal tools**: Only basic shell commands for system monitoring
- **Cost controls**: Daily and monthly budget caps
- **Production mode**: Strict security, no write access
- **Low memory window**: Keep context small for fast responses

## Setup

1. Install AgentZero on your device:
   ```bash
   curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash
   ```

2. Set your API key:
   ```bash
   export OPENROUTER_API_KEY="sk-..."
   ```

3. Start the gateway:
   ```bash
   agentzero gateway --config examples/edge-deployment/config.toml
   ```

4. Query from another device:
   ```bash
   curl http://device-ip:8080/api/chat \
     -H "Content-Type: application/json" \
     -d '{"message": "What is the system uptime and disk usage?"}'
   ```

## Cost management

The config enforces:
- $1/day maximum spend
- $10/month maximum spend
- Warning at 80% of budget

Adjust in `[cost]` section as needed.

## Next steps

- Add Telegram alerts: install the `telegram-bot` skill
- Monitor multiple devices: use the gossip event bus for coordination
- Add cron health checks: `agentzero cron add "*/5 * * * *" "check system health"`
