---
title: Skills Guide
description: Install, create, and manage skills — AgentZero's universal extension mechanism for reusable agent behaviors.
---

Skills are AgentZero's universal extension mechanism. A skill is a self-contained package that bundles an agent persona, tool configuration, and optional extensions (WASM plugins, scripts, HTTP bridges) into a single installable unit. Skills are higher-level than plugins — where a plugin provides a single tool, a skill provides an entire behavior.

## Quick Start

```bash
# Install a skill from the built-in catalog
agentzero skill add code-reviewer

# List installed skills
agentzero skill list

# Use it — the agent gains the skill's tools and persona
agentzero agent -m "Review the changes in the last commit"

# Remove a skill
agentzero skill remove code-reviewer
```

---

## Skill Package Format

A skill is a directory containing a `skill.toml` manifest and supporting files:

```
code-reviewer/
├── skill.toml          # Skill metadata and configuration
├── AGENT.md            # Agent persona and instructions
├── config.toml         # Default tool and security settings
└── extensions/         # Optional tool implementations
    ├── lint-check.wasm # WASM plugin
    ├── format.sh       # Script tool
    └── bridge.json     # HTTP bridge definition
```

### skill.toml

The manifest describes the skill and declares its dependencies:

```toml
[skill]
name = "code-reviewer"
version = "0.2.0"
description = "Reviews code for bugs, style issues, and security problems"
author = "agentzero-project"
license = "MIT"
keywords = ["code", "review", "lint", "security"]

[skill.requires]
min_agentzero = "0.3.0"
tools = ["read_file", "glob_search", "content_search", "git_operations"]
features = []                    # optional feature gates (e.g., "rag")

[skill.provides]
tools = ["lint-check"]           # tools provided by extensions/
commands = []                    # CLI subcommands (future)
```

### AGENT.md

The agent persona file. When a skill is active, its `AGENT.md` content is injected into the system prompt:

```markdown
You are a code reviewer. When asked to review code, follow these steps:

1. Read the changed files using `git_operations` to get the diff
2. Analyze each change for correctness, security, and style
3. Use `content_search` to check for related patterns in the codebase
4. Provide specific feedback with file paths and line numbers

Be concise. Flag only genuine issues, not style preferences.
```

### config.toml

Default configuration that the skill applies. Users can override any setting in their `agentzero.toml`:

```toml
[security]
enable_git = true

[security.write_file]
enabled = false                  # reviewer reads, does not write

[agent]
max_tool_iterations = 15
```

### extensions/

Optional directory containing tool implementations. Supported formats:

| Type | File | Description |
|---|---|---|
| WASM plugin | `*.wasm` + `manifest.json` | Sandboxed tool (same as regular plugins) |
| Script tool | `*.sh` / `*.py` | Executed as a subprocess with stdin/stdout JSON |
| HTTP bridge | `*.json` | Proxies tool calls to an HTTP endpoint |

---

## Installing Skills

### From the built-in catalog

```bash
agentzero skill add code-reviewer
agentzero skill add scheduler
agentzero skill add research-assistant
```

### From a Git repository

```bash
agentzero skill add --url https://github.com/user/my-skill.git
```

### From a local directory

```bash
agentzero skill add --path ./my-custom-skill
```

### Managing installed skills

```bash
agentzero skill list              # List all installed skills
agentzero skill info code-reviewer  # Show skill details
agentzero skill update            # Update all skills
agentzero skill update code-reviewer  # Update a specific skill
agentzero skill remove code-reviewer  # Uninstall
agentzero skill test code-reviewer    # Run the skill's smoke test
```

---

## Built-in Skills Catalog

AgentZero ships with a catalog of ready-to-use skills:

| Skill | Description | Key Tools |
|---|---|---|
| `code-reviewer` | Reviews code for bugs, style, and security issues | `read_file`, `git_operations`, `content_search` |
| `scheduler` | Manages cron jobs and one-time scheduled tasks | `cron_add`, `cron_list`, `schedule` |
| `research-assistant` | Researches topics using web search and summarization | `web_search`, `web_fetch`, `memory_store` |
| `telegram-bot` | Pre-configured Telegram bot persona with group handling | Channel integration |
| `discord-bot` | Pre-configured Discord bot with thread support | Channel integration |
| `slack-bot` | Pre-configured Slack bot with Socket Mode setup | Channel integration |
| `devops-monitor` | Monitors infrastructure and sends alerts | `shell`, `http_request`, `cron_add` |

