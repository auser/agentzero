# Research-to-Brief Pipeline

A multi-step AI pipeline that turns any topic into a polished research brief. Four specialized agents work in sequence, each using a model optimized for its role. Everything is controlled by a single `agentzero.toml` config file.

## Architecture

```
  POST /api/chat  "Research AI regulation in the EU"
         │
         ▼
  ┌─────────────────┐
  │  GatewayChannel  │   Bridges HTTP API into the swarm event bus
  └────────┬────────┘
           │
  ┌────────▼────────┐
  │  Router          │   Matches pipeline trigger keywords/regex
  └────────┬────────┘
           │
           ▼
  ┌──────────────────────────────────────────────────┐
  │  Pipeline: research-to-brief (sequential steps)  │
  │                                                  │
  │  ┌──────────────┐                                │
  │  │  Researcher   │  Haiku 4.5 (fast, cheap)      │
  │  │  web_search   │  → research/raw-findings.md   │
  │  └──────┬───────┘                                │
  │         │                                        │
  │  ┌──────▼───────┐                                │
  │  │  Scraper      │  Haiku 4.5 (fast)             │
  │  │  browser      │  → research/detailed-data.md  │
  │  └──────┬───────┘  (skipped if browser absent)   │
  │         │                                        │
  │  ┌──────▼───────┐                                │
  │  │  Analyst      │  Sonnet 4.6 (powerful)        │
  │  │  synthesize   │  → research/analysis.md       │
  │  └──────┬───────┘                                │
  │         │                                        │
  │  ┌──────▼───────┐                                │
  │  │  Writer       │  Sonnet 4.6 (powerful)        │
  │  │  write brief  │  → output/brief.md            │
  │  └──────────────┘                                │
  └──────────────────────────────────────────────────┘
           │
           ▼
  HTTP response with final brief
```

## How It Works

1. A message hits the gateway (`/api/chat`, `/v1/chat/completions`, or `/v1/runs`)
2. The `GatewayChannel` bridges it into the swarm's event bus
3. The router matches the message against pipeline triggers (keywords + regex)
4. The pipeline executor runs each step sequentially, passing output as the next step's input
5. The final output is sent back to the API caller via `channel_reply = true`

Each agent is a full AgentZero agent with its own model, tools, and system prompt — defined entirely in config.

### Model Strategy

| Agent | Model | Why |
|-------|-------|-----|
| Researcher | Haiku 4.5 | Fast + cheap — broad search doesn't need deep reasoning |
| Scraper | Haiku 4.5 | Fast — extraction is mechanical, not analytical |
| Analyst | Sonnet 4.6 | Powerful — synthesis requires deep reasoning |
| Writer | Sonnet 4.6 | Powerful — polished writing requires nuance |

### Error Handling

The pipeline uses `on_step_error = "skip"` so that if the Scraper fails (e.g., `agent-browser` not installed), the pipeline continues with the Analyst using just the raw findings. This makes the browser step optional.

Other error strategies: `"abort"` (stop pipeline on failure) and `"retry"` (retry failed step up to `max_retries` times).

## Quick Start

### 1. Authenticate

```bash
# OAuth (opens browser)
agentzero auth login --provider anthropic

# Or use an API key directly
export ANTHROPIC_API_KEY="sk-ant-api03-..."
```

### 2. Start the gateway

Pick one:

```bash
# Point --config at the example config
agentzero gateway --config examples/research-pipeline/agentzero.toml

# Or run as a background daemon
agentzero daemon start --config examples/research-pipeline/agentzero.toml

# Or copy config to default location and run without --config
cp examples/research-pipeline/agentzero.toml ~/.agentzero/agentzero.toml
agentzero gateway

# Or set the config path via environment variable
export AGENTZERO_CONFIG=examples/research-pipeline/agentzero.toml
agentzero gateway
```

Use `--new-pairing` to clear existing paired tokens and generate a fresh pairing code.

### 3. Pair a client

Use the pairing code shown at startup:

