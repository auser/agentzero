# AgentZero Examples

## Configuration References

| File | Description |
|------|-------------|
| [config-basic.toml](config-basic.toml) | Minimal config — just provider, security, and gateway |
| [config-full.toml](config-full.toml) | Complete reference with every option documented |

## Use-Case Examples

| Example | Description |
|---------|-------------|
| [business-office/](business-office/) | 1-click AI business office — 7 agents (CEO, CTO, CSO, Marketing, Legal, Finance, HR) with automated pipelines for onboarding, launches, and security audits |
| [research-pipeline/](research-pipeline/) | Research-to-brief pipeline — 4 agents (Researcher, Scraper, Analyst, Writer) that turn any topic into a polished research brief |

## Quick Start

### 1. Pick a provider

```bash
# OpenRouter (access to all models)
agentzero onboard --provider openrouter --model anthropic/claude-sonnet-4-6 --yes
export OPENAI_API_KEY="sk-or-v1-..."

# Anthropic (direct)
agentzero onboard --provider anthropic --model claude-sonnet-4-6 --yes
export ANTHROPIC_API_KEY="sk-ant-..."

# OpenAI (direct)
agentzero onboard --provider openai --model gpt-4o --yes
export OPENAI_API_KEY="sk-..."

# Local model (no API key needed)
agentzero onboard --provider ollama --model llama3.1:8b --yes
```

### 2. Send a message

```bash
agentzero agent -m "Hello, what can you do?"
```

### 3. Use an example config

```bash
# Copy a use-case example
cp examples/business-office/agentzero.toml ./agentzero.toml

# Start the gateway
agentzero gateway

# In another terminal — pair a client (use the pairing code shown at startup)
curl -X POST http://localhost:42617/pair -H "X-Pairing-Code: <code-from-startup>"
# Returns: {"paired":true,"token":"<your-bearer-token>"}

# Send a message
curl -X POST http://localhost:42617/api/chat \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <your-bearer-token>" \
  -d '{"message": "Hello"}'
```

## Common Configurations

### Enable file writing

By default the agent can only read files. To let it write:

```toml
[security.write_file]
enabled = true
max_write_bytes = 65536
```

### Enable web search

```toml
[web_search]
enabled = true
provider = "duckduckgo"     # no API key needed
# provider = "brave"        # requires BRAVE_API_KEY
```

### Enable MCP tools

Define servers in `~/.agentzero/mcp.json` (global) or `.agentzero/mcp.json` (project):

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-filesystem", "/tmp"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "ghp_..." }
    }
  }
}
```

Then enable in `agentzero.toml`:

```toml
[security.mcp]
enabled = true
```

Each tool appears as `mcp__{server}__{tool}` (e.g. `mcp__filesystem__read_file`).

### Enable browser automation

```toml
[browser]
enabled = true
backend = "agent_browser"
```

Requires Node.js — Playwright + Chromium install automatically on first use.

### Configure delegation (multi-agent)

```toml
[agents.coder]
provider = "anthropic"
model = "claude-sonnet-4-6"
max_depth = 2
agentic = true
allowed_tools = ["shell", "read_file", "write_file", "file_edit"]

[agents.researcher]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
agentic = true
allowed_tools = ["web_search", "web_fetch", "read_file"]
```

### Add a channel integration

```toml
[channels.telegram]
bot_token = "YOUR_TELEGRAM_BOT_TOKEN"

[channels.slack]
bot_token = "xoxb-YOUR-SLACK-BOT-TOKEN"
app_token = "xapp-YOUR-SLACK-APP-TOKEN"

[channels.discord]
bot_token = "YOUR_DISCORD_BOT_TOKEN"
```

### Privacy mode (local only)

```toml
[privacy]
mode = "local_only"          # blocks all outbound network tools
```

### Cost tracking

```toml
[cost]
enabled = true
daily_limit_usd = 10.0
monthly_limit_usd = 100.0
warn_at_percent = 80
```

## Each example directory contains

- `agentzero.toml` — ready-to-use configuration
- `README.md` — architecture explanation and customization guide
- `.env.example` — environment variable template

## Browser Automation

Examples that use the `browser` tool (like research-pipeline) require Node.js and npm. Dependencies (Playwright + Chromium) install automatically on first use — no manual setup needed.

See [scripts/agent-browser/README.md](../scripts/agent-browser/README.md) for details.
