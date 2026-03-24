---
title: Multi-Agent Patterns
description: Configure delegation, swarm coordination, pipelines, and event-driven architectures with multiple agents.
---

AgentZero supports multiple agent patterns — from simple delegation (one agent spawns another) to full swarm coordination with event-driven pipelines.

## Delegation

The simplest multi-agent pattern. A primary agent delegates specific tasks to specialized sub-agents, each with their own provider, model, and tool allowlist.

### Configuration

Define named agents in `agentzero.toml`:

```toml
[agents.coder]
provider = "anthropic"
model = "claude-sonnet-4-6"
max_depth = 2                              # max delegation nesting
agentic = true                             # enable tool-calling loop
max_iterations = 15                        # max tool calls per turn
allowed_tools = ["shell", "read_file", "write_file", "file_edit", "glob_search", "content_search"]

[agents.researcher]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
max_depth = 1
agentic = true
max_iterations = 10
allowed_tools = ["web_search", "web_fetch", "read_file"]

[agents.reviewer]
provider = "openai"
model = "gpt-4o"
max_depth = 1
agentic = true
max_iterations = 5
allowed_tools = ["read_file", "glob_search", "content_search"]
```

### How It Works

When delegation is configured, the primary agent gets a `delegate` tool. It can call:

```
delegate(agent="coder", message="Implement a rate limiter in src/middleware.rs")
delegate(agent="researcher", message="Find best practices for rate limiting in Rust")
```

Each sub-agent:
- Runs with its own provider and model
- Can only use the tools in its `allowed_tools` list
- Has a `max_depth` limit to prevent infinite delegation chains
- Inherits the workspace root and security policy from the parent

### Depth Limits

`max_depth` controls how many levels deep delegation can go:

- `max_depth = 1` — the sub-agent cannot delegate further
- `max_depth = 2` — the sub-agent can delegate once more
- `max_depth = 0` — delegation disabled for this agent

---

## Agent Conversations

For back-and-forth dialogue between agents (or between an agent and a human), the `converse` tool enables multi-turn conversations. Unlike delegation (which is one-shot), conversations let agents discuss, negotiate, ask clarifying questions, and iterate.

### Configuration

Any swarm agent with `"converse"` in its `allowed_tools` gets access to the `converse` tool. Configure conversation limits per agent:

```toml
[swarm.agents.researcher]
name = "Researcher"
provider = "anthropic"
model = "claude-sonnet-4-6"
system_prompt = "You research topics. Use converse to discuss findings with the analyst."
allowed_tools = ["converse", "web_search", "web_fetch"]

[swarm.agents.researcher.conversation]
max_turns = 15                             # max turns per conversation (default: 10)
turn_timeout_secs = 120                    # per-turn timeout (default: 120)

[swarm.agents.analyst]
name = "Analyst"
provider = "openai"
model = "gpt-4o"
system_prompt = "You analyze research. Ask clarifying questions when needed."
allowed_tools = ["converse", "read_file", "write_file"]

[swarm.agents.analyst.conversation]
max_turns = 10
```

### How It Works

The calling agent controls the conversation flow. Each `converse` call is one turn:

```
converse(agent="analyst", message="Here are my findings on X...", conversation_id="conv-123")
```

1. Agent A calls `converse` with a message and a `conversation_id`
2. The message is dispatched to Agent B
3. Agent B processes it (with full context from prior turns via shared `conversation_id`)
4. Agent B's response is returned to Agent A as the tool result
5. Agent A decides whether to send another message or stop

The `conversation_id` groups turns together — Agent B remembers prior messages in the same conversation. Agent A generates the ID on the first turn and reuses it for follow-ups.

### Human-in-the-Loop

Agents can also converse with humans through channels:

```
converse(channel="slack", recipient="#engineering", message="Should we proceed with approach A or B?", conversation_id="conv-456")
```

The agent's turn blocks until the human replies (or the timeout elapses).

### Safety

| Protection | Mechanism |
|---|---|
| Turn limit | `max_turns` per conversation (configurable per agent) |
| Per-turn timeout | `turn_timeout_secs` (default: 120s) |
| Budget limits | Inherited token and cost limits from parent context |
| Loop detection | Built-in tool loop detector catches repetitive calls |
| Cancellation | Conversations respect the cancellation token |
| Leak guard | Responses are scanned for credential leaks before returning |

---

## Swarm Coordination

For more complex patterns, the swarm system provides an event bus, AI-powered message routing, and sequential pipelines.

### Configuration

```toml
[swarm]
enabled = true
max_agents = 10
event_bus_capacity = 256                   # broadcast channel buffer size
shutdown_grace_ms = 10000                  # grace period for in-flight tasks
```

