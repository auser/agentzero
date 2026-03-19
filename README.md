# AgentZero

A lightweight, security-first agent runtime and CLI built entirely in Rust. Single binary, 50+ tools, encrypted storage, fail-closed security defaults, and trait-driven extensibility.

## Quick start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash

# Configure (interactive wizard)
agentzero onboard --interactive

# Set your API key (or use `agentzero auth setup-token`)
export OPENAI_API_KEY="sk-..."

# Send a message
agentzero agent -m "Hello, what can you do?"
```

### Use a local model (no API key needed)

```bash
agentzero onboard --provider ollama --model llama3.1:8b --yes
agentzero agent -m "Hello"
```

### Use Anthropic directly

```bash
agentzero onboard --provider anthropic --model claude-sonnet-4-6 --yes
export ANTHROPIC_API_KEY="sk-ant-..."
agentzero agent -m "Hello"
```

### Use OpenRouter (access all models)

```bash
agentzero onboard --provider openrouter --model anthropic/claude-sonnet-4-6 --yes
export OPENAI_API_KEY="sk-or-v1-..."
agentzero agent -m "Hello"
```

## What it does

AgentZero runs an agentic loop: you send a message, the agent reasons about it, calls tools (file I/O, shell, search, web, git, etc.), and returns a result. It can chain multiple tool calls in a single turn.

```bash
# Multi-step task — the agent figures out the tool calls
agentzero agent -m "Find all Rust files with TODO comments and list them with line numbers"

# Streaming output
agentzero agent -m "Explain this codebase" --stream

# Override provider/model for one request
agentzero agent -m "Hello" --provider openai --model gpt-4o

# Debug mode — see every tool call
agentzero -vvv agent -m "your task"
```

## Features at a glance

| | |
|---|---|
| **Language** | 100% Rust — single binary, millisecond cold starts |
| **Providers** | Anthropic, OpenAI, OpenRouter, Ollama, LM Studio, vLLM, LlamaCPP |
| **Tools** | 50+ built-in (file I/O, shell, git, web search, browser, memory, delegation, MCP, and more) |
| **Memory** | SQLite with SQLCipher encryption (default), Turso/libsql (optional) |
| **Channels** | Telegram, Discord, Slack, Matrix, Email, IRC, Nostr, Webhook, and more |
| **Plugins** | WASM sandbox with integrity verification |
| **MCP** | First-class Model Context Protocol — each MCP tool registered individually |
| **Security** | Fail-closed defaults, allowlists, OTP gating, audit trail, secret redaction, estop |
| **Sandbox** | Isolated execution environments for untrusted workloads (start/stop/status/shell) |
| **Gateway** | HTTP/WebSocket server with OpenAI-compatible API |
| **Dashboard** | Interactive TUI for live agent monitoring, run tracking, and tool call timelines |
| **Multi-agent** | Delegation, swarm coordination, pipelines, event bus, persistent named agents, A2A remote agents |

## Configuration

AgentZero uses `agentzero.toml` for configuration. The `onboard` command generates one interactively, or copy an example:

```bash
# Minimal config
cp examples/config-basic.toml ./agentzero.toml

# Full reference (every option documented)
cp examples/config-full.toml ./agentzero.toml
```

### Minimal config

```toml
[provider]
kind = "openrouter"
model = "anthropic/claude-sonnet-4-6"

[security]
allowed_root = "."
allowed_commands = ["ls", "pwd", "cat", "echo", "grep", "find", "git", "cargo"]

[gateway]
port = 42617
```

### Enable tools (security defaults are fail-closed)

```toml
[security.write_file]
enabled = true                    # allow the agent to write files

[security.mcp]
enabled = true                    # enable MCP server tools

[web_search]
enabled = true                    # enable web search
provider = "duckduckgo"

[browser]
enabled = true                    # enable browser automation
```

### MCP servers

MCP server definitions go in `mcp.json` files (global or per-project):

```bash
# Global — available to all projects
cat > ~/.agentzero/mcp.json << 'EOF'
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-filesystem", "/tmp"]
    }
  }
}
EOF

