# 1-Click Business Office

A complete AI business office powered by AgentZero's swarm system. Seven specialized agents handle everything from strategy to security, communicating through an event bus and executing multi-step workflows via pipelines.

## Architecture

```
                         ┌─────────────────────┐
                         │   Incoming Message   │
                         │  (Telegram / Slack)  │
                         └─────────┬───────────┘
                                   │
                         ┌─────────▼───────────┐
                         │     AI Router        │
                         │  (Claude Haiku 4.5)  │
                         │  + keyword fallback  │
                         └─────────┬───────────┘
                                   │
            ┌──────────────────────┼──────────────────────┐
            │                      │                      │
   ┌────────▼────────┐   ┌────────▼────────┐   ┌────────▼────────┐
   │      CEO        │   │      CTO        │   │   Marketing     │
   │  (Opus 4.6)     │   │  (Sonnet 4.6)   │   │  (Sonnet 4.6)   │
   │  orchestrate    │   │  code + infra   │   │  content + SEO  │
   └────────┬────────┘   └─────────────────┘   └─────────────────┘
            │
   ┌────────▼─────────────────────────────────────────────┐
   │              Event Bus (pub/sub)                     │
   │  topics: task.ceo.directive, task.cto.complete, ...  │
   └──────────────────────────────────────────────────────┘
            │
   ┌────────┼──────────┬──────────┬──────────┐
   ▼        ▼          ▼          ▼          ▼
  CSO    Legal     Finance      HR       Marketing
```

## Agents

| Agent | Model | Privacy | Role |
|-------|-------|---------|------|
| **CEO** | Opus 4.6 | encrypted | Strategic decisions, cross-functional delegation |
| **CTO** | Sonnet 4.6 | encrypted | Technical architecture, code, infrastructure |
| **CSO** | Sonnet 4.6 | local_only | Security audits, threat modeling, vulnerability review |
| **Marketing** | Sonnet 4.6 | encrypted | Content creation, campaigns, market research |
| **Legal** | Sonnet 4.6 | encrypted | Contracts, compliance, regulatory analysis |
| **Finance** | Sonnet 4.6 | encrypted | Budgeting, forecasting, expense tracking |
| **HR** | Sonnet 4.6 | local_only | Hiring, onboarding, team management |

## Pipelines

Pipelines are multi-step workflows triggered by keywords in messages:

| Pipeline | Trigger | Steps | Error Handling |
|----------|---------|-------|----------------|
| **Employee Onboarding** | "onboard", "new hire" | HR → CTO → Finance | abort on error |
| **Product Launch** | "launch", "release", "ship" | CTO → Marketing → Legal | skip on error |
| **Security Audit** | "security audit", "pentest" | CSO → CTO → Legal | abort on error |

## How It Works

1. **Message arrives** via a channel (Telegram, Slack, or the gateway API)
2. **AI Router** classifies the message and picks the best agent (or matches a pipeline trigger)
3. **Agent processes** the request using its allowed tools and system prompt
4. **Results flow** through the event bus — if another agent subscribes to the output topic, it picks up automatically
5. **Terminal output** (no more subscribers) routes back to the originating channel

### Privacy Boundaries

- **local_only**: CSO and HR — sensitive data never leaves the device
- **encrypted_only**: CEO, CTO, Marketing, Legal, Finance — cloud providers allowed through encrypted transport
- The event bus enforces boundaries: a `local_only` event cannot be consumed by an `encrypted_only` agent

### Tool Sandboxing

Each agent only has access to the tools listed in `allowed_tools`:
- CEO: delegation and IPC (coordinates, doesn't execute)
- CTO: full dev toolkit (shell, git, files, web)
- CSO: read-only analysis (files, search, shell)
- Marketing: research and writing (web search, files)
- HR: document management only (files, memory)

## Quick Start

```bash
# 1. Copy config
cp examples/business-office/agentzero.toml ./agentzero.toml

# 2. Set your API key
export AGENTZERO_API_KEY="your-openrouter-api-key"

# 3. (Optional) Enable a channel — edit agentzero.toml and uncomment [channels.telegram]
# export TELEGRAM_BOT_TOKEN="your-bot-token"

# 4. Start the office
agentzero gateway

# 5. Pair a client (in another terminal — use the pairing code shown at startup)
curl -X POST http://localhost:42617/pair -H "X-Pairing-Code: <code-from-startup>"
# Returns: {"paired":true,"token":"<your-bearer-token>"}

# 6. Send a message:
curl -X POST http://localhost:42617/api/chat \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <your-bearer-token>" \
  -d '{"message": "Draft a product launch plan for our new AI assistant"}'
```

## Customization

### Adding a new agent

Add a new `[swarm.agents.<id>]` section:

```toml
[swarm.agents.designer]
name = "Designer"
description = "UI/UX designer. Creates wireframes, design systems, and visual assets."
keywords = ["design", "ui", "ux", "wireframe", "mockup", "figma"]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
base_url = "https://openrouter.ai/api/v1"
privacy_boundary = "encrypted_only"
allowed_tools = ["write_file", "read_file", "web_search", "agents_ipc"]
subscribes_to = ["task.ceo.directive", "channel.*.message"]
produces = ["task.design.complete"]
max_iterations = 15
system_prompt = "You are the UI/UX Designer..."
```

### Adding a pipeline

```toml
[[swarm.pipelines]]
name = "design-review"
channel_reply = true
on_step_error = "skip"
step_timeout_secs = 120
steps = ["designer", "cto", "cso"]

[swarm.pipelines.trigger]
keywords = ["design review", "ux review"]
```

### Using different models per agent

Each agent can use a different provider and model. For example, use a local model for the CSO:

```toml
[swarm.agents.cso]
provider = "ollama"
model = "llama3"
base_url = "http://localhost:11434"
privacy_boundary = "local_only"
```

### Event-driven chaining

Agents can form automatic chains via `subscribes_to` and `produces`:

```
CEO publishes "task.ceo.directive"
  → CTO subscribes to "task.ceo.directive"
    → CTO publishes "task.cto.complete"
      → (no subscribers = terminal → reply to channel)
```

To add chaining, set one agent's `produces` topic as another agent's `subscribes_to`.