Install any of them:

```bash
agentzero skill add research-assistant
```

---

## Creating Custom Skills

### 1. Scaffold a new skill

```bash
mkdir my-skill && cd my-skill
```

### 2. Create skill.toml

```toml
[skill]
name = "my-skill"
version = "0.1.0"
description = "Does something useful"
author = "you"
keywords = ["custom"]

[skill.requires]
tools = ["read_file", "shell"]

[skill.provides]
tools = []
```

### 3. Write the agent persona

Create `AGENT.md` with instructions for the agent when this skill is active:

```markdown
You are a specialist in [domain]. When asked to perform [task], follow these steps:

1. First, gather context by reading relevant files
2. Then, execute the necessary commands
3. Finally, summarize the results

Always explain your reasoning before taking action.
```

### 4. Add default configuration

Create `config.toml` with sensible defaults:

```toml
[security]
allowed_commands = ["ls", "pwd", "cat", "grep"]

[agent]
max_tool_iterations = 10
```

### 5. (Optional) Add extension tools

If your skill needs custom tools, add them to `extensions/`:

```bash
mkdir extensions
# Add a WASM plugin, script, or HTTP bridge
```

### 6. Install and test locally

```bash
agentzero skill add --path .
agentzero skill test my-skill
```

---

## Workflow Packs

A workflow pack is a skill that defines a coordination graph — multiple agents working together in a pipeline or swarm pattern. The pack's `config.toml` includes swarm agent definitions, pipeline steps, and routing rules.

### Example: review-and-deploy pack

```toml
# config.toml
[swarm]
enabled = true

[swarm.agents.reviewer]
name = "Reviewer"
description = "Reviews code changes"
provider = "anthropic"
model = "claude-sonnet-4-6"
allowed_tools = ["read_file", "git_operations", "content_search"]
max_iterations = 15

[swarm.agents.tester]
name = "Tester"
description = "Runs tests and checks coverage"
provider = "anthropic"
model = "claude-sonnet-4-6"
allowed_tools = ["shell", "read_file"]
max_iterations = 20

[[swarm.pipelines]]
name = "review-and-test"
steps = ["reviewer", "tester"]
channel_reply = true
on_step_error = "abort"

[swarm.pipelines.trigger]
keywords = ["review", "check", "PR"]
```

Install the pack and the entire multi-agent workflow is ready:

```bash
agentzero skill add --path ./review-and-deploy
agentzero agent -m "Review and test the latest changes"
```

---

## Discovery Order

Skills are discovered from three locations, checked in priority order (later overrides earlier):

| Priority | Path | Scope |
|---|---|---|
| 1 (lowest) | Built-in catalog | Ships with AgentZero |
| 2 | `~/.agentzero/skills/` | Global (user-wide) |
| 3 (highest) | `.agentzero/skills/` | Project-local |

A project-local skill with the same name as a global or built-in skill takes priority. This lets you override or customize built-in skills per project.

### Directory structure

```
~/.agentzero/skills/
├── code-reviewer/
│   ├── skill.toml
│   ├── AGENT.md
│   └── config.toml
└── research-assistant/
    ├── skill.toml
    ├── AGENT.md
    ├── config.toml
    └── extensions/
        └── summarize.wasm
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| Skill not appearing in `skill list` | Not installed | Run `agentzero skill add <name>` |
| Skill tools not available | Required tools not enabled in security policy | Check `skill.requires.tools` against your `agentzero.toml` security settings |
| Extension WASM fails to load | Missing `wasm32-wasip1` target | Rebuild with `cargo build --target wasm32-wasip1` |
| Skill persona not applied | Conflicting system prompt | The skill's `AGENT.md` is appended to, not replacing, your configured system prompt |
| `skill test` fails | Missing dependencies | Check `skill.requires` and ensure all required tools and features are available |
