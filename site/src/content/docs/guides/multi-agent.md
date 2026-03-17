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

## Markdown Agent Definitions

In addition to TOML-based agent configuration, you can define agents as Markdown files in the `agents/` directory at your project root. Each file defines one agent with its persona, instructions, and configuration in a human-readable format.

### Creating an agent definition

Create a file at `agents/<name>.md`:

```markdown
<!-- agents/reviewer.md -->
---
provider: anthropic
model: claude-sonnet-4-6
max_iterations: 15
allowed_tools:
  - read_file
  - glob_search
  - content_search
  - git_operations
---

# Reviewer

You are a senior code reviewer. When asked to review code:

1. Read the diff using `git_operations`
2. Check each change for correctness, security issues, and style
3. Search for related patterns in the codebase with `content_search`
4. Provide specific feedback referencing file paths and line numbers

Be constructive. Distinguish between must-fix issues and suggestions.
```

The YAML frontmatter contains the agent's configuration (provider, model, tools). The Markdown body becomes the agent's system prompt.

### Discovery

Agent definitions are loaded from the `agents/` directory in the project root. Files are named `<agent-name>.md` — the filename (without extension) becomes the agent's routing name.

```
project/
├── agents/
│   ├── reviewer.md
│   ├── coder.md
│   └── researcher.md
├── agentzero.toml
└── ...
```

Markdown-defined agents merge with TOML-defined agents. If both define an agent with the same name, the Markdown definition takes priority.

### @agent routing

Route messages to a specific agent using the `@agent` prefix:

```bash
agentzero agent -m "@reviewer check the last commit for security issues"
agentzero agent -m "@coder implement a rate limiter in src/middleware.rs"
agentzero agent -m "@researcher find best practices for Rust error handling"
```

When no `@agent` prefix is present, the message goes to the default agent (or the swarm router, if configured).

---

## Conversation Threads

Agents can participate in threaded conversations using a `thread_id`. Threads maintain context across multiple messages, enabling back-and-forth dialogue.

### Starting a thread

```bash
# Start a new thread (auto-generates thread_id)
agentzero agent -m "Let's plan the API redesign" --new-thread

# Continue an existing thread
agentzero agent -m "What about error handling?" --thread <thread_id>
```

### Thread commands

| Command | Description |
|---|---|
| `/thread` | Show the current thread ID and message count |
| `/thread list` | List all active threads |
| `/thread switch <id>` | Switch to a different thread |
| `/thread new` | Start a new thread |

Threads are stored in the memory backend (SQLite or Turso) and persist across sessions.

---

## Heartbeat-Driven Cycles

For long-running autonomous agents, heartbeat cycles provide a periodic execution loop. The agent wakes at a configurable interval, checks for pending work, and executes tasks.

```toml
[agent.heartbeat]
enabled = true
interval_secs = 300              # wake every 5 minutes
idle_action = "check_channels"   # what to do when no pending tasks
max_consecutive_idle = 12        # stop after 12 idle cycles (1 hour)
```

During each heartbeat cycle, the agent:

1. Checks for new messages on configured channels
2. Processes any pending cron-triggered tasks
3. Reviews and advances in-progress workflows
4. Reports status if configured to do so

Heartbeat mode is useful for always-on agents that monitor channels, process scheduled tasks, or maintain long-running workflows.

---

## Multi-Agent CLI Commands

These commands manage agents, conversations, and coordination from the CLI:

### `/agents` — manage agent definitions

```bash
agentzero agents list            # List all agents (TOML + Markdown)
agentzero agents create --name Aria --description "Travel planner" \
  --model claude-sonnet-4-6 --provider anthropic --keywords travel,booking
agentzero agents get --id agent_abc123
agentzero agents update --id agent_abc123 --model gpt-4o
agentzero agents delete --id agent_abc123
```

### `/talk` — send a message to a specific agent

```bash
agentzero talk reviewer "Check the latest PR for issues"
agentzero talk coder "Add input validation to the signup endpoint"
```

This is equivalent to using `@agent` routing but as a dedicated command.

### `/thread` — manage conversation threads

```bash
agentzero thread list            # List active threads
agentzero thread show <id>       # Show thread messages
agentzero thread new             # Start a new thread
```

### `/broadcast` — send a message to all agents

```bash
agentzero broadcast "Project deadline moved to Friday — adjust priorities"
```

All configured agents receive the message. Responses are collected and returned. Useful for announcements or coordination signals.

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
| `@agent` routing not working | No agent with that name defined | Check `agents/` directory and `[agents.*]` in config |
| Markdown agent not loading | File not in `agents/` directory | Ensure the file is at `<project-root>/agents/<name>.md` |
| Thread context missing | Wrong `thread_id` | Use `agentzero thread list` to find the correct ID |