```bash
curl -X POST http://localhost:42617/pair -H "X-Pairing-Code: <code-from-startup>"
# Returns: {"paired":true,"token":"<your-bearer-token>"}

export TOKEN="<your-bearer-token>"
```

Or as a variable:

```bash
# Fill in CODE, get TOKEN
TOKEN=$(curl -X POST http://localhost:42617/pair -H "X-Pairing-Code: <code>" | jq -r .token)
```

### 4. Send a research request

There are several ways to interact with the pipeline.

#### Synchronous — `/api/chat`

Blocks until the full pipeline completes and returns the final brief:

```bash
curl -X POST http://localhost:42617/api/chat \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"message": "Research the current state of AI regulation in the EU"}'
```

#### Synchronous — `/v1/chat/completions` (OpenAI-compatible)

Same behavior, OpenAI-compatible request/response format:

```bash
curl -X POST http://localhost:42617/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Research quantum computing breakthroughs in 2025"}]
  }'
```

#### Async — `/v1/runs` (fire-and-forget with polling)

Submit a job and poll for the result. Useful for long-running research that might exceed HTTP timeouts:

```bash
# Submit
curl -X POST http://localhost:42617/v1/runs \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"message": "Research the global semiconductor supply chain"}'
# Returns: {"run_id": "run-...", "accepted_at": "..."}

# Poll status
curl http://localhost:42617/v1/runs/<run_id> \
  -H "Authorization: Bearer $TOKEN"

# Get result when complete
curl http://localhost:42617/v1/runs/<run_id>/result \
  -H "Authorization: Bearer $TOKEN"

# Get full event log for the run
curl http://localhost:42617/v1/runs/<run_id>/events \
  -H "Authorization: Bearer $TOKEN"

# Get agent transcript
curl http://localhost:42617/v1/runs/<run_id>/transcript \
  -H "Authorization: Bearer $TOKEN"

# List all runs
curl http://localhost:42617/v1/runs \
  -H "Authorization: Bearer $TOKEN"

# Cancel a running job
curl -X DELETE http://localhost:42617/v1/runs/<run_id> \
  -H "Authorization: Bearer $TOKEN"
```

#### Streaming — SSE event stream

Subscribe to a run's events as server-sent events:

```bash
curl -N http://localhost:42617/v1/runs/<run_id>/stream \
  -H "Authorization: Bearer $TOKEN"
```

#### WebSocket — `/ws/chat`

Interactive chat over WebSocket:

```bash
websocat ws://localhost:42617/ws/chat
```

## Output

The pipeline produces files in your workspace:

```
research/
  raw-findings.md      # Researcher's web search results
  detailed-data.md     # Scraper's extracted data
  analysis.md          # Analyst's synthesis
  events.jsonl         # Persistent event log (all inter-agent events)
output/
  brief.md             # Final polished brief
```

### Persistent Event Log

Events are written to `research/events.jsonl` as append-only JSONL. This means:
- Events survive process restarts — relaunch the gateway and prior events are still available
- Full audit trail of every inter-agent message in the pipeline
- Each line is a complete JSON object with `id`, `topic`, `source`, `payload`, and `timestamp_ms`

To disable persistence (ephemeral mode), remove the `event_log_path` line from the config.

## API Reference

