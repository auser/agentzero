# Simplify AgentZero: Skills, Conversations, and Zero-Config

## Context

AgentZero is powerful but still lives in the "development lab." It's time to make it real and useful. The core loop we're optimizing for: **AI agents talking to each other autonomously, with humans able to jump into the conversation when they want to.**

Key principles from this simplification:
- **Skills as a marketplace** -- installable, shareable feature packs (the user's top-priority path)
- **Skills are the universal extension** -- a skill can provide any combination of: agents, tools, channels, `/` commands, config. Channels are just a skill capability, not a separate system.
- **Three implementation tiers** for skill-provided tools/channels:
  - **Built-in**: 57 native Rust tools + 20+ channels (compiled in, fastest)
  - **WASM**: sandboxed modules compiled from Rust/Go/C
  - **Script/HTTP bridge**: Python, Node, or any language via subprocess or HTTP endpoint
- **Local-first discovery** -- everything resolves: `$PWD/.agentzero/` (project) -> `~/.agentzero/` (global) -> built-in. A project can have its own skills, agents, tools, channels, and config without affecting other projects.
- **Config file is optional** -- the power underneath, not the primary interface
- **Autonomous agent-to-agent conversation** -- agents talk to each other without human involvement; humans *can* join but don't have to
- **Paperclip-style organization** -- inspired by [paperclip.ing](https://paperclip.ing/): agents in org hierarchies with heartbeats, tickets (threads), budgets, and governance
- **Simpler extension = better** -- make it trivial to add capabilities
- **Docs always current** -- site docs, SPRINT.md, and README.md updated with every code change

**Existing assets we build on:**
- `/` slash commands already exist in channels ([commands.rs](crates/agentzero-channels/src/commands.rs)) -- `/models`, `/model`, `/new`, `/approve`, `/help`
- `agents_ipc` tool already handles inter-agent messaging via event bus or file-based fallback
- Coordinator in orchestrator crate already runs multi-agent loops with routing + event bus
- WASM plugin system (`agentzero-plugins` + `agentzero-plugin-sdk`) for sandboxed tool extensions
- Sandboxing is handled by `mvm` (separate project), not replicated here
- Cost tracking, approval system, estop, audit logging all already exist

---

## Phase 1: Skills Marketplace

**The highest-impact change.** Make agentzero extensible through installable skill packs -- like NanoClaw's `/add-whatsapp` but as a proper marketplace with built-in and community skills.

### 1a. Skill package format

A skill is a directory with a standard structure:

```
<skills-dir>/<name>/
  skill.toml        # metadata + declares what this skill provides (tools, channels, commands)
  AGENT.md          # optional: agent definition (YAML frontmatter + system prompt)
  config.toml       # optional: config fragment to merge into main config
  install.sh        # optional: setup script (API keys, webhooks, dependencies)
  extensions/       # optional: WASM modules, Python/JS scripts, or HTTP bridge definitions
  templates/        # optional: prompt templates, example configs
  README.md         # usage instructions
```

**Extension methods** (tools, channels, or both -- all declared in `skill.toml`):
- **WASM**: `extensions/my_tool.wasm` -- sandboxed, compiled from Rust/Go/C
- **Script**: `extensions/my_tool.py` or `.js` -- subprocess execution (simplest for Python/Node devs)
- **HTTP bridge**: declared in `skill.toml` with an endpoint URL (any language, any host)

Example `skill.toml` for a Telegram skill:
```toml
[skill]
name = "telegram-bot"
version = "0.1.0"
description = "Telegram bot channel + management tools"
provides = ["channel", "tools", "agent"]

[channel]
name = "telegram"
type = "builtin"  # uses the compiled-in Telegram channel impl

[[tools]]
name = "telegram_admin"
type = "script"
path = "extensions/admin.py"
```

**Discovery order** (project-local wins, applies to ALL extensions):
1. `$PWD/.agentzero/skills/` -- project-local skills, agents, tools, channels
2. `$PWD/.agentzero/agents/` -- project-local agent definitions
3. `~/.agentzero/skills/` -- global user skills
4. `~/.agentzero/agents/` -- global user agents
5. Built-in (embedded in binary) -- 57 tools, 20+ channels, starter skill templates

Same-name conflicts: project-local overrides global overrides built-in.

`skill.toml` example:
```toml
[skill]
name = "telegram-bot"
version = "0.1.0"
description = "Telegram bot integration for AgentZero"
author = "agentzero-community"
keywords = ["telegram", "channel", "messaging"]
requires = ["channels-standard"]  # feature gate dependencies

[config]
# Config fragment merged into main config when skill is active
[config.channels.telegram]
# placeholder -- user fills in bot_token after install
```

### 1b. Skill lifecycle commands

**File**: Extend [cli.rs](crates/agentzero-cli/src/cli.rs) `SkillCommands`

```
agentzero skill list                          # list installed + available built-in skills
agentzero skill add telegram-bot              # install from built-in registry
agentzero skill add https://github.com/...    # install from git URL
agentzero skill add ./path/to/skill           # install from local path
agentzero skill remove telegram-bot           # uninstall
agentzero skill info telegram-bot             # show details, config requirements
agentzero skill update                        # update all installed skills
```

### 1c. Skill registry and discovery

**File**: New `crates/agentzero-config/src/skills.rs`

```rust
pub struct SkillManifest { name, version, description, config_fragment, agent_def, ... }

pub fn discover_skills(data_dir: &Path) -> Vec<SkillManifest>
pub fn install_skill(source: &SkillSource, data_dir: &Path) -> Result<SkillManifest>
pub fn remove_skill(name: &str, data_dir: &Path) -> Result<()>
pub fn merge_skill_configs(base: &AgentZeroConfig, skills: &[SkillManifest]) -> AgentZeroConfig
```

Skills discovered at startup; their config fragments merged into the main config. Their AGENT.md files become available agents.

### 1d. Built-in skill templates (shipped with the binary)

Embed a set of starter skills in the binary (via `include_str!` or a `skills/` directory in the repo):

| Skill | What it does |
|-------|-------------|
| `telegram-bot` | Telegram channel integration |
| `discord-bot` | Discord channel integration |
| `slack-bot` | Slack integration |
| `code-reviewer` | PR review agent with git + read + shell tools |
| `scheduler` | Natural language cron scheduling |
| `research-assistant` | Web search + summarization |
| `devops-monitor` | Health checks + alert routing |

`agentzero skill add code-reviewer` copies from built-in templates to `~/.agentzero/skills/`.

### 1e. Skill-provided `/` commands

**File**: Extend [commands.rs](crates/agentzero-channels/src/commands.rs)

Skills can register their own `/` commands by declaring them in `skill.toml`:

```toml
[[commands]]
name = "review"
description = "Start a code review on the current branch"
handler = "agent"  # routes to the skill's AGENT.md agent
```

These get merged into the channel command parser at startup.

### 1f. Site docs

**Files**: New `site/src/content/docs/guides/skills.md`

Document: what skills are, how to install/create/share them, skill.toml reference, built-in skill catalog.

---

## Phase 2: Agent Conversations (Agent-to-Agent + Human)

**Core loop**: agents talk to each other, humans can join. This builds on existing `agents_ipc` and the coordinator.

### 2a. Conversation-first agent model

**File**: New `crates/agentzero-config/src/agents.rs`

Agents defined as markdown files in `agents/` directory. **Only `name` is required** -- everything else has smart defaults:

```markdown
---
name: reviewer
---

You are a senior code reviewer. When asked to review code, analyze it for bugs, security issues, and style problems. Always cite specific line numbers.

When you find issues that need fixing, delegate to @writer with specific instructions.
```

**Defaults** (all fields optional):
- `model` -- inherits from main config provider
- `tools` -- **all available tools** (built-in + skill-provided). Only specify to restrict.
- `preset` -- defaults to `production` (full security). Only override when you need relaxed security.
- `listens_to` -- defaults to `["*"]` (everything: @-mentions, all event topics). Only specify to filter.
- `talks_to` -- defaults to all other discovered agents. Only specify to restrict.
- `heartbeat` -- optional cron expression for autonomous wake cycles
- `budget_usd_monthly` -- optional spending cap

**Philosophy**: agents are maximally capable by default. You only add config to *restrict*, not to *enable*. Agents and skills are auto-discoverable -- just drop a file in `agents/` and it works.

### 2b. `@agent` routing in all contexts

**File**: [runtime.rs](crates/agentzero-infra/src/runtime.rs) + [commands.rs](crates/agentzero-channels/src/commands.rs)

- CLI: `agentzero run "@reviewer check this PR"` -- routes to reviewer agent
- Channel: user sends `@reviewer check this PR` in Telegram -- routes to reviewer
- Agent-to-agent: reviewer's response contains `@writer fix the bug on line 42` -- auto-delegates

This is already partially supported via `DelegateTool` -- we wire it to also trigger on `@name` patterns in messages, not just explicit `delegate` tool calls.

### 2c. Conversation threads (transport-agnostic)

Agent conversations must work across machines, not just locally. The transport layer is:
- **Local**: file-based IPC (existing `agents_ipc.rs`) -- fast, simple
- **Distributed**: event bus (existing `SqliteEventBus` + `GossipEventBus` TCP mesh) -- cross-machine
- **Remote**: gateway HTTP/WebSocket API -- agents on entirely different hosts

**File**: Extend [agents_ipc.rs](crates/agentzero-tools/src/agents_ipc.rs) + event bus integration

Add `thread_id` to both `IpcMessage` and `Event` payloads:
- When agent A messages agent B, a thread is created
- Agent B's response carries the same thread_id
- Human can join by sending to the thread: `agentzero run --thread <id> "actually, use the other approach"`
- All messages in a thread are available as conversation context to participating agents
- `agents_ipc` tool auto-selects transport: event bus if available (handles TCP gossip), file IPC as fallback
- Remote agents reached via gateway HTTP when `talks_to` includes a URL: `talks_to: ["reviewer", "https://remote-host:8080/v1/agents/writer"]`

### 2d. Heartbeat-driven agent cycles (Paperclip-inspired)

Agents can run autonomously on a schedule without waiting for messages. Inspired by [Paperclip's heartbeat model](https://paperclip.ing/).

**How it works:**
- Agent definition gains optional `heartbeat` field: `heartbeat: "*/5 * * * *"` (cron expression)
- On each heartbeat tick, the agent wakes, checks for pending work (threads, events, tasks), executes, reports results
- Results published to event bus, triggering downstream agents
- Budget enforcement: each agent can have a `budget_usd_monthly` cap (existing cost tracking, extended to per-agent)
- If no work found, agent sleeps until next tick (no wasted API calls)

**This enables fully autonomous operation:**
1. Scheduler skill sends heartbeat events on cron schedule
2. Agent wakes, processes inbox (threads + events matching `listens_to`)
3. Agent does work, delegates to other agents via `talks_to`
4. Downstream agents wake on their next heartbeat or immediately via event bus
5. Human can observe via `/thread` or join any time

**File**: Extend coordinator in [coordinator.rs](crates/agentzero-orchestrator/src/coordinator.rs) + cron integration

### 2e. `/` commands for conversation management

**File**: Extend [commands.rs](crates/agentzero-channels/src/commands.rs)

Add to existing slash commands:
- `/agents` -- list available agents and their status
- `/talk <agent>` -- start a conversation with a specific agent
- `/thread` -- show current conversation thread
- `/join <thread>` -- join an ongoing agent conversation
- `/broadcast <message>` -- send to all agents

### 2e. Site docs

**File**: Update `site/src/content/docs/guides/multi-agent.md`

Document: agent conversation model, @-mention routing, thread management, human participation.

---

## Phase 3: Optional Config (Config File as Power Layer)

Make the config file optional -- it's the power underneath, not required for basic use.

### 3a. Auto-detect provider from env vars

**File**: [loader.rs](crates/agentzero-config/src/loader.rs)

Add `pub fn load_or_infer(path: &Path) -> anyhow::Result<AgentZeroConfig>`:
- If config file exists, use it (backward compat)
- If no config, build from env vars: `ANTHROPIC_API_KEY` -> anthropic, `OPENAI_API_KEY` -> openai, `OPENROUTER_API_KEY` -> openrouter
- Merge in any installed skill configs
- Merge in any discovered agent definitions

**File**: [model.rs](crates/agentzero-config/src/model.rs)

Add `AgentZeroConfig::inferred_from_env() -> Option<Self>`

### 3b. `agentzero run` -- simplest entry point

**File**: [cli.rs](crates/agentzero-cli/src/cli.rs)

```rust
Run {
    #[arg(trailing_var_arg = true)]
    message: Vec<String>,         // positional, no -m flag
    #[arg(long, default_value = "dev")]
    preset: String,               // sandbox | dev | full
    #[arg(long)]
    stream: bool,
}
```

Usage: `agentzero run write me a haiku` -- works with just an API key in env.

### 3c. Security presets

**File**: [lib.rs](crates/agentzero-tools/src/lib.rs) on `ToolSecurityPolicy`

```rust
pub fn preset_sandbox(root: PathBuf) -> Self  // read-only, no network
pub fn preset_dev(root: PathBuf) -> Self      // read+write+git+shell
pub fn preset_full(root: PathBuf) -> Self     // everything enabled
```

### 3d. Build runtime without config file

**File**: [runtime.rs](crates/agentzero-infra/src/runtime.rs)

Add `build_runtime_from_config(config: AgentZeroConfig, ...)` -- accepts in-memory config directly.

### 3e. Site docs

**File**: Update `site/src/content/docs/quickstart.md`, `site/src/content/docs/config/reference.md`

Document: zero-config mode, presets, env var auto-detection.

---

## Phase 4: CLI Simplification

### 4a. Promote essential commands, nest the rest under `admin`

**File**: [cli.rs](crates/agentzero-cli/src/cli.rs)

**Top-level (9 commands)**:
- `run` -- simplest entry point (new)
- `agent` -- single message with full control
- `agents` -- manage persistent agents
- `onboard` -- setup wizard
- `status` -- quick health check
- `auth` -- credential management
- `skill` -- skill marketplace
- `cron` -- scheduling
- `admin` -- all operational/infrastructure commands

**Under `agentzero admin`**:
- `gateway`, `daemon`, `service`, `estop`, `channel`, `tunnel`, `plugin`, `providers`, `hooks`, `integrations`, `local`, `models`, `approval`, `identity`, `coordination`, `cost`, `goals`, `doctor`, `migrate`, `update`, `config`, `memory`, `conversation`, `rag`, `hardware`, `peripheral`, `completions`, `dashboard`

Backward compat: `agentzero gateway` still works via clap `hide = true` + alias. Just hidden from top-level `--help`.

### 4b. Aliases

- `agentzero chat` -> `agentzero agent --stream`
- `agentzero ask "question"` -> `agentzero run "question"`
- `agentzero setup` -> `agentzero onboard`

### 4c. Site docs

**File**: Update `site/src/content/docs/reference/commands.md`

Document: new CLI structure, admin subcommand, aliases.

---

## Phase 5: Tool Registration Cleanup

### 5a. ToolRegistry builder

**File**: New `crates/agentzero-infra/src/tools/registry.rs`

```rust
pub struct ToolRegistry { tools: Vec<Box<dyn Tool>> }

impl ToolRegistry {
    pub fn new() -> Self
    pub fn with_core(self, policy) -> Self      // always-on: read, shell, glob, memory, etc.
    pub fn with_files(self, policy) -> Self      // write, edit, patch
    pub fn with_network(self, policy) -> Self    // web, http, fetch
    pub fn with_cron(self, policy) -> Self       // scheduling tools
    pub fn with_domain(self, policy) -> Self     // domain learning tools
    pub fn with_delegation(self, ...) -> Self    // delegate, ipc, sub-agent
    pub fn with_skill_tools(self, skills) -> Self // tools from installed skills
    pub fn with_preset(self, preset, policy) -> Self
    pub fn build(self) -> Vec<Box<dyn Tool>>
}
```

### 5b. Refactor `default_tools()` to use registry

**File**: [mod.rs](crates/agentzero-infra/src/tools/mod.rs)

```rust
ToolRegistry::new()
    .with_preset("default", policy)
    .with_delegation(router, delegates, agent_store)
    .with_skill_tools(&installed_skills)
    .build()
```

### 5c. Site docs

**File**: Update `site/src/content/docs/reference/tools.md`

Document: tool categories, how skills add tools, registry architecture.

---

## Critical Files

| File | Change |
|------|--------|
| [crates/agentzero-config/src/skills.rs](crates/agentzero-config/src/skills.rs) | New: skill registry, install, merge |
| [crates/agentzero-config/src/agents.rs](crates/agentzero-config/src/agents.rs) | New: markdown agent discovery, conversation model |
| [crates/agentzero-config/src/loader.rs](crates/agentzero-config/src/loader.rs) | `load_or_infer()`, skill config merging |
| [crates/agentzero-config/src/model.rs](crates/agentzero-config/src/model.rs) | `inferred_from_env()`, preset field |
| [crates/agentzero-tools/src/lib.rs](crates/agentzero-tools/src/lib.rs) | Security preset constructors |
| [crates/agentzero-tools/src/agents_ipc.rs](crates/agentzero-tools/src/agents_ipc.rs) | Thread IDs for agent conversations |
| [crates/agentzero-channels/src/commands.rs](crates/agentzero-channels/src/commands.rs) | New `/` commands, skill-provided commands |
| [crates/agentzero-infra/src/tools/mod.rs](crates/agentzero-infra/src/tools/mod.rs) | Registry refactor |
| [crates/agentzero-infra/src/tools/registry.rs](crates/agentzero-infra/src/tools/registry.rs) | New: ToolRegistry builder |
| [crates/agentzero-infra/src/runtime.rs](crates/agentzero-infra/src/runtime.rs) | `build_runtime_from_config()`, agent discovery, @-routing |
| [crates/agentzero-cli/src/cli.rs](crates/agentzero-cli/src/cli.rs) | `Run` command, `Admin` subcommand, skill commands |
| [crates/agentzero-cli/src/commands/run.rs](crates/agentzero-cli/src/commands/run.rs) | New: run command implementation |
| [site/src/content/docs/guides/skills.md](site/src/content/docs/guides/skills.md) | New: skills guide |
| [site/src/content/docs/guides/multi-agent.md](site/src/content/docs/guides/multi-agent.md) | Update: conversation model |
| [site/src/content/docs/quickstart.md](site/src/content/docs/quickstart.md) | Update: zero-config, run command |
| [site/src/content/docs/reference/commands.md](site/src/content/docs/reference/commands.md) | Update: CLI restructure |
| [site/src/content/docs/reference/tools.md](site/src/content/docs/reference/tools.md) | Update: tool registry, skill tools |
| [README.md](README.md) | Update: new CLI, skills, agent conversations |
| [specs/SPRINT.md](specs/SPRINT.md) | New sprint section with checkboxes |

## Verification

1. **Skill install**: `agentzero skill add code-reviewer` -- installs, shows in `skill list`
2. **Skill agent**: After install, `agentzero run "@code-reviewer check this"` routes to skill's agent
3. **Agent conversation**: Agent A delegates to Agent B, human joins with `--thread`
4. **`/` commands**: In channel, `/agents` lists agents, `/talk reviewer` starts conversation
5. **Zero-config**: `ANTHROPIC_API_KEY=test agentzero run hello` -- no TOML needed
6. **CLI**: `agentzero --help` shows 9 commands, `agentzero admin --help` shows the rest
7. **Backward compat**: `agentzero gateway`, `agentzero agent -m "hello"` still work
8. **Site docs**: All new features documented before merge
9. **Full test suite**: `cargo test --workspace` -- all tests pass
10. **Clippy clean**: `cargo clippy --workspace --all-targets -- -D warnings`

## Implementation Order

| Phase | Effort | Impact | Why this order |
|-------|--------|--------|----------------|
| 1: Skills marketplace | 3-4 days | Very High | User's top priority; makes extension trivial |
| 2: Agent conversations | 3-4 days | Very High | Core loop: agents talking + human engagement |
| 3: Optional config | 2-3 days | High | Removes the #1 barrier to "just use it" |
| 4: CLI simplify | 1-2 days | High | Quick win, reduces cognitive load |
| 5: Tool registry | 2-3 days | Medium | Internal cleanup, enables skill-provided tools |

Each phase includes site doc updates, SPRINT.md checkbox updates, and README.md updates. No phase ships without docs.

---

## Step 0: Save plan and update SPRINT.md

Before any code changes:
1. Create and switch to `feat/simplification` branch
2. Copy this plan to `specs/plans/13-simplification.md`
3. Add new sprint section to `specs/SPRINT.md` (after Sprint 46, before Backlog):

```markdown
## Sprint 47: Simplification — Skills Marketplace, Agent Conversations, Zero-Config

**Goal:** Transform AgentZero from a development lab into a tool people use daily. Skills marketplace for extensibility, first-class autonomous agent-to-agent conversations with human participation, optional config, and a clean CLI. Inspired by Paperclip's org-hierarchy/heartbeat model.

**Plan:** `specs/plans/13-simplification.md`

---

### Phase 1: Skills Marketplace (HIGH)

Installable, shareable skill packs. Built-in skills + community marketplace.

- [ ] **Skill package format** — `skill.toml` + `AGENT.md` + `config.toml` + tools + channels; per-project (`$PWD/.agentzero/skills/`) or global (`~/.agentzero/skills/`). Extensions via WASM, HTTP bridge, or script (Python/JS).
- [ ] **Skill lifecycle CLI** — `agentzero skill list/add/remove/info/update` commands
- [ ] **Skill registry & discovery** — `discover_skills()`, `install_skill()`, `merge_skill_configs()` in `agentzero-config`
- [ ] **Built-in skill templates** — `telegram-bot`, `discord-bot`, `slack-bot`, `code-reviewer`, `scheduler`, `research-assistant`, `devops-monitor`
- [ ] **Skill-provided `/` commands** — Skills declare commands in `skill.toml`, merged into channel command parser
- [ ] **Site docs** — `site/src/content/docs/guides/skills.md`

### Phase 2: Agent Conversations (HIGH)

First-class agent-to-agent communication with human participation.

- [ ] **Markdown agent definitions** — `agents/<name>.md` with YAML frontmatter. Only `name` required; defaults: all tools, all topics, all agents, production preset
- [ ] **Agent discovery** — `discover_agent_files()`, `parse_agent_markdown()` in `agentzero-config`
- [ ] **`@agent` routing** — CLI, channels, and agent-to-agent all support `@name` routing
- [ ] **Conversation threads** — `thread_id` on IPC messages + events, transport-agnostic (file IPC / event bus / HTTP)
- [ ] **Heartbeat-driven cycles** — Paperclip-inspired: agents wake on cron schedule, process inbox, delegate, sleep. Per-agent budget caps.
- [ ] **`/` conversation commands** — `/agents`, `/talk <agent>`, `/thread`, `/join <thread>`, `/broadcast`
- [ ] **Site docs** — Update `site/src/content/docs/guides/multi-agent.md`

### Phase 3: Optional Config (MEDIUM)

Config file becomes optional power layer, not required.

- [ ] **Auto-detect provider** — `load_or_infer()` in loader.rs, `inferred_from_env()` on config model
- [ ] **`agentzero run`** — Simplest entry point, positional message, no -m flag
- [ ] **Security presets** — `preset_sandbox()`, `preset_dev()`, `preset_full()` on `ToolSecurityPolicy`
- [ ] **Runtime from config** — `build_runtime_from_config()` accepts in-memory config
- [ ] **Site docs** — Update quickstart, config reference

### Phase 4: CLI Simplification (MEDIUM)

Clean UI with 9 top-level commands, rest under `admin`.

- [ ] **CLI restructure** — Top-level: run, agent, agents, onboard, status, auth, skill, cron, admin
- [ ] **Admin subcommand** — All operational commands nested under `agentzero admin`
- [ ] **Aliases** — `chat`, `ask`, `setup`
- [ ] **Backward compat** — Old commands work via hidden aliases
- [ ] **Site docs** — Update `site/src/content/docs/reference/commands.md`

### Phase 5: Tool Registry Cleanup (MEDIUM)

Builder pattern for tool registration, supports skill-provided tools.

- [ ] **ToolRegistry builder** — `with_core()`, `with_files()`, `with_network()`, `with_skill_tools()`, etc.
- [ ] **Refactor `default_tools()`** — Replace if-chain with registry builder
- [ ] **Site docs** — Update `site/src/content/docs/reference/tools.md`
- [ ] **README.md** — Update with new CLI commands, skills system, agent conversations
- [ ] **SPRINT.md** — Keep checkboxes current throughout implementation

### Future Enhancement: Markdown Config (Backlog)

Natural-language configuration via markdown. Instead of TOML, users write a free-form `.agentzero/config.md` and the agent loop interprets it at startup. Not in this sprint, captured for future exploration.

- [ ] **Markdown config parser** — LLM-powered config interpretation from natural language markdown
```
