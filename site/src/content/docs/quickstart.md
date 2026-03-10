---
title: Quick Start
description: Get AgentZero installed, configured, and running — from first install to production deployment.
---

This guide walks you through every step to get AgentZero running, from installation through production deployment.

## Prerequisites

You need **one** of the following:

- **curl + bash** — to install a pre-built binary (Linux, macOS, WSL)
- **Rust 1.80+** — to build from source (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

You also need an API key from at least one AI provider (OpenRouter, OpenAI, Anthropic, etc.). If you want to use a local model (Ollama, LM Studio), no API key is needed.

---

## 1. Install

### Option A: Pre-built binary (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash
```

This auto-detects your platform and installs to `~/.cargo/bin` (or `~/.local/bin`). Options:

```bash
# Install a specific version
curl -fsSL ... | bash -s -- --version 0.2.0

# Install to a custom directory
curl -fsSL ... | bash -s -- --dir /usr/local/bin

# Install shell completions (bash, zsh, fish)
curl -fsSL ... | bash -s -- --completions zsh
```

### Option B: Build from source

```bash
git clone https://github.com/auser/agentzero.git
cd agentzero
cargo build -p agentzero --release
```

The binary is at `target/release/agentzero`. Move it somewhere on your `$PATH`:

```bash
cp target/release/agentzero ~/.cargo/bin/
```

### Verify installation

```bash
agentzero --help
```

---

## 2. Configure

The `onboard` command creates your `agentzero.toml` config file in the current directory.

### Interactive wizard

```bash
agentzero onboard --interactive
```

This walks you through choosing a provider, model, memory backend, and security settings.

### Non-interactive (scripted)

```bash
agentzero onboard \
  --provider openrouter \
  --model anthropic/claude-sonnet-4-6 \
  --memory sqlite \
  --allowed-root . \
  --allowed-commands ls,pwd,cat,echo \
  --yes
```

The generated `agentzero.toml` contains three required sections:

```toml
[provider]
kind = "openrouter"
base_url = "https://openrouter.ai/api/v1"
model = "anthropic/claude-sonnet-4-6"

[memory]
backend = "sqlite"
sqlite_path = "./agentzero.db"

[security]
allowed_root = "."
allowed_commands = ["ls", "pwd", "cat", "echo"]
```

To view or modify your config later:

```bash
agentzero config show          # View effective config
agentzero config get provider.model   # Get a single value
agentzero config set provider.model gpt-4o  # Change a value
```

---

## 3. Authenticate

You need to give AgentZero credentials for your AI provider. Pick **any one** of the methods below — they all work equally well.

### Option A: `auth` command (recommended)

The built-in auth store saves your credentials securely and supports multiple providers and profiles:

```bash
# Paste an API key (prompts interactively if --token is omitted)
agentzero auth setup-token --provider openrouter
agentzero auth setup-token --provider anthropic --token sk-ant-...
agentzero auth setup-token --provider openai --token sk-...

# Or use OAuth login (for providers that support it)
agentzero auth login --provider openai-codex     # OpenAI browser login
agentzero auth login --provider anthropic        # Claude browser login
```

Manage saved profiles:

```bash
agentzero auth list                          # List all profiles
agentzero auth status                        # Show active profile
agentzero auth use --provider anthropic --profile default  # Switch active profile
agentzero auth logout --provider openrouter  # Remove a profile
```

### Option B: Environment variable

Set `OPENAI_API_KEY` — this is the universal env var name for **all** cloud providers (OpenAI, OpenRouter, Anthropic, etc.):

```bash
export OPENAI_API_KEY="sk-..."       # OpenAI
export OPENAI_API_KEY="sk-or-v1-..." # OpenRouter
export OPENAI_API_KEY="sk-ant-..."   # Anthropic
```

### Option C: `.env` file

Create a `.env` file in your config directory (`~/.agentzero/`) or the current working directory:

```bash
OPENAI_API_KEY=sk-or-v1-...
```

AgentZero loads `.env` and `.env.local` from both the config directory and the current working directory. CWD files take priority over config-dir files, so you can set global defaults in `~/.agentzero/.env` and override per-project in a local `.env`.

### Local models (no credentials needed)

If you're using Ollama or another local provider, skip this step entirely:

```bash
agentzero onboard --provider ollama --model llama3.1:8b --yes
agentzero agent -m "Hello"
```

---

## 4. Send Your First Message

```bash
agentzero agent -m "What is the capital of France?"
```

Override the provider or model for a single request:

```bash
agentzero agent -m "Hello" --provider openai --model gpt-4o-mini
agentzero agent -m "Hello" --profile my-anthropic-profile
```

---

## 5. Verify Everything Works

```bash
# Quick status check (shows memory count)
agentzero status

# Diagnose model availability across providers
agentzero doctor models

# List supported providers
agentzero providers

# Refresh and list available models
agentzero models refresh
agentzero models list
```

---

## 6. Running in Production

AgentZero offers three ways to run as a server, from simple to fully managed:

### Gateway (foreground)

Start the HTTP gateway directly. Good for testing and development:

```bash
agentzero gateway --host 127.0.0.1 --port 8080
```

The gateway exposes these endpoints:

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check |
| `/api/chat` | POST | Send a chat message |
| `/v1/chat/completions` | POST | OpenAI-compatible completions API |
| `/v1/models` | GET | List available models |
| `/ws/chat` | GET | WebSocket chat |
| `/pair` | POST | Pair a client (get bearer token) |
| `/v1/ping` | POST | Connectivity check |
| `/metrics` | GET | Prometheus-style metrics |

Test it:

```bash
# Health check
curl http://127.0.0.1:8080/health

# Pair (get a bearer token for subsequent requests)
curl -X POST http://127.0.0.1:8080/pair
```

### Daemon (background process)

Run the gateway as a background daemon. Logs to `daemon.log` in your data directory:

```bash
# Start the daemon
agentzero daemon start --port 8080

# Check if it's running
agentzero daemon status

# View logs
agentzero daemon status --json   # shows log file path

# Stop the daemon
agentzero daemon stop
```

For debugging, run the daemon in the foreground:

```bash
agentzero daemon start --foreground
```

### Service (auto-start on boot)

Install AgentZero as a system service (systemd or OpenRC, auto-detected):

```bash
# Install and start
agentzero service install
agentzero service start

# Check status
agentzero service status

# Restart after config changes
agentzero service restart

# Remove completely
agentzero service stop
agentzero service uninstall
```

Force a specific init system:

```bash
agentzero service install --service-init systemd
agentzero service install --service-init openrc
```

---

## 7. Run a Full Agent Workflow

The agent automatically calls tools in a loop until it completes your task. Each turn it can use `shell`, `read_file`, `glob_search`, `content_search`, and more — chaining them as needed.

### Example: multi-step task

```bash
agentzero agent -m "Find all Rust source files containing TODO comments and show each one with its line number"
```

Internally the agent:

1. Calls `glob_search` → finds `**/*.rs` files
2. Calls `content_search` → searches for `TODO` across results
3. Calls `read_file` → reads flagged files for context
4. Returns a formatted answer

Max tool iterations are controlled by `agent.max_tool_iterations` in your config (default: 20).

### Control verbosity

```bash
agentzero -vvv agent -m "your task"   # debug: see every tool call
agentzero -vvvv agent -m "your task"  # trace: see full request/response
```

---

## 8. Manage Skills

Skills are pre-built tool bundles that extend what the agent can do.

```bash
agentzero skill list              # See installed skills
agentzero skill install <name>    # Install a skill
agentzero skill test <name>       # Run smoke test
agentzero skill remove <name>     # Uninstall
```

---

## 9. Add a New Tool

Tools are Rust structs that implement the `Tool` trait from `agentzero-core`.

### 1. Implement the trait

Create `crates/agentzero-infra/src/tools/my_tool.rs`:

```rust
use agentzero_core::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;

pub struct MyTool;

#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &'static str { "my_tool" }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success(format!("Processed: {input}")))
    }
}
```

### 2. Register it

In `crates/agentzero-infra/src/tools/mod.rs`, declare the module and add to `default_tools()`:

```rust
pub mod my_tool;

pub fn default_tools() -> Vec<Box<dyn Tool>> {
    vec![
        // ... existing tools ...
        Box::new(my_tool::MyTool),
    ]
}
```

### 3. Write a test

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_basic() {
        let ctx = ToolContext::test_default();
        let result = MyTool.execute("hello", &ctx).await.unwrap();
        assert!(result.is_success());
    }
}
```

### 4. Verify

```bash
cargo test -p agentzero-infra my_tool
cargo clippy -p agentzero-infra
```

---

## 10. Explore More Commands

```bash
# Interactive terminal dashboard
agentzero dashboard

# Manage scheduled tasks
agentzero cron list
agentzero cron add --id daily-check --schedule "0 9 * * *" --command "agentzero agent -m 'morning report'"

# Memory inspection
agentzero memory list
agentzero memory stats

# Shell completions
agentzero completions --shell zsh >> ~/.zshrc
agentzero completions --shell bash >> ~/.bashrc

# Check for updates
agentzero update check
```

### Developer quality gate

If you're building from source and making changes:

```bash
just ci           # fmt-check + clippy + nextest (full gate)
just fmt          # auto-format code
just lint         # clippy with warnings as errors
just test         # run all tests with cargo nextest
just test-verbose # tests with full output
just bench        # run benchmarks
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `missing API key for provider 'X'` | API key not set | `export OPENAI_API_KEY="your_key"` or `agentzero auth login` |
| `config file not found` | No `agentzero.toml` in current dir | Run `agentzero onboard` |
| Tool execution denied | Security defaults are fail-closed | Edit `[security]` in `agentzero.toml` |
| Provider timeout | Network or rate limit issue | Check with `-vvv` for debug logs |
| Daemon won't start | Port already in use | `agentzero daemon stop` then retry, or use a different `--port` |

Debug with verbose output:

```bash
# Increasing verbosity: -v (error) → -vv (info) → -vvv (debug) → -vvvv (trace)
agentzero -vvv agent -m "test"

# Run full model diagnostics
agentzero doctor models

# View recent trace events
agentzero doctor traces --limit 10
```

---

## Environment Variables Reference

| Variable | Purpose | Required |
|---|---|---|
| `OPENAI_API_KEY` | API key for all cloud providers (OpenAI, OpenRouter, Anthropic, etc.) | Yes (or auth store) |
| `AGENTZERO_DATA_DIR` | Override the `~/.agentzero/` data directory | No |
| `AGENTZERO_CONFIG` | Override the config file path | No |
| `AGENTZERO_ENV` | Select an env-specific `.env.{env}` overlay file | No |
| `BRAVE_API_KEY` | Enable Brave web search | No |
| `JINA_API_KEY` | Enable Jina web fetch | No |
| `AGENTZERO_MCP_SERVERS` | JSON map of MCP server configs (override layer; prefer `mcp.json` files) | No |

---

## Next Steps

- [Configuration Reference](/agentzero/config/reference/) — Full annotated `agentzero.toml`
- [CLI Commands](/agentzero/reference/commands/) — Complete command reference
- [Architecture](/agentzero/architecture/) — How it all fits together
