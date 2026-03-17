# Multi-Agent Team Example

Three specialized agents collaborate on tasks via delegation and IPC.

## Architecture

```
User Message
     |
     v
  [Router] --- classifies by keywords
     |
     +---> [Researcher] --- web search, file reading
     |         |
     |         +--> delegates writing to Writer
     |
     +---> [Writer] --- creates documents, code
     |         |
     |         +--> delegates review to Reviewer
     |
     +---> [Reviewer] --- analyzes quality, style
               |
               +--> delegates fixes to Writer
```

## Agents

| Agent | Role | Tools |
|-------|------|-------|
| Researcher | Find information, synthesize sources | web_search, read_file, web_fetch |
| Writer | Create documents, write code | write_file, file_edit, git, shell |
| Reviewer | Quality review, feedback | read_file, git, shell |

## Setup

1. Set your API key:
   ```bash
   export OPENROUTER_API_KEY="sk-..."
   ```

2. Start the gateway:
   ```bash
   agentzero gateway --config examples/multi-agent-team/config.toml
   ```

3. Send a message via API:
   ```bash
   curl -X POST http://localhost:42617/api/chat \
     -H "Content-Type: application/json" \
     -d '{"message": "Research and write a brief on WebAssembly component model"}'
   ```

## How delegation works

1. The **Router** (fast, cheap model) classifies the message by keywords
2. It dispatches to the best-fit agent
3. Agents can delegate to each other via `@agent_name` mentions
4. All agents share conversation memory via the event bus
5. Results flow back to the user through the originating channel

## Customization

- Add agents: add `[swarm.agents.<name>]` sections
- Change routing: adjust `keywords` per agent
- Add pipelines: define `[[swarm.pipelines]]` for sequential workflows
- Connect channels: add `[channels.telegram]` etc. for messaging platforms