All endpoints require `Authorization: Bearer <token>` (except `/health`, `/metrics`, and `/pair`).

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/` | Web dashboard |
| GET | `/health` | Health check |
| GET | `/health/ready` | Readiness probe |
| GET | `/metrics` | Prometheus metrics |
| POST | `/pair` | Pair a new client (`X-Pairing-Code` header) |
| POST | `/api/chat` | Synchronous agent chat |
| POST | `/v1/chat/completions` | OpenAI-compatible chat |
| GET | `/v1/models` | List available models |
| POST | `/v1/runs` | Submit async job |
| GET | `/v1/runs` | List all jobs |
| GET | `/v1/runs/:id` | Job status |
| DELETE | `/v1/runs/:id` | Cancel job |
| GET | `/v1/runs/:id/result` | Job result |
| GET | `/v1/runs/:id/events` | Job event log |
| GET | `/v1/runs/:id/transcript` | Job transcript |
| GET | `/v1/runs/:id/stream` | SSE event stream |
| GET | `/v1/agents` | List registered agents |
| POST | `/v1/estop` | Emergency stop all agents |
| GET | `/ws/chat` | WebSocket agent chat |
| GET | `/ws/runs/:id` | WebSocket run subscription |
| POST | `/v1/webhook/:channel` | Channel webhook |

## Config Reference

The entire pipeline is defined in `agentzero.toml`. Key sections:

### Swarm

```toml
[swarm]
enabled = true
max_agents = 5
event_bus_capacity = 256
event_log_path = "./research/events.jsonl"  # persistent events (optional)
shutdown_grace_ms = 10000
```

### Agent definition

```toml
[swarm.agents.researcher]
name = "Researcher"
description = "Searches the web for information on a given topic."
keywords = ["research", "search", "find"]
provider = "anthropic"
model = "claude-haiku-4-5"
privacy_boundary = "encrypted_only"
allowed_tools = ["web_search", "web_fetch", "write_file", "read_file"]
subscribes_to = ["channel.*.message"]
produces = ["task.research.raw"]
max_iterations = 15
system_prompt = """..."""
```

### Pipeline definition

```toml
[[swarm.pipelines]]
name = "research-to-brief"
channel_reply = true
on_step_error = "skip"
max_retries = 1
step_timeout_secs = 120
steps = ["researcher", "scraper", "analyst", "writer"]

[swarm.pipelines.trigger]
keywords = ["research", "brief", "analyze", "investigate"]
regex = "(?i)(research|write a brief|analyze|report on)\\s+"
```

## Customization

### Adding a fact-checker step

Insert a fact-checker agent between Analyst and Writer:

```toml
[swarm.agents.factchecker]
name = "Fact Checker"
description = "Verifies claims and data points from the analysis against original sources."
keywords = ["verify", "fact-check"]
provider = "anthropic"
model = "claude-sonnet-4-6"
allowed_tools = ["read_file", "write_file", "web_fetch", "web_search"]
subscribes_to = ["task.analysis.complete"]
produces = ["task.factcheck.complete"]
max_iterations = 15
system_prompt = "You verify every claim in research/analysis.md against its cited source..."
```

Then update the pipeline steps:
```toml
steps = ["researcher", "scraper", "analyst", "factchecker", "writer"]
```

And update the Writer's `subscribes_to`:
```toml
subscribes_to = ["task.factcheck.complete"]
```

### Using local models for privacy

Route all analysis through a local model:

```toml
[swarm.agents.analyst]
provider = "ollama"
model = "llama3"
base_url = "http://localhost:11434"
privacy_boundary = "local_only"
```

### Changing search providers

For better search quality, use Brave or Perplexity:

```toml
[web_search]
provider = "brave"
# Set BRAVE_API_KEY in your .env
```

### Fan-out execution

Run multiple agents in parallel (e.g., search multiple topics at once):

```toml
[[swarm.pipelines]]
name = "parallel-research"
execution_mode = "fanout"
channel_reply = true

[[swarm.pipelines.fanout_steps]]
agents = ["researcher-a", "researcher-b", "researcher-c"]
merge = "wait_all"
```

### Template-free output formatting

The Writer agent's system prompt defines the output format directly. To change the brief format, edit the Writer's `system_prompt` in the config. No template engine needed.

## Prerequisites

- **Required**: Anthropic account — run `agentzero auth login --provider anthropic` or set `ANTHROPIC_API_KEY`
- **Optional**: `agent-browser` for the Scraper step — npm dependencies install automatically on first use (pipeline works without it, requires Node.js + npm)
- **Optional**: Brave/Perplexity API key for better search results (DuckDuckGo works without keys)
