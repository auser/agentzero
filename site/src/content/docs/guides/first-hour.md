---
title: Your First Hour with AgentZero
description: Go from zero to running a CLI agent, HTTP gateway, and multi-agent swarm in under an hour.
---

A hands-on walkthrough that gets all three use cases running: CLI agent, HTTP API, and multi-agent swarm. Every command is copy-pasteable. For deeper explanations, see the [Quick Start](/quickstart/).

## Prerequisites

- An API key from [OpenRouter](https://openrouter.ai/), [Anthropic](https://console.anthropic.com/), or [OpenAI](https://platform.openai.com/)
- Rust 1.80+ (to build from source) or curl (to install a pre-built binary)

---

## Part 1: CLI Agent

### Install

```bash
# Option A: pre-built binary
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash

# Option B: build from source
git clone https://github.com/auser/agentzero.git
cd agentzero
cargo build -p agentzero --release
cp target/release/agentzero ~/.cargo/bin/
```

### Configure

```bash
agentzero onboard --interactive
```

Or skip the wizard:

```bash
agentzero onboard \
  --provider openrouter \
  --model anthropic/claude-sonnet-4-6 \
  --memory sqlite \
  --yes
```

### Set your API key

```bash
# Pick one:
export OPENAI_API_KEY="sk-or-v1-..."   # OpenRouter
export OPENAI_API_KEY="sk-ant-..."     # Anthropic
export OPENAI_API_KEY="sk-..."         # OpenAI

# Or save it permanently:
agentzero auth setup-token --provider openrouter
```

### Send your first message

```bash
agentzero agent -m "What tools do you have available?"
```

### Try a multi-step task

The agent chains tools automatically — file search, content grep, shell commands:

```bash
agentzero agent -m "Find all Rust files containing TODO comments and list them with line numbers"
```

Watch it work with debug output:

```bash
agentzero -vvv agent -m "Summarize the README in this directory"
```

### Verify

```bash
agentzero status          # health check
agentzero doctor models   # verify provider connectivity
agentzero tools list      # see all available tools
```

---

## Part 2: HTTP Gateway

### Start the gateway

```bash
agentzero gateway
```

You'll see a pairing code printed at startup. Note it down.

### Pair a client

In a second terminal:

```bash
# Exchange the pairing code for a bearer token
TOKEN=$(curl -s -X POST http://localhost:42617/pair \
  -H "X-Pairing-Code: YOUR_PAIRING_CODE" | jq -r '.token')

echo $TOKEN
```

### Send a chat message

```bash
curl -s -X POST http://localhost:42617/api/chat \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message": "What is the capital of France?"}' | jq .
```

### Use the OpenAI-compatible endpoint

Works with any OpenAI client library:

```bash
curl -s -X POST http://localhost:42617/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "default",
    "messages": [{"role": "user", "content": "Hello!"}]
  }' | jq .
```

### Submit an async job

For long-running tasks, submit a job and poll for results:

```bash
# Submit
RUN_ID=$(curl -s -X POST http://localhost:42617/v1/runs \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message": "Research the top 5 Rust web frameworks and compare them"}' | jq -r '.id')

# Poll for status
curl -s http://localhost:42617/v1/runs/$RUN_ID \
  -H "Authorization: Bearer $TOKEN" | jq .status

# Get the result when complete
curl -s http://localhost:42617/v1/runs/$RUN_ID/result \
  -H "Authorization: Bearer $TOKEN" | jq .
```

### Key endpoints

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check (no auth) |
| `/pair` | POST | Exchange pairing code for token |
| `/api/chat` | POST | Synchronous chat |
| `/v1/chat/completions` | POST | OpenAI-compatible |
| `/v1/runs` | POST | Async job submission |
| `/v1/runs/:id` | GET | Job status |
| `/v1/runs/:id/result` | GET | Job result |
| `/v1/runs/:id/stream` | GET | SSE event stream |
| `/ws/chat` | GET | WebSocket chat |
| `/metrics` | GET | Prometheus metrics |

Stop the gateway with `Ctrl+C`, then continue to Part 3.

---

## Part 3: Multi-Agent Swarm

### Copy the example config

The business-office example has 7 agents (CEO, CTO, CSO, Marketing, Legal, Finance, HR) and 3 pipelines pre-configured:

```bash
mkdir -p ~/agentzero-swarm && cd ~/agentzero-swarm
cp /path/to/agentzero/examples/business-office/agentzero.toml .
```

Or if you cloned the repo:

```bash
mkdir -p ~/agentzero-swarm && cd ~/agentzero-swarm
cp ~/agentzero/examples/business-office/agentzero.toml .
```

### Set the API key

The example uses OpenRouter by default:

```bash
export AGENTZERO_API_KEY="sk-or-v1-..."
```

### Start the swarm

```bash
agentzero gateway
```

Pair a client (same as Part 2), then test message routing:

### Test agent routing

Messages are automatically classified and routed to the right agent:

```bash
# Routes to CTO (technical keywords)
curl -s -X POST http://localhost:42617/api/chat \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message": "Review the architecture of our API layer"}' | jq .

# Routes to CSO (security keywords)
curl -s -X POST http://localhost:42617/api/chat \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message": "Audit our authentication for vulnerabilities"}' | jq .

# Routes to Marketing
curl -s -X POST http://localhost:42617/api/chat \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message": "Draft a launch campaign for our new product"}' | jq .
```

### Trigger a pipeline

Pipelines chain agents sequentially. The example has three built-in:

```bash
# Employee Onboarding: HR → CTO → Finance
curl -s -X POST http://localhost:42617/api/chat \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message": "Onboard a new frontend engineer starting next Monday"}' | jq .

# Product Launch: CTO → Marketing → Legal
curl -s -X POST http://localhost:42617/api/chat \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message": "Launch the new API v2 publicly"}' | jq .

# Security Audit: CSO → CTO → Legal
curl -s -X POST http://localhost:42617/api/chat \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message": "Run a security audit on our payment processing"}' | jq .
```

### Customize the swarm

Edit `agentzero.toml` to add your own agents:

```toml
[swarm.agents.devops]
name = "DevOps"
description = "Handles deployments, CI/CD, infrastructure, and monitoring."
keywords = ["deploy", "ci", "cd", "infrastructure", "docker", "kubernetes", "monitoring"]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
base_url = "https://openrouter.ai/api/v1"
allowed_tools = ["shell", "read_file", "write_file", "web_search"]
subscribes_to = ["task.ceo.directive", "channel.*.message"]
produces = ["task.devops.complete"]
max_iterations = 20
system_prompt = """You are the DevOps engineer. You handle deployments,
CI/CD pipelines, infrastructure provisioning, and monitoring setup."""
```

Add a pipeline that includes your new agent:

```toml
[[swarm.pipelines]]
name = "deploy-and-verify"
channel_reply = true
on_step_error = "abort"
steps = ["cto", "devops"]

[swarm.pipelines.trigger]
keywords = ["deploy", "ship to production", "release to prod"]
```

---

## What's Next

| Want to... | Go to |
|---|---|
| Learn day-to-day commands | [Daily Usage](/guides/daily-usage/) |
| Set up Telegram/Discord/Slack | [Channels](/guides/channels/) |
| Deep dive into swarm patterns | [Multi-Agent Patterns](/guides/multi-agent/) |
| Harden for production | [Production Setup](/guides/production/) |
| Deploy behind nginx/Caddy/Docker | [Gateway Deployment](/guides/deployment/) |
| Add MCP tool servers | [MCP Servers](/guides/mcp/) |
| See every config option | [Config Reference](/config/reference/) |
| Browse all CLI commands | [CLI Commands](/reference/commands/) |
