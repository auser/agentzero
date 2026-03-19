---
title: MCP Server Integration
description: Connect external tool servers via the Model Context Protocol (MCP) — each MCP tool is registered as a first-class tool with its real name, description, and schema.
---

AgentZero integrates with [MCP (Model Context Protocol)](https://modelcontextprotocol.io/) servers as **first-class tools**. Each tool exposed by an MCP server is registered individually — the LLM sees `mcp__filesystem__read_file`, not a generic "call MCP" dispatcher.

## Quick Start

### 1. Define your servers

Create an `mcp.json` file. AgentZero discovers servers from two locations:

| Location | Path | Scope |
|---|---|---|
| **Global** | `~/.agentzero/mcp.json` | Available to all projects |
| **Project** | `{workspace}/.agentzero/mcp.json` | Project-specific servers |

Project servers override global ones with the same name.

```bash
mkdir -p ~/.agentzero
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
```

### 2. Enable MCP

Add to your `agentzero.toml`:

```toml
[security.mcp]
enabled = true
```

### 3. Verify

```bash
agentzero tools list | grep mcp
```

You should see tools like `mcp__filesystem__read_file`, `mcp__filesystem__write_file`, etc.

---

## mcp.json Format

The format matches the Claude Code / VS Code convention:

```json
{
  "mcpServers": {
    "server-name": {
      "command": "path/to/binary",
      "args": ["arg1", "arg2"],
      "env": {
        "API_KEY": "value"
      }
    }
  }
}
```

| Field | Required | Description |
|---|---|---|
| `command` | Yes | The executable to run (e.g. `npx`, `python3`, `/usr/local/bin/my-server`) |
| `args` | No | Command-line arguments passed to the executable |
| `env` | No | Environment variables set for the subprocess |

## Common MCP Servers

### Filesystem

Read and write files in a scoped directory:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-filesystem", "/path/to/allowed/dir"]
    }
  }
}
```

### GitHub

Create issues, PRs, search repos:

```json
{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "ghp_..." }
    }
  }
}
```

### Brave Search

Web search via Brave:

```json
{
  "mcpServers": {
    "brave-search": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-brave-search"],
      "env": { "BRAVE_API_KEY": "BSA..." }
    }
  }
}
```

### Custom Server

Any executable that speaks JSON-RPC 2.0 over stdio:

```json
{
  "mcpServers": {
    "my-tools": {
      "command": "/usr/local/bin/my-mcp-server",
      "args": ["--verbose"],
      "env": { "MY_CONFIG": "/etc/my-config.json" }
    }
  }
}
```

---

## Configuration Options

### agentzero.toml

```toml
[security.mcp]
enabled = true                     # master switch (default: false)
allowed_servers = []               # empty = all servers; non-empty = allowlist filter
```

When `allowed_servers` is non-empty, only servers whose names appear in the list are loaded. This lets you define many servers globally but restrict which ones a specific project can use.

### Environment Variable Override

The `AGENTZERO_MCP_SERVERS` environment variable provides a final override layer:

```bash
export AGENTZERO_MCP_SERVERS='{"my-server":{"command":"my-binary","args":[]}}'
```

This is useful for CI/CD or ephemeral environments. For normal use, prefer `mcp.json` files.

---

## Merge Order

When the same server name appears in multiple locations, later sources override earlier ones:

1. **Global** `~/.agentzero/mcp.json`
2. **Project** `{workspace}/.agentzero/mcp.json`
3. **Environment** `AGENTZERO_MCP_SERVERS`

---

## How It Works

### Tool Discovery

At startup, AgentZero:

1. Reads and merges all `mcp.json` sources
2. Filters by `allowed_servers` (if non-empty)
3. Connects to each server subprocess via stdio
4. Calls `tools/list` to discover available tools
5. Registers each tool as its own `McpIndividualTool` with:
   - **Name**: `mcp__{server}__{tool}` (e.g. `mcp__filesystem__read_file`)
   - **Description**: From the server's tool metadata
   - **Schema**: The `inputSchema` from `tools/list`, passed directly to the LLM

### Tool Naming

Tool names follow the pattern `mcp__{server}__{tool}`:

- Double underscores (`__`) separate the server name from the tool name
- Non-alphanumeric characters in tool names are replaced with `_`
- Example: server `brave-search`, tool `web_search` → `mcp__brave_search__web_search`

### Session Sharing

Multiple tools from the same server share a single connection (subprocess + stdin/stdout handles). The first tool call spawns the process; subsequent calls reuse it.

```
              Arc<McpServerConnection>
               (shared subprocess)
              /         |          \
mcp__fs__read    mcp__fs__write    mcp__fs__list
```

### Reconnection

If a connection error occurs during a tool call, the session is cleared and retried once automatically. If the retry also fails, the error is returned to the agent.

### Graceful Degradation

If a server fails to connect at startup (missing binary, timeout, protocol error), it is **skipped with a warning** — other servers and all built-in tools continue to work normally. This prevents one misconfigured server from blocking the entire agent.

---

## Security Considerations

- MCP servers run as **subprocesses** with the same OS permissions as the AgentZero process
- Use `allowed_servers` to restrict which servers are available in production
- Server environment variables (API keys, tokens) are passed only to that specific subprocess
- The `enabled = false` default means MCP is opt-in — no servers are loaded unless explicitly enabled
- Consider running AgentZero with reduced OS privileges when using MCP servers from untrusted sources

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| No `mcp__*` tools in `tools list` | MCP not enabled | Add `[security.mcp]` with `enabled = true` |
| Server not found | No `mcp.json` file | Create `~/.agentzero/mcp.json` or `.agentzero/mcp.json` |
| `npx` command not found | Node.js not installed | Install Node.js and npm |
| Server skipped at startup | Server failed to connect | Check the server command and args, run manually to debug |
| Tool call returns error | Server-side error | Enable `-vvv` for debug logs showing the JSON-RPC exchange |

---

## MCP Server Mode

AgentZero can also run **as** an MCP server, exposing its tools to any MCP client (Claude Desktop, Cursor, Windsurf, etc.).

### stdio Transport (Claude Desktop)

Run AgentZero as an MCP server over stdin/stdout:

```bash
agentzero mcp-serve
```

This reads JSON-RPC messages from stdin and writes responses to stdout. Configure it in Claude Desktop's MCP settings:

```json
{
  "mcpServers": {
    "agentzero": {
      "command": "agentzero",
      "args": ["mcp-serve"]
    }
  }
}
```

All tools registered in your `agentzero.toml` (file ops, shell, web search, etc.) become available as MCP tools.

### HTTP Transport (Gateway)

When the gateway starts with a config, the MCP server is automatically initialized. Use the HTTP endpoint:

```
POST /mcp/message
Content-Type: application/json

{"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}
```

### Tool Execution via REST API

The `POST /v1/tool-execute` endpoint executes tools directly (used by lite-mode nodes for remote delegation):

```bash
curl -X POST http://localhost:8080/v1/tool-execute \
  -H "Content-Type: application/json" \
  -d '{"tool": "read_file", "input": {"path": "/tmp/test.txt"}}'
```

### Supported MCP Methods

| Method | Description |
|--------|-------------|
| `initialize` | Handshake — returns server capabilities |
| `tools/list` | List all available tools with schemas |
| `tools/call` | Execute a tool by name with arguments |
| `ping` | Health check |