# Enable in agentzero.toml
# [security.mcp]
# enabled = true
```

Each MCP tool appears as `mcp__{server}__{tool}` (e.g. `mcp__filesystem__read_file`).

### Multi-agent delegation

```toml
[agents.coder]
provider = "anthropic"
model = "claude-sonnet-4-6"
max_depth = 2
agentic = true
allowed_tools = ["shell", "read_file", "write_file", "file_edit"]

[agents.researcher]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
agentic = true
allowed_tools = ["web_search", "web_fetch", "read_file"]
```

### Persistent agent management

Create named agents at runtime — from the CLI, the browser config UI, or via natural language during a conversation:

```bash
# Create a persistent agent
agentzero agents create --name Aria --description "Travel planner" \
  --model claude-sonnet-4-20250514 --provider anthropic --keywords travel,booking

# List agents
agentzero agents list

# Update an agent
agentzero agents update --id agent_abc123 --model gpt-4o --keywords travel,flights

# Delete an agent
agentzero agents delete --id agent_abc123
```

Enable the LLM-callable `agent_manage` tool so agents can create other agents during conversation:

```toml
[agent]
enable_agent_manage = true
```

### Channel integrations

```toml
[channels.telegram]
bot_token = "YOUR_TELEGRAM_BOT_TOKEN"
allowed_users = []

[channels.slack]
bot_token = "xoxb-YOUR-SLACK-BOT-TOKEN"
app_token = "xapp-YOUR-SLACK-APP-TOKEN"

[channels.discord]
bot_token = "YOUR_DISCORD_BOT_TOKEN"
```

## Authentication

Three ways to provide credentials (use whichever you prefer):

```bash
# Option 1: Auth store (recommended — supports multiple profiles)
agentzero auth setup-token --provider openrouter
agentzero auth setup-token --provider anthropic --token sk-ant-...

# Option 2: Environment variable
export OPENAI_API_KEY="sk-..."

# Option 3: .env file (in ~/.agentzero/ or current directory)
echo 'OPENAI_API_KEY=sk-...' >> ~/.agentzero/.env
```

Manage profiles:

```bash
agentzero auth list                          # list all profiles
agentzero auth status                        # show active profile
agentzero auth use --provider anthropic      # switch active profile
```

## Running in production

```bash
# Foreground gateway
agentzero gateway --host 127.0.0.1 --port 8080

# Background daemon
agentzero daemon start --port 8080
agentzero daemon status
agentzero daemon stop

