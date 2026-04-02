---
title: A2A Protocol
description: Agent-to-Agent protocol support for cross-framework interoperability — expose AgentZero as an A2A agent and call external A2A agents.
---

AgentZero supports the [Google Agent-to-Agent (A2A) protocol](https://github.com/google/A2A) for cross-framework agent interoperability. Any A2A-compatible agent can discover and interact with AgentZero, and AgentZero can call external A2A agents as swarm participants.

## Agent Card Discovery

Every AgentZero gateway exposes an Agent Card at the standard well-known URL:

```
GET /.well-known/agent.json
```

Response:
```json
{
  "name": "agentzero-gateway",
  "description": "AgentZero AI agent",
  "version": "0.6.0",
  "capabilities": {
    "streaming": true,
    "pushNotifications": false,
    "stateTransitionHistory": true
  },
  "skills": [{
    "id": "general",
    "name": "General Agent",
    "description": "AgentZero agent with 48 tools available",
    "tags": ["agent", "tools"]
  }],
  "defaultInputModes": ["text"],
  "defaultOutputModes": ["text"]
}
```

## Sending Tasks

External agents can send tasks via JSON-RPC:

```
POST /a2a
Content-Type: application/json

{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tasks/send",
  "params": {
    "id": "task-123",
    "message": {
      "role": "user",
      "parts": [{"type": "text", "text": "Analyze the latest quarterly report"}]
    }
  }
}
```

The response contains the completed task with the agent's response:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "id": "task-123",
    "status": {
      "state": "completed",
      "message": {
        "role": "agent",
        "parts": [{"type": "text", "text": "Here is my analysis..."}]
      }
    },
    "history": [...]
  }
}
```

## Supported Methods

| Method | Description |
|--------|-------------|
| `tasks/send` | Send a message and receive a response (sync) |
| `tasks/sendSubscribe` | Send a message and stream results via SSE |
| `tasks/get` | Retrieve a task by ID (with optional history length) |
| `tasks/cancel` | Cancel a running task |

## Calling External A2A Agents

AgentZero can call external A2A agents, making them first-class swarm participants through the `ConverseTool`.

### Configuration

Add external agents to your `agentzero.toml`:

```toml
[a2a]
enabled = true

[a2a.agents.research-agent]
url = "https://research-agent.example.com"
auth_token = "bearer-token-here"
timeout_secs = 120

[a2a.agents.data-agent]
url = "https://data-agent.example.com"
timeout_secs = 60
```

### How It Works

1. AgentZero creates an `A2aAgentEndpoint` for each configured external agent
2. These endpoints implement the `AgentEndpoint` trait (same as local swarm agents)
3. The `ConverseTool` can call them by name: `{"agent": "research-agent", "message": "..."}`
4. Messages are sent as A2A `tasks/send` requests to the external agent's `/a2a` endpoint

This means your local agents can seamlessly delegate work to remote A2A agents — they don't need to know whether the target is local or remote.

## Streaming via SSE

For long-running tasks, use the SSE streaming endpoint instead of the synchronous `tasks/send`:

```
POST /a2a/stream
Content-Type: application/json

{
  "id": "task-456",
  "message": {
    "role": "user",
    "parts": [{"type": "text", "text": "Analyze the full codebase and write a report"}]
  }
}
```

The response is a `text/event-stream` with typed events:

```
event: status
data: {"id":"task-456","status":{"state":"working"},"final":false}

event: artifact
data: {"id":"task-456","artifact":{"name":"response","parts":[{"type":"text","text":"Section 1..."}],"index":0}}

event: status
data: {"id":"task-456","status":{"state":"completed","message":{"role":"agent","parts":[...]}},"final":true}
```

The Agent Card now advertises `"streaming": true` in its capabilities.

## Multi-Turn with InputRequired

Tasks can pause to request clarification. When the agent needs more information, the task enters the `input-required` state:

```json
{
  "id": "task-789",
  "status": {
    "state": "input-required",
    "message": {
      "role": "agent",
      "parts": [{"type": "text", "text": "What output format do you want? (PDF, HTML, or Markdown)"}]
    }
  }
}
```

Resume the task by sending a new `tasks/send` with the same task ID:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tasks/send",
  "params": {
    "id": "task-789",
    "message": {
      "role": "user",
      "parts": [{"type": "text", "text": "PDF please"}]
    }
  }
}
```

The agent receives the full conversation history and continues from where it left off.

## Agent Discovery

List all known agents (local + discovered via presence):

```
GET /a2a/agents
```

```json
{
  "agents": [
    {"name": "agentzero-gateway", "url": "http://localhost:3000", "status": "online"},
    {"agent_id": "research-agent", "status": "Alive"}
  ]
}
```

## Rich Payloads

Messages support three part types:

| Type | Description | Example |
|------|-------------|---------|
| `text` | Plain text content | `{"type": "text", "text": "Hello"}` |
| `data` | Structured data with MIME type | `{"type": "data", "data": "...", "mimeType": "application/json"}` |
| `file` | File attachment (base64 or URL) | `{"type": "file", "name": "report.pdf", "mimeType": "application/pdf", "data": "base64..."}` |

## Task Lifecycle

```
submitted → working → completed
                    → input-required → (user responds) → working → completed
                    → failed
                    → canceled (via tasks/cancel)
```

Tasks include full conversation history in the `history` array, enabling multi-turn interactions.