### Swarm Router

The router classifies incoming messages and dispatches them to the right agent. It can use an LLM for classification or fall back to keyword matching:

```toml
[swarm.router]
provider = "openrouter"
model = "anthropic/claude-haiku-4-5"       # fast model for classification
base_url = "https://openrouter.ai/api/v1"
fallback_to_keywords = true                # use keywords if AI classification fails
```

### Swarm Agents

Define agents with descriptions (used by the AI router) and keyword fallbacks:

```toml
[swarm.agents.support]
name = "Support Agent"
description = "Handles customer support questions, troubleshooting, and FAQ"
keywords = ["help", "issue", "problem", "error", "broken"]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
allowed_tools = ["read_file", "web_search", "web_fetch"]
max_iterations = 10
system_prompt = "You are a helpful support agent. Be concise and solution-oriented."

[swarm.agents.developer]
name = "Developer Agent"
description = "Writes code, fixes bugs, refactors, and reviews pull requests"
keywords = ["code", "implement", "fix", "refactor", "review", "PR"]
provider = "anthropic"
model = "claude-sonnet-4-6"
allowed_tools = ["shell", "read_file", "write_file", "file_edit", "glob_search", "content_search", "git_operations"]
max_iterations = 20
system_prompt = "You are a senior developer. Write clean, tested code."

[swarm.agents.analyst]
name = "Data Analyst"
description = "Analyzes data, creates reports, and answers analytical questions"
keywords = ["analyze", "report", "data", "statistics", "metrics"]
provider = "openai"
model = "gpt-4o"
allowed_tools = ["read_file", "shell", "web_search"]
max_iterations = 15
```

### Event Bus

Swarm agents communicate via a topic-based event bus. Agents can subscribe to topic patterns and publish results:

```toml
[swarm.agents.monitor]
name = "Monitor"
subscribes_to = ["task.*.complete"]        # wildcard topic patterns
produces = ["report.daily"]                # topics published on output
```

The event bus uses a broadcast channel — all subscribers receive every matching message. The `event_bus_capacity` setting controls the buffer size.

---

## Pipelines

Pipelines define sequential agent workflows — a message flows through a chain of agents, each processing the output of the previous one.

### Configuration

```toml
[[swarm.pipelines]]
name = "research-to-brief"
channel_reply = true                       # send final result back to the channel
on_step_error = "abort"                    # abort | skip | retry
max_retries = 1
step_timeout_secs = 120
steps = ["researcher", "analyst", "writer"]

[swarm.pipelines.trigger]
keywords = ["research", "investigate", "deep dive"]
# regex = "^research:"                     # optional regex trigger
```

### How Pipelines Work

1. A message matches the pipeline trigger (keywords or regex)
2. The message is sent to the first agent in `steps`
3. Each agent's output becomes the input to the next agent
4. The final agent's output is returned as the response
5. If `channel_reply = true`, the result is sent back to the originating channel

### Error Handling

| Mode | Behavior |
|---|---|
| `abort` | Stop the pipeline on any error |
| `skip` | Skip the failed step and pass the previous output to the next step |
| `retry` | Retry the failed step up to `max_retries` times |

---

## Privacy Boundaries

Each swarm agent can enforce a privacy boundary:

```toml
[swarm.agents.internal]
privacy_boundary = "local_only"            # only local tools, no outbound network
allowed_tools = ["read_file", "shell"]

[swarm.agents.external]
privacy_boundary = "any"                   # full access to all enabled tools
allowed_tools = ["web_search", "web_fetch", "read_file"]
```

| Boundary | Effect |
|---|---|
| `"local_only"` | Blocks outbound network tools |
| `"encrypted_only"` | Requires encrypted transport |
| `"any"` | No restrictions |

---

## Examples

### Code Review Pipeline

```toml
[swarm]
enabled = true

[swarm.router]
provider = "openrouter"
model = "anthropic/claude-haiku-4-5"
fallback_to_keywords = true

[swarm.agents.reviewer]
name = "Code Reviewer"
description = "Reviews code for bugs, style issues, and security problems"
keywords = ["review", "PR", "check"]
provider = "anthropic"
model = "claude-sonnet-4-6"
allowed_tools = ["read_file", "glob_search", "content_search", "git_operations"]
max_iterations = 15
system_prompt = "Review the code for correctness, security, and style. Be specific about line numbers."

[swarm.agents.tester]
name = "Test Writer"
description = "Writes tests for code changes"
keywords = ["test", "coverage"]
provider = "anthropic"
model = "claude-sonnet-4-6"
allowed_tools = ["read_file", "write_file", "shell", "glob_search"]
max_iterations = 20
system_prompt = "Write comprehensive tests. Use the project's existing test patterns."

[[swarm.pipelines]]
name = "review-and-test"
steps = ["reviewer", "tester"]
channel_reply = true
on_step_error = "abort"

[swarm.pipelines.trigger]
keywords = ["review and test", "full review"]
```