# System service (systemd/OpenRC, auto-detected)
agentzero service install
agentzero service start
```

Gateway endpoints:

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check |
| `/api/chat` | POST | Send a chat message |
| `/v1/chat/completions` | POST | OpenAI-compatible API |
| `/v1/models` | GET | List models |
| `/ws/chat` | GET | WebSocket chat |
| `/pair` | POST | Client pairing (get bearer token) |
| `/metrics` | GET | Prometheus metrics |

## CLI commands

```bash
agentzero onboard          # Interactive setup
agentzero agent -m "..."   # Send a message
agentzero agents list      # Manage persistent agents
agentzero gateway          # Start HTTP gateway
agentzero status           # Quick status check
agentzero doctor models    # Diagnose model availability
agentzero providers        # List supported providers
agentzero models list      # List available models
agentzero config show      # View effective config
agentzero auth list        # Manage credentials
agentzero cron list        # Manage scheduled tasks
agentzero memory list      # Inspect memory store
agentzero tools list       # List available tools
agentzero dashboard        # Interactive terminal dashboard
agentzero sandbox start    # Isolated sandbox environment
agentzero daemon start     # Background daemon
agentzero service install  # OS service (systemd/OpenRC)
agentzero plugin list      # Manage WASM plugins
agentzero skill list       # Manage skills
agentzero estop engage     # Emergency stop
agentzero cost show        # Usage tracking
agentzero completions zsh  # Shell completions
```

All commands support `--json` for structured output and `-v`/`-vv`/`-vvv`/`-vvvv` for increasing verbosity.

## Workspace layout (16 crates)

| Crate | Purpose |
|---|---|
| `bin/agentzero` | Binary entrypoint |
| `agentzero-core` | Traits (`Provider`, `Tool`, `MemoryStore`, `Channel`), types, security policy |
| `agentzero-config` | TOML config, loader, policy validation |
| `agentzero-storage` | Encrypted persistence (SQLCipher, XChaCha20Poly1305, Turso) |
| `agentzero-providers` | LLM providers (Anthropic, OpenAI-compatible) |
| `agentzero-auth` | Authentication profiles (API keys, OAuth) |
| `agentzero-tools` | 50+ tool implementations |
| `agentzero-infra` | Runtime orchestration, tool wiring, MCP |
| `agentzero-channels` | Channel backends + outbound leak guard |
| `agentzero-plugins` | WASM plugin runtime with integrity checks |
| `agentzero-plugin-sdk` | Plugin SDK for third-party authors |
| `agentzero-gateway` | HTTP/WebSocket gateway |
| `agentzero-orchestrator` | Swarm coordination, event bus, pipelines |
| `agentzero-ffi` | FFI bindings (Swift/Kotlin/Python/Node) |
| `agentzero-cli` | CLI commands and UX |
| `agentzero-testkit` | Shared test utilities |
| `agentzero-bench` | Benchmark suite |

## Security

- **Fail-closed defaults** — tools are denied unless explicitly enabled
- **Filesystem scoping** — all file access confined to `allowed_root`
- **Shell allowlists** — only explicitly listed commands can execute
- **Encrypted storage** — SQLCipher for memory, XChaCha20Poly1305 for JSON stores
- **OTP-gated tools** — require TOTP verification for sensitive operations
- **Audit trail** — log every tool call to a tamper-evident log
- **Secret redaction** — outbound leak guard blocks sensitive data from leaving
- **Emergency stop** — `agentzero estop engage` freezes all tool execution
- **WASM sandbox** — plugins run with fuel metering, memory limits, and capability gates

## Build from source

```bash
git clone https://github.com/auser/agentzero.git
cd agentzero
cargo build -p agentzero --release
./target/release/agentzero --help
```

### Feature flags

```bash
# Default (includes SQLite, plugins, gateway, TUI)
cargo build -p agentzero --release

# Minimal (SQLite memory only)
cargo build -p agentzero --release --no-default-features --features minimal

# With Turso cloud memory
cargo build -p agentzero --release --features memory-turso

# With all channel integrations
cargo build -p agentzero --release --features channels-standard

# With hardware inspection tools
cargo build -p agentzero --release --features hardware

# With privacy features (Noise protocol, sealed envelopes)
cargo build -p agentzero --release --features privacy

# Lightweight privacy-first binary for edge devices (Raspberry Pi, etc.)
# Defaults to "private" mode: network tools blocked, Noise auto-enabled
cargo build -p agentzero-lite --release
```

### Quality gates

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Examples

See [examples/](examples/) for ready-to-use configurations:

- **[config-basic.toml](examples/config-basic.toml)** — Minimal 3-section config
- **[config-full.toml](examples/config-full.toml)** — Every option documented
- **[business-office/](examples/business-office/)** — 7-agent AI office (CEO, CTO, CSO, Marketing, Legal, Finance, HR)
- **[research-pipeline/](examples/research-pipeline/)** — 4-agent research pipeline (Researcher, Scraper, Analyst, Writer)

## Documentation

- [Always-On Agent in 5 Minutes](https://auser.github.io/agentzero/guides/always-on/) — Zero to a Telegram/Discord/Slack bot with tools
- [Quick Start](https://auser.github.io/agentzero/quickstart/) — Installation through production deployment
- [Configuration Reference](https://auser.github.io/agentzero/config/reference/) — Full annotated `agentzero.toml`
- [CLI Commands](https://auser.github.io/agentzero/reference/commands/) — Complete command reference
- [Architecture](https://auser.github.io/agentzero/architecture/) — Crate boundaries and trait design
- [Tools Reference](https://auser.github.io/agentzero/reference/tools/) — All 50+ tools with schemas
- [Security Threat Model](https://auser.github.io/agentzero/security/threat-model/) — Per-threat analysis

## License

MIT / Apache-2.0
