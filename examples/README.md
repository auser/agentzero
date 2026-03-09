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

```bash
# Pick an example
cp examples/business-office/agentzero.toml ./agentzero.toml

# Set your API key
export AGENTZERO_API_KEY="your-key"

# Start
agentzero gateway

# Pair a client (in another terminal — use the pairing code shown at startup)
curl -X POST http://localhost:42617/pair -H "X-Pairing-Code: <code-from-startup>"
# Returns: {"paired":true,"token":"<your-bearer-token>"}

# Send a message
curl -X POST http://localhost:42617/api/chat \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <your-bearer-token>" \
  -d '{"message": "Hello"}'
```

Each example directory contains:
- `agentzero.toml` — ready-to-use configuration
- `README.md` — architecture explanation and customization guide
- `.env.example` — environment variable template

## Browser Automation

Examples that use the `browser` tool (like research-pipeline) require Node.js and npm. Dependencies (Playwright + Chromium) install automatically on first use — no manual setup needed.

See [scripts/agent-browser/README.md](../scripts/agent-browser/README.md) for details.