### Research Brief Pipeline

See [examples/research-pipeline/](https://github.com/auser/agentzero/tree/main/examples/research-pipeline) for a complete 4-agent research pipeline.

### Business Office

See [examples/business-office/](https://github.com/auser/agentzero/tree/main/examples/business-office) for a 7-agent AI office with CEO, CTO, CSO, Marketing, Legal, Finance, and HR agents.

---

## Autonomous Swarms

Give AgentZero a natural language goal and it autonomously decomposes it into a parallel agent swarm. Each agent gets only the tools it needs.

### CLI

```bash
agentzero swarm "summarize this video, generate a thumbnail, and write a script"
```

The `GoalPlanner` uses an LLM to decompose the goal into a multi-agent DAG:

1. **video_downloader** (tools: `shell`, `web_fetch`) — downloads the video
2. **transcriber** (tools: `shell`) — runs Whisper via `whisper {{input}}`
3. **thumbnail_generator** (tools: `image_gen`, `shell`) — generates a thumbnail
4. **summarizer** (depends on: transcriber) — summarizes the transcript (LLM only)
5. **script_writer** (depends on: summarizer) — writes a polished script

Nodes 1-3 run in parallel. Nodes 4-5 are sequential. Each agent receives only its `tool_hints` — not the full tool set.

### Gateway

```bash
curl -X POST http://localhost:3000/v1/swarm \
  -H "Content-Type: application/json" \
  -d '{"goal": "summarize this video"}'
```

### How It Works

1. **GoalPlanner** sends the goal + available tool catalog to the LLM
2. LLM returns a `PlannedWorkflow` with per-node `tool_hints`
3. `SwarmSupervisor` compiles the workflow into an `ExecutionPlan`
4. Each agent node gets a `HintedToolSelector` that filters tools to its hints
5. Nodes execute in parallel via `tokio::JoinSet`, respecting dependency ordering
6. Results are collected and merged

### Pre-planned Workflows

You can also provide a pre-generated plan:

```bash
agentzero swarm --plan workflow.json "execute this plan"
```

---

## Natural Language Agent Definitions

Define persistent agents from plain English. The system derives name, system prompt, keywords, tools, and schedule automatically. Agents persist encrypted and are auto-routed to by keywords.

### Creating an Agent via Conversation

```
You: "I need an agent that reviews my GitHub PRs daily and posts summaries to Slack"

Agent calls agent_manage:
{
  "action": "create_from_description",
  "nl_description": "An agent that reviews my GitHub PRs daily and posts summaries to Slack"
}

→ LLM derives the agent definition:
  name:           pr_reviewer
  description:    Reviews GitHub PRs and posts daily summaries to Slack
  system_prompt:  "You are an expert code reviewer. Each day, check for open
                   PRs on the configured GitHub repos, review the diffs for
                   bugs, security issues, and style violations, then post a
                   concise summary to the designated Slack channel."
  keywords:       ["pr", "review", "github", "code review", "pull request"]
  allowed_tools:  ["shell", "read_file", "web_fetch", "git_operations", "http_request"]
  schedule:       "0 9 * * *"  (daily at 9am)

→ Agent created with ID: agent-a1b2c3
→ Persists in .agentzero/agents.json (encrypted)
→ Suggested schedule: 0 9 * * * (use cron_add to activate)
```

### Creating an Agent via the Gateway API

```bash
curl -X POST http://localhost:3000/v1/agent \
  -H "Content-Type: application/json" \
  -d '{
    "message": "Create an agent that monitors my server logs for errors and alerts me on Telegram"
  }'
```

The agent internally calls `agent_manage create_from_description` and returns the created agent details.

### How Agents Are Routed

When you send a message to AgentZero, the system checks if any persistent agent's keywords match:

```
You: "Review the latest PRs on agentzero"

→ AgentRouter matches keywords ["pr", "review"] against stored agents
→ Routes to pr_reviewer agent (created last week)
→ pr_reviewer already has the right system prompt, tools, and context
→ No manual configuration needed
```

### Managing Persistent Agents

The `agent_manage` tool supports these actions:

| Action | Description | Example |
|---|---|---|
| `create` | Create with explicit fields | `{"action": "create", "name": "monitor", "keywords": ["logs"]}` |
| `create_from_description` | Create from plain English | `{"action": "create_from_description", "nl_description": "..."}` |
| `list` | List all persistent agents | `{"action": "list"}` |
| `get` | Get full agent details | `{"action": "get", "agent_id": "agent-a1b2c3"}` |
| `update` | Update agent fields | `{"action": "update", "agent_id": "...", "keywords": ["new"]}` |
| `delete` | Delete an agent | `{"action": "delete", "agent_id": "agent-a1b2c3"}` |
| `set_status` | Pause/resume | `{"action": "set_status", "agent_id": "...", "status": "stopped"}` |

### Dedup Awareness

When creating an agent via `create_from_description`, the LLM prompt includes all existing agents. If a similar agent already exists, the system can update it rather than creating a duplicate:

```
You: "Create an agent that reviews code in pull requests"

→ LLM sees existing agent: pr_reviewer (keywords: [pr, review, github])
→ Response: "Agent 'pr_reviewer' already exists with similar capabilities.
   Updated its system prompt to include general code review."
```

### Agent Channel Integration

Persistent agents can be connected to communication channels. The `AgentRecord` includes a `channels` field for platform-specific configuration:

```
You: "Connect the pr_reviewer agent to my Telegram"

Agent calls agent_manage update:
{
  "action": "update",
  "agent_id": "agent-a1b2c3",
  "channels": {
    "telegram": { "chat_id": "-1001234567890", "enabled": true }
  }
}
```

Once connected, the agent receives messages from that channel and responds through it.

### Configuration

```toml
[agent]
enable_agent_manage = true
enable_dynamic_tools = true    # needed for the LLM call in create_from_description
```

### Persistence

Agents persist in `.agentzero/agents.json` (encrypted at rest). They survive restarts, updates, and reboots. The user's agent library grows over time as a personal team of specialists.

---

## Tool Catalog Learning

The system remembers which tool combinations worked for what kinds of goals. Over time, this builds an institutional memory that makes future runs faster and more accurate.

### How It Works

```
Week 1: "summarize this video"
  → System uses: shell, web_fetch, whisper_transcribe, image_gen
  → Run succeeds
  → Recipe recorded: {goal: "summarize video", tools: [...], success: true}

Week 2: "transcribe this podcast"
  → RecipeStore matches on "transcribe" keywords
  → Boosts whisper_transcribe tool (already exists from Week 1)
  → No tool creation needed — faster execution

Week 4: "create a highlight reel from this webinar"
  → RecipeStore matches on "video" + existing tools
  → whisper_transcribe, shell, image_gen all boosted
  → Agent assembles the right pipeline immediately
```

### Selection Priority

The `HintedToolSelector` combines three signals in order:

1. **Explicit hints** — from `GoalPlanner` per-node `tool_hints` (highest priority)
2. **Recipe matches** — Jaccard similarity on goal keywords → boost previously successful tools
3. **Keyword fallback** — TF-IDF matching on tool name + description

### Recipe Matching

Recipes are matched using Jaccard similarity on tokenized goal keywords:

```
Stored recipe:  "summarize this video" → tokens: [summarize, this, video]
New goal:       "summarize this podcast" → tokens: [summarize, this, podcast]

Jaccard similarity = |intersection| / |union|
                   = |{summarize, this}| / |{summarize, this, video, podcast}|
                   = 2/4 = 0.5  → MATCH (above threshold)

→ Tools from stored recipe are boosted: shell, web_fetch, whisper_transcribe
```

Recipes with higher `use_count` (reused more often) get a logarithmic boost, so frequently successful patterns float to the top.

### What Gets Recorded

| Field | Description |
|---|---|
| `goal_summary` | The original goal text |
| `goal_keywords` | Pre-tokenized keywords for matching |
| `tools_used` | Tool names that were invoked during the run |
| `success` | Whether the run completed successfully |
| `timestamp` | When the recipe was recorded |
| `use_count` | Incremented each time this recipe's tools are reused |

Only successful recipes are used for matching. Failed recipes are stored but excluded from suggestions — the system only recommends tool combos that actually worked.

### Persistence

Recipes persist in `.agentzero/tool-recipes.json` (encrypted at rest). The store retains up to 200 recipes, pruning oldest failed recipes when the limit is reached. Successful, frequently-reused recipes are prioritized.

### Growth Over Time

| Timeframe | System State |
|---|---|
| **Day 1** | 0 dynamic tools, 0 agents, 0 recipes. Every goal starts from scratch. |
| **Week 1** | 3 dynamic tools created, 1 agent defined, 5 recipes recorded. |
| **Week 4** | 8 tools, 3 agents, 20 recipes. New goals match existing patterns 60%+ of the time. |
| **Month 2** | 15 tools, 5 agents, 50 recipes. The system resolves most goals using existing infrastructure. |

The `.agentzero/` directory is the system's growing brain — portable, encrypted, and backupable.

---

## Observability

When running multiple agents, visibility into what each agent is doing becomes critical. AgentZero provides built-in observability through the web dashboard and API.

### Agent Topology

The dashboard shows a live topology graph of all active agents and their delegation relationships. Agents appear as nodes colored by status (green = running, blue = active, gray = idle). Delegation links appear as directed edges between agents.

The topology data is available via `GET /v1/topology` and refreshes every 3 seconds in the dashboard.

### Delegation Tree View

The Runs page supports a **Tree** view (toggle the Flat/Tree button) that groups runs by their delegation hierarchy. Parent runs appear at the top level, and child runs are indented below their parent with visual connectors. Each run shows its `depth` in the delegation chain.

### Per-Agent Analytics

Click the stats button on any agent row to see aggregated metrics:

- **Total runs** with status breakdown (running, completed, failed)
- **Cost and token usage** totals
- **Success rate** percentage
- **Tool usage frequency** — a bar chart showing which tools the agent calls most often

Available via `GET /v1/agents/:agent_id/stats`.

### Regression Detection

When multiple agents modify files in the same delegation tree, AgentZero can detect potential conflicts — cases where one agent may be undoing another's work.

The `FileModificationTracker` monitors `tool.file_written` events and flags when two different agents modify the same file within the same correlation tree (delegation chain). Conflicts are published as `regression.file_conflict` events on the event bus and appear as warning banners on the dashboard.

### Tool Call Timeline

Each run's detail panel includes a **Timeline** tab showing a color-coded sequential view of every tool call the agent made during that run. This helps identify patterns, bottlenecks, and unexpected tool usage.

---

## A2A Swarm Integration

AgentZero supports the [Agent-to-Agent (A2A) protocol](https://google.github.io/A2A/) for integrating remote agents into your swarm. Remote A2A agents are treated as first-class swarm participants — the router can dispatch messages to them, and they appear in the topology graph alongside local agents.

### Configuration

Define remote agents in the `[a2a]` config section. Each entry points to an A2A-compatible agent endpoint:

```toml
[a2a]
enabled = true

[a2a.agents.remote-researcher]
url = "https://research-agent.example.com/.well-known/agent.json"
name = "Remote Researcher"
description = "External research agent hosted on a remote server"
keywords = ["research", "academic", "papers"]
timeout_secs = 60

[a2a.agents.remote-coder]
url = "https://coder.internal:8443/.well-known/agent.json"
name = "Remote Coder"
description = "Code generation agent running on a GPU server"
keywords = ["code", "implement", "refactor"]
auth_header = "Bearer ${A2A_CODER_TOKEN}"
timeout_secs = 120
```

### How It Works

1. At startup, AgentZero fetches each remote agent's A2A Agent Card from the configured URL
2. The agent's declared skills and capabilities are registered with the swarm router
3. When a message matches a remote agent's keywords or description, the router dispatches it over HTTPS
4. The response is returned to the calling agent or pipeline as if it came from a local agent

### Authentication

Remote A2A agents can require authentication. Use `auth_header` to set a bearer token or other auth header. Environment variable interpolation (`${VAR}`) is supported in the header value.

### Mixing Local and Remote Agents

Local and remote agents can coexist in the same swarm and pipelines. A pipeline step can reference a remote A2A agent by its config key:

```toml
[[swarm.pipelines]]
name = "research-and-code"
steps = ["remote-researcher", "developer"]    # remote + local
channel_reply = true
on_step_error = "abort"
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `delegate` tool not available | No `[agents.*]` configured | Add at least one agent section |
| Infinite delegation loop | No `max_depth` limit | Set `max_depth = 1` or `2` |
| Swarm router misclassifies | Vague agent descriptions | Write specific descriptions for each agent |
| Pipeline hangs | Step timeout too short | Increase `step_timeout_secs` |
| Agent can't use a tool | Tool not in `allowed_tools` | Add the tool name to the agent's allowlist |
| `converse` tool not available | Not in `allowed_tools` | Add `"converse"` to the agent's `allowed_tools` |
| Conversation stops unexpectedly | Turn limit reached | Increase `max_turns` in `[*.conversation]` |
| Conversation timeout | Agent takes too long to respond | Increase `turn_timeout_secs` |
