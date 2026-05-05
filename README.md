# AgentZero

The secure operating layer for AI agents. Policy-controlled, audited, encrypted tool execution for local AI workflows.

## What it does

AgentZero lets local AI agents work with private files, code, tools, and secrets without bypassing policy, redaction, audit, or runtime isolation.

- Every tool call goes through policy evaluation
- Secrets never reach models (capability-based handles only)
- PII is redacted before remote calls
- All actions produce auditable events
- Content from tools is labeled as untrusted (ADR 0008)

## Install

```bash
cargo install --path crates/agentzero-cli
```

Or build from source:

```bash
git clone https://github.com/auser/agentzero
cd agentzero
cargo build --release
```

## Quick Start

```bash
# Initialize a project
agentzero init --private

# Chat with a local model (requires Ollama)
agentzero chat --local

# Or use llama.cpp / vLLM / LM Studio
agentzero chat --local --provider llama-cpp
agentzero chat --local --provider vllm --model my-model

# Run the security scanner
agentzero run repo-security-audit

# Start MCP server for Claude Code / Cursor
agentzero mcp
```

## MCP Integration

AgentZero works as an MCP server, providing policy-controlled tools to any MCP client:

```json
{
  "mcpServers": {
    "agentzero": {
      "command": "agentzero",
      "args": ["mcp"]
    }
  }
}
```

Tools available: `read_file`, `list_directory`, `search_files`, `write_file`, `run_command`

## Commands

| Command | Description |
|---------|-------------|
| `init --private` | Create project with encrypted policy |
| `chat --local` | Interactive chat with tool calling |
| `mcp` | Start MCP server for editor integration |
| `serve` | Start ACP server |
| `run <skill>` | Run an installed skill |
| `install <path/url>` | Install a skill from path or git URL |
| `doctor` | System diagnostic |
| `history` | List past sessions |
| `policy status` | Show active policy rules |
| `audit tail` | Show recent audit events |
| `vault add/get/remove/list` | Manage encrypted secrets |

## Architecture

```
agentzero              facade crate (re-exports all sub-crates)
├── agentzero-core     types, crypto, vault, trust labels, redaction
├── agentzero-policy   rule-based policy engine + TOML loader
├── agentzero-audit    JSONL + encrypted audit logging
├── agentzero-session  session engine, providers, tool executor, router, retry
├── agentzero-tools    tool registry + schemas
├── agentzero-skills   manifests, scanner, report generator, registry
├── agentzero-sandbox  sandbox profiles + WASM runtime (feature flag)
├── agentzero-mcp      MCP server (JSON-RPC 2.0 over stdio)
├── agentzero-acp      ACP adapter for editor integration
├── agentzero-tracing  centralized tracing/logging
└── agentzero-cli      CLI binary
```

## Security Model

- **Deny by default**: unknown permissions are denied
- **Local first**: all inference local unless policy explicitly allows remote
- **Secret handles**: models see `handle://vault/github/token`, never raw values
- **Redaction**: PII stripped before remote model calls
- **Content provenance**: tool output marked as untrusted (ADR 0008)
- **Encrypted at rest**: AES-256-GCM + Argon2id for vault, sessions, audit
- **Approval flow**: dangerous operations (shell, file write) require user consent

## Providers

| Provider | Type | Default Port |
|----------|------|-------------|
| Ollama | Native API | 11434 |
| llama.cpp | OpenAI-compatible | 8080 |
| vLLM | OpenAI-compatible | 8000 |
| LM Studio | OpenAI-compatible | 1234 |

## Development

```bash
just ci          # Run full CI (docs, check, test, clippy, fmt)
just test        # Run tests only
just clippy      # Lint only
cargo run -- doctor  # Check system status
```

## Governance

Architecture decisions are tracked in `specs/adrs/` (10 accepted ADRs). See `AGENTS.md` for contribution rules.

## License

MIT
