---
title: Autopilot Mode
description: Build self-running AI companies with autonomous agent loops, cap gates, triggers, and reaction matrices.
---

Autopilot mode enables agents to operate autonomously as a self-running company. Agents propose work, the system enforces resource constraints (cap gates), approved proposals become executable missions, and events trigger new proposals — creating a closed-loop cycle with no human intervention required.

## Architecture

```
Proposal → Cap Gate → Approval → Mission → Execution → Event → Trigger/Reaction → Proposal
```

Three components work together:

- **AgentZero (VPS)** — The brain. Runs the agent loops, executes missions, enforces cap gates.
- **Supabase** — State layer. Stores proposals, missions, events. Provides real-time subscriptions.
- **Next.js/Vercel (separate repo)** — Dashboard. Read-only view + lightweight control (approve/reject).

## Quick Start

### 1. Enable Autopilot

Add to `agentzero.toml`:

```toml
[autopilot]
enabled = true
supabase_url = "https://xxx.supabase.co"
supabase_service_role_key = "eyJ..."
max_daily_spend_cents = 500
max_concurrent_missions = 5
stale_threshold_minutes = 30
reaction_matrix_path = "reactions.json"
```

### 2. Define Agents

```toml
[[agents]]
name = "editor"
system_prompt = "You are the Editor-in-Chief. Propose blog topics and review content."

[[agents]]
name = "writer"
system_prompt = "You are a Content Writer. Write posts from research briefs."
```

### 3. Add Triggers

```toml
[[autopilot.triggers]]
name = "periodic_topic_proposal"
condition = { type = "cron", schedule = "0 */6 * * *" }
action = { type = "propose_task", agent = "editor", prompt = "Propose a new blog topic." }
cooldown_secs = 21600
```

### 4. Configure Reactions

Create `reactions.json`:

```json
[
  {
    "source_agent": "editor",
    "event_pattern": "proposal.approved",
    "target_agent": "writer",
    "action": "begin_writing",
    "probability": 1.0,
    "cooldown_secs": 60
  }
]
```

### 5. Apply Supabase Schema

Run `supabase/migrations/001_autopilot_schema.sql` against your Supabase project.

## Core Concepts

### Proposals

Agents create proposals for work to be done. Each proposal has a type, priority, and estimated cost:

| Field | Description |
|---|---|
| `proposal_type` | `content_idea`, `task_request`, `resource_request`, `system_change` |
| `priority` | `low`, `medium`, `high`, `critical` |
| `estimated_cost_microdollars` | Cost estimate (1 cent = 10,000 microdollars) |
| `status` | `pending` → `approved` / `rejected` → `executed` |

### Cap Gates

Cap gates enforce resource constraints before a proposal is approved:

| Constraint | Default | Description |
|---|---|---|
| `max_daily_spend_cents` | 500 | Maximum daily spend across all agents |
| `max_concurrent_missions` | 5 | Maximum missions running simultaneously |
| `max_proposals_per_hour` | 20 | Proposal rate limit |
| `max_missions_per_agent_per_day` | 10 | Per-agent mission cap |

A proposal that violates any constraint is automatically rejected with a reason.

### Missions

When a proposal is approved, it becomes a mission with executable steps:

| Field | Description |
|---|---|
| `status` | `pending` → `in_progress` → `completed` / `failed` / `stalled` |
| `steps` | Ordered list of `MissionStep` with individual status tracking |
| `heartbeat_at` | Last heartbeat timestamp (used for stale detection) |
| `deadline` | Optional deadline |

### Triggers

Triggers create new proposals when conditions are met:

| Condition Type | Description |
|---|---|
| `event_match` | Fire when a specific event type is emitted |
| `cron` | Fire on a cron schedule (e.g., `0 */6 * * *`) |
| `metric_threshold` | Fire when a metric exceeds a threshold |

All triggers support `cooldown_secs` to prevent rapid re-firing.

### Reaction Matrix

The reaction matrix defines probabilistic inter-agent interactions. When agent A emits event X, agent B has probability P of proposing action Y.

```json
{
  "source_agent": "writer",
  "event_pattern": "mission.completed",
  "target_agent": "social_media",
  "action": "propose_social_post",
  "probability": 0.9,
  "cooldown_secs": 600
}
```

Event patterns support wildcards: `*` matches any event, `content.*` matches all content events.

### Stale Recovery

Missions with `heartbeat_at` older than `stale_threshold_minutes` are automatically marked as `stalled`, and a `mission.stalled` event is emitted. This prevents stuck missions from consuming cap gate capacity.

## Company Templates

Three pre-built templates are available in `templates/`:

### Content Agency

6 agents: editor, researcher, writer, seo, social_media, analyst. Agents autonomously propose topics, research, write, optimize, publish, and promote content.

### Dev Agency

6 agents: pm, architect, coder, reviewer, devops, support. Agents triage issues, design solutions, write code, review PRs, deploy, and handle support.

### SaaS Product

6 agents: product, engineer, qa, support, growth, ops. Agents manage product development, testing, customer support, growth marketing, and operations.

Each template includes an `agentzero.toml` with agent definitions and triggers, plus a `reactions.json` with the reaction matrix.

## Autopilot Tools

Four tools are available to agents when autopilot is enabled:

| Tool | Description |
|---|---|
| `proposal_create` | Create a new proposal with type, priority, and cost estimate |
| `proposal_vote` | Approve or reject a proposal (auto-creates mission on approval) |
| `mission_status` | Query one or all missions, optionally filtered by status |
| `trigger_fire` | Manually fire a trigger by ID (for testing or agent-initiated reactions) |

## Human-in-the-Loop

Autopilot and human interaction work together:

- **Dashboard approval** — Proposals can be auto-approved by cap gates OR manually approved/rejected from the dashboard via `/v1/autopilot/proposals/:id/approve`.
- **Chat** — Agents still respond to chat via channels (Telegram, Discord, Slack). Human messages can influence agent behavior alongside autonomous operations.
- **Trigger control** — Triggers can be enabled/disabled from the dashboard without restarting.
- **Emergency stop** — The existing estop mechanism halts all autonomous operations immediately.

## Gateway Endpoints

When autopilot is enabled, the gateway exposes additional REST endpoints for dashboard control:

| Method | Path | Description |
|---|---|---|
| `GET` | `/v1/autopilot/proposals` | List proposals (paginated) |
| `POST` | `/v1/autopilot/proposals/:id/approve` | Approve a proposal |
| `POST` | `/v1/autopilot/proposals/:id/reject` | Reject a proposal |
| `GET` | `/v1/autopilot/missions` | List missions |
| `GET` | `/v1/autopilot/missions/:id` | Mission detail with steps |
| `GET` | `/v1/autopilot/triggers` | List triggers |
| `POST` | `/v1/autopilot/triggers/:id/toggle` | Enable/disable a trigger |
| `GET` | `/v1/autopilot/stats` | Daily spend, mission counts, agent activity |

## Supabase Schema

The SQL migration (`supabase/migrations/001_autopilot_schema.sql`) creates 8 tables:

| Table | Description |
|---|---|
| `proposals` | Proposal records with status tracking |
| `missions` | Mission records with heartbeat and deadline |
| `mission_steps` | Individual mission steps with status |
| `events` | Event log for triggers and audit |
| `triggers` | Trigger rule configuration |
| `content` | Published content (blog posts, etc.) |
| `agent_activity` | Agent activity log |
| `cap_gate_ledger` | Cost tracking for cap gate enforcement |

RLS policies grant full access to the service role (AgentZero VPS), read-only to anon (dashboard), and public read on published content only.
