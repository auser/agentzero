# Agent-as-a-Service Design

**Goal:** Enable runtime agent creation, management, and webhook routing via the gateway API, so agents can be deployed instantly without restarting the gateway.

## Motivation

Currently agents are defined statically in `[swarm.agents]` TOML config at startup. To support "deploy your agent in seconds", we need:

1. Runtime agent CRUD via REST API
2. Persistent agent storage (survives gateway restarts)
3. Webhook proxy with agent targeting
4. Platform webhook auto-registration (Telegram, Discord, Slack)

## Architecture

### AgentStore (agentzero-orchestrator)

Follows the `ApiKeyStore` pattern: `RwLock<Vec<AgentRecord>>` + optional `EncryptedJsonStore` backing.

```rust
pub struct AgentRecord {
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub system_prompt: Option<String>,
    pub provider: String,
    pub model: String,
    pub keywords: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub channels: HashMap<String, ChannelConfig>,
    pub created_at: u64,
    pub updated_at: u64,
    pub status: AgentStatus,
}

pub enum AgentStatus {
    Active,
    Stopped,
}

pub struct ChannelConfig {
    pub platform: String,
    pub webhook_url: Option<String>,
    // Token stored separately in secrets, not in AgentRecord
}
```

### API Endpoints (agentzero-gateway)

| Method | Path | Scope | Description |
|--------|------|-------|-------------|
| POST | /v1/agents | Admin | Create agent |
| GET | /v1/agents | RunsRead | List all agents (static + dynamic) |
| GET | /v1/agents/:id | RunsRead | Get agent details |
| PATCH | /v1/agents/:id | Admin | Update agent config |
| DELETE | /v1/agents/:id | Admin | Delete agent |
| POST | /v1/hooks/:channel/:agent_id | RunsWrite | Webhook with agent targeting |

### Request/Response Types

**Create Agent Request:**
```json
{
  "name": "Aria",
  "description": "Helpful travel assistant",
  "system_prompt": "You are Aria, a knowledgeable travel assistant...",
  "provider": "anthropic",
  "model": "claude-sonnet-4-20250514",
  "keywords": ["travel", "booking", "hotels"],
  "allowed_tools": ["web_search", "web_fetch"],
  "channels": {
    "telegram": { "bot_token": "123:ABC..." }
  }
}
```

**Create Agent Response:**
```json
{
  "agent_id": "agent_a1b2c3d4",
  "name": "Aria",
  "status": "active",
  "channels": ["telegram"],
  "created_at": "2026-03-13T..."
}
```

### Coordinator Integration

The coordinator gains two new methods:

- `register_dynamic_agent(record, config_path, workspace_root)` — builds a `RuntimeExecution` from the record, creates an agent worker task, registers with the router
- `deregister_agent(agent_id)` — cancels the worker task, removes from router

On gateway startup, `build_swarm_with_presence` loads agents from both TOML config AND AgentStore (AgentStore agents supplement TOML ones).

### Webhook Auto-Registration

When an agent is created with channel config containing a bot token:

- **Telegram**: Call `POST https://api.telegram.org/bot<token>/setWebhook` with `url=https://<gateway>/v1/hooks/telegram/<agent_id>`
- **Discord**: Register interaction endpoint URL
- **Slack**: Configure event subscription URL

On agent deletion, deregister webhooks (Telegram: `deleteWebhook`).

### Security

- Bot tokens stored in AgentStore's encrypted JSON (EncryptedJsonStore)
- Tokens never logged or returned in API responses (masked in GET)
- Agent CRUD requires Admin scope
- Webhook routes require RunsWrite scope

## Files to Modify

- `crates/agentzero-orchestrator/src/agent_store.rs` (new)
- `crates/agentzero-orchestrator/src/lib.rs`
- `crates/agentzero-orchestrator/src/coordinator.rs`
- `crates/agentzero-gateway/src/handlers.rs`
- `crates/agentzero-gateway/src/router.rs`
- `crates/agentzero-gateway/src/state.rs`
- `crates/agentzero-gateway/src/models.rs`
