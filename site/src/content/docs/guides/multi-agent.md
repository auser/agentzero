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

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `delegate` tool not available | No `[agents.*]` configured | Add at least one agent section |
| Infinite delegation loop | No `max_depth` limit | Set `max_depth = 1` or `2` |
| Swarm router misclassifies | Vague agent descriptions | Write specific descriptions for each agent |
| Pipeline hangs | Step timeout too short | Increase `step_timeout_secs` |
| Agent can't use a tool | Tool not in `allowed_tools` | Add the tool name to the agent's allowlist |
