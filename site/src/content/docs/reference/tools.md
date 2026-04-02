---
title: Tools & Plugins
description: Built-in tools, security policy, WASM plugin system, and skills.
---

AgentZero ships with 50+ built-in tools and supports extension via WASM plugins, process plugins, MCP servers, skills, and **dynamic tools** (runtime-created tools that persist across sessions). Every tool enforces **fail-closed security** — capabilities are denied unless explicitly enabled. All tools implement `input_schema()` for structured tool-use APIs (Anthropic `tool_use`, OpenAI function calling).

## Tool Tiers

Tools are organized into three tiers that control which tools are compiled into the binary. This is especially relevant for resource-constrained deployments (e.g., Raspberry Pi, embedded devices).

| Tier | Description | Included Tools |
|---|---|---|
| **Core** | Essential agent tools — always included | `read_file`, `shell`, `glob_search`, `content_search`, `memory_store`, `memory_recall`, `memory_forget`, `task_plan` |
| **Extended** | Standard tools for most deployments (default) | Core + `write_file`, `file_edit`, `apply_patch`, `git_operations`, `web_search`, `web_fetch`, `http_request`, `browser`, `delegate`, `converse`, `cron_*`, `subagent_*` |
| **Full** | All 50+ tools including hardware, SOP, WASM, and integration tools | Extended + `hardware_*`, `sop_*`, `wasm_*`, `composio`, `pushover`, `schedule` |

Use the `embedded-minimal` feature flag to build with only the **Core** tier, producing a significantly smaller binary suitable for edge devices:

```bash
cargo build -p agentzero --release --no-default-features --features embedded-minimal
```

The default build includes the **Extended** tier. To include all tools, enable the `full-tools` feature.

## Built-in Tools

### Always Enabled

| Tool | Description |
|---|---|
| `read_file` | Read file contents within allowed root |
| `shell` | Execute allowlisted shell commands |
| `glob_search` | Find files by glob pattern |
| `content_search` | Search file contents with regex |
| `memory_store` | Store entries in agent memory |
| `memory_recall` | Recall entries from agent memory |
| `memory_forget` | Remove entries from agent memory |
| `image_info` | Extract image metadata |
| `docx_read` | Read DOCX file contents |
| `pdf_read` | Read PDF file contents |
| `screenshot` | Capture screen screenshots |
| `task_plan` | Create and manage task plans |
| `process_tool` | Execute external processes |
| `subagent_spawn` | Spawn background sub-agents |
| `subagent_list` | List running sub-agents |
| `subagent_manage` | Manage sub-agent lifecycle |
| `cli_discovery` | Discover CLI capabilities |
| `proxy_config` | Query proxy configuration |
| `delegate_coordination_status` | Check delegate coordination status |
| `sop_list` | List Standard Operating Procedures |
| `sop_status` | Check SOP execution status |
| `sop_advance` | Advance SOP to next step |
| `sop_approve` | Approve SOP step |
| `sop_execute` | Execute an SOP |
| `hardware_board_info` | Query hardware board information |
| `hardware_memory_map` | Read hardware memory map |
| `hardware_memory_read` | Read hardware memory |
| `wasm_module` | Load WASM modules |
| `wasm_tool_exec` | Execute WASM tool |

### Policy-Gated (Disabled by Default)

| Tool | Description | Config |
|---|---|---|
| `write_file` | Write file contents within allowed root | `[security.write_file]` |
| `file_edit` | Edit files with search/replace | Enabled with `write_file` |
| `apply_patch` | Validate and apply structured patches | Enabled with `write_file` |
| `git_operations` | Git operations (status, diff, log, etc.) | `enable_git` |
| `http_request` | Make HTTP requests to allowed domains | `[http_request]` |
| `web_fetch` | Fetch and convert web pages to markdown | `[web_fetch]` |
| `url_validation` | Validate URLs against access policy | `[url_access]` |
| `web_search` | Search the web via DuckDuckGo/Brave/etc | `[web_search]` |
| `browser` | Browser automation and screenshot | `[browser]` |
| `browser_open` | Open URLs in system browser | `[browser]` |
| `cron_add` | Add a cron schedule | `[cron]` |
| `cron_list` | List cron schedules | `[cron]` |
| `cron_remove` | Remove a cron schedule | `[cron]` |
| `cron_update` | Update a cron schedule | `[cron]` |
| `cron_pause` | Pause a cron schedule | `[cron]` |
| `cron_resume` | Resume a cron schedule | `[cron]` |
| `schedule` | Schedule one-time tasks | `[cron]` |
| `composio` | Composio integration | `enable_composio` |
| `pushover` | Pushover notifications | `enable_pushover` |
| `agent_manage` | Create, list, update, or delete persistent named agents. Supports `create_from_description` for NL agent definitions. | `enable_agent_manage` |
| `tool_create` | Create, list, delete, export, or import dynamic tools at runtime. Supports shell, HTTP, LLM, and composite strategies. | `enable_dynamic_tools` |
| `proposal_create` | Create autopilot proposals for agent-driven work | `[autopilot]` |
| `proposal_vote` | Approve or reject autopilot proposals | `[autopilot]` |
| `mission_status` | Query autopilot mission status | `[autopilot]` |
| `trigger_fire` | Manually fire an autopilot trigger | `[autopilot]` |

### Conditionally Registered

| Tool | Description | Condition |
|---|---|---|
| `agents_ipc` | Inter-process communication between agents | `enable_agents_ipc` (default: true) |
| `converse` | Multi-turn conversations between agents or with humans | When `"converse"` in `allowed_tools` |
| `mcp__{server}__{tool}` | MCP server tools (one per remote tool) | `[security.mcp]` + `mcp.json` |
| `model_routing_config` | Query model routing configuration | When router is configured |
| `delegate` | Spawn sub-agent with scoped tools | When `[agents.*]` configured |

## Tool Trait

All tools implement the core `Tool` trait:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str { "" }
    fn input_schema(&self) -> Option<serde_json::Value> { None }
    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult>;
}
```

The `input_schema()` method returns a JSON Schema describing expected input parameters. When provided, this enables structured tool-use APIs (Anthropic `tool_use`, OpenAI function calling) and input validation before execution. All 50+ built-in tools implement this method.

### CLI Introspection

```bash
agentzero tools list                  # List all registered tools
agentzero tools list --with-schema    # Include JSON schemas
agentzero tools list --json           # Machine-readable output
agentzero tools info read_file        # Show details for a specific tool
agentzero tools schema read_file      # Print the JSON schema
agentzero tools schema shell --pretty # Pretty-printed schema
```

The `ToolContext` carries workspace-scoped security state:

```rust
pub struct ToolContext {
    pub workspace_root: String,
    pub allow_sensitive_file_reads: bool,
    pub allow_sensitive_file_writes: bool,
    pub sender_id: Option<String>,
    // ... additional fields omitted for brevity
}
```

---

## File Tools

### `read_file`

Reads text file contents with full path-safety enforcement.

**Input:** Relative file path (e.g., `src/main.rs`)

**Security controls:**

- **Path traversal prevention** — rejects `..` components and absolute paths
- **Canonicalization** — resolves symlinks and validates the final path stays within `allowed_root`
- **Hard-link guard (B7)** — blocks files with multiple hard links to prevent symlink attacks
- **Binary detection** — rejects files containing null bytes or non-UTF8 content
- **Sensitive file detection** — blocks `.env`, `.aws/credentials`, `.ssh/id_rsa`, `.gnupg/`, `credentials.json`, etc. unless `allow_sensitive_file_reads` is true
- **Size cap** — 64 KiB per read (configurable)

```toml
[security.read_file]
allowed_root = "."          # workspace root
max_read_bytes = 65536      # 64 KiB
allow_binary = false
```

### `write_file`

Writes text files with the same path-safety enforcement as `read_file`.

**Input:** JSON payload:

```json
{
  "path": "src/output.txt",
  "content": "file contents here",
  "overwrite": false,
  "dry_run": false
}
```

**Security controls:**

- **Disabled by default** — requires explicit `enabled = true`
- All `read_file` protections apply (path traversal, canonicalization, hard-link guard, sensitive file detection)
- **Existence check** — errors if file exists and `overwrite: false`
- **Dry-run mode** — preview capability without writing to disk

**Output:** `dry_run={bool} path={path} bytes={count} overwrite={bool}`

```toml
[security.write_file]
enabled = true
allowed_root = "."
max_write_bytes = 65536     # 64 KiB
```

### `apply_patch`

Validates and applies structured patches using a strict envelope format.

**Input:** Patch content with BEGIN/END markers:

```
*** Begin Patch
*** Update File: src/main.rs
@@ ... patch content ...
*** End Patch
```

**Security:** Validates patch structure before any file modification. Rejects malformed envelopes.

---

## Shell Tool

### `shell`

Executes shell commands with **allowlist-driven** security. Only explicitly permitted commands can run.

**Input:** Command string (e.g., `ls -la src/`)

**Security controls:**

- **Command allowlist** — only commands listed in `allowed_commands` can execute
- **Default allowlist:** `ls`, `pwd`, `cat`, `echo`
- **Quote-aware validation** — metacharacters (`; & | > < $`) are forbidden when **unquoted**, but allowed inside single/double quotes
- **Always forbidden** — backtick (`` ` ``) and null byte are blocked even inside quotes
- **Argument limits** — max 8 arguments of 128 bytes each
- **Output truncation** — stdout/stderr capped at 8 KiB with truncation notice

**Output:** `status={code}\nstdout:\n...\nstderr:\n...`

**Examples:**

```bash
# Allowed — semicolon is inside single quotes
echo 'hello;world'

# Blocked — unquoted semicolon is shell injection
echo hello;world

# Blocked — backtick is always forbidden
echo `whoami`
```

**Shell tokenizer:** The command parser performs quote-aware tokenization supporting single quotes (no interpretation), double quotes (with escape handling), and backslash escapes. Each character's quoting context is tracked for policy validation.

```toml
[security]
allowed_commands = ["ls", "pwd", "cat", "echo", "grep", "find", "git"]

[security.shell]
max_args = 8
max_arg_length = 128
max_output_bytes = 8192
forbidden_chars = ";&|><$`\n\r"
```

---

## Network Tools

All network tools share the **URL Access Policy** for SSRF prevention.

### URL Access Policy

```toml
[url_access]
block_private_ip = true        # blocks 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
allow_loopback = false         # blocks 127.0.0.0/8
enforce_domain_allowlist = false
domain_allowlist = []
domain_blocklist = []
```

**Private IP ranges blocked:**

| Range | Description |
|---|---|
| `10.0.0.0/8` | Private Class A |
| `172.16.0.0/12` | Private Class B |
| `192.168.0.0/16` | Private Class C |
| `169.254.0.0/16` | Link-local |
| `100.64.0.0/10` | Carrier-grade NAT |
| `0.0.0.0/8` | Unspecified |
| `240.0.0.0/4` | Reserved |
| `fc00::/7` | IPv6 unique local |
| `fe80::/10` | IPv6 link-local |

**DNS rebinding protection:** Domain names are resolved to IP addresses and checked against the private IP blocklist. This prevents attackers from registering a domain, then changing DNS to point to an internal IP.

### `http_request`

Performs HTTP requests (GET, POST, PUT, DELETE) to allowed domains.

**Input:** `<METHOD> <URL> [JSON_BODY]`

```
GET https://api.example.com/data
POST https://api.example.com/items {"name": "item1"}
```

**Security pipeline:**

```
Input → URL Parse → Scheme Check (http/https only) →
Policy Check (blocklist) → IP Resolution → Private IP Check →
DNS Rebinding Check → Domain Allowlist Check → Execute
```

```toml
[http_request]
enabled = true
allowed_domains = ["api.example.com", "*.internal.dev"]
max_response_size = 1000000   # 1 MB
timeout_secs = 30
```

### `web_fetch`

Fetches content from URLs and returns the response body.

**Input:** URL string

**Output:** `status={code}\n{body}`

**Security:** Same URL access policy as `http_request`. Automatic response truncation at 64 KiB default.

### `web_search`

Searches the web via configurable provider.

```toml
[web_search]
enabled = true
provider = "duckduckgo"       # duckduckgo, brave, perplexity, exa, jina
max_results = 5
timeout_secs = 15
```

---

## Agent Tools

### `delegate`

Spawns a sub-agent with scoped tools. Prevents infinite delegation chains.

**Input:**

```json
{
  "agent": "researcher",
  "prompt": "Find the latest Rust async runtime benchmarks"
}
```

**Security controls:**

- **Depth limiting** — `max_depth` prevents infinite delegation chains
- **Tool blocklist** — the `delegate` tool itself is forbidden in sub-agent tool lists (prevents recursion)
- **Scoped tool access** — each delegate configuration specifies its own `allowed_tools`

**Per-agent configuration:**

```toml
[agents.researcher]
provider = "https://api.openai.com/v1"
model = "gpt-4"
max_depth = 2
agentic = true
max_iterations = 10
allowed_tools = ["read_file", "web_search", "memory"]
```

### `converse`

Multi-turn bidirectional conversations between agents or with humans via channels. Each call is one turn — the calling agent controls the flow.

**Input:**

```json
{
  "agent": "analyst",
  "message": "What do you think about these findings?",
  "conversation_id": "conv-researcher-analyst-001"
}
```

For human-in-the-loop:

```json
{
  "channel": "slack",
  "recipient": "#engineering",
  "message": "Should we proceed with approach A or B?",
  "conversation_id": "conv-approval-001"
}
```

**Parameters:**

| Parameter | Required | Description |
|---|---|---|
| `agent` | One of `agent`/`channel` | Target agent ID |
| `channel` | One of `agent`/`channel` | Target channel for human conversation |
| `recipient` | With `channel` | Channel recipient |
| `message` | Yes | The message to send |
| `conversation_id` | Yes | Shared across turns (generate on first turn, reuse for follow-ups) |

**Safety controls:**

- **Turn limit** — configurable `max_turns` per conversation (default: 10)
- **Per-turn timeout** — `turn_timeout_secs` (default: 120s)
- **Budget limits** — inherited token/cost limits
- **Loop detection** — catches repetitive conversation patterns
- **Leak guard** — responses scanned for credential leaks

**Configuration:**

```toml
[swarm.agents.researcher.conversation]
max_turns = 15
turn_timeout_secs = 120
```

### `agents_ipc`

Inter-process communication between agents with encrypted storage.

**Operations:**

| Operation | Input | Description |
|---|---|---|
| `send` | `{"op":"send","from":"a1","to":"a2","payload":"msg"}` | Send message between agents |
| `recv` | `{"op":"recv","to":"agent_name"}` | Receive pending messages |
| `list` | `{"op":"list","to":"?","from":"?","limit":"?"}` | List messages with filters |
| `clear` | `{"op":"clear","to":"?","from":"?"}` | Clear messages |

**Security:** All IPC data is encrypted at rest using `EncryptedJsonStore`. Messages include timestamps for audit trail.

### `model_routing`

Queries the model routing configuration to determine which provider/model handles a request.

**Operations:**

| Operation | Description |
|---|---|
| `list_routes` | Returns all model route hints |
| `list_embedding_routes` | Returns all embedding route hints |
| `resolve_hint` | Find route by hint name |
| `classify_query` | Classify query to appropriate hint |
| `route_query` | Complete routing decision for query |

---

## Autonomy Levels

Tools are gated by the autonomy policy, which controls what requires user approval:

| Level | Read Tools | Write Tools | Network Tools |
|---|---|---|---|
| `ReadOnly` | Auto-approve | **Blocked** | **Blocked** |
| `Supervised` | Auto-approve | Requires approval | Requires approval |
| `Full` | Auto-approve | Auto-approve | Auto-approve |

**Read tools (auto-approved):** `read_file`, `glob`, `search`, `memory_read`

**Write tools (gated):** `write_file`, `shell`, `apply_patch`, `browser`, `http_request`

**Forbidden paths (all levels):** `/etc`, `/root`, `/proc`, `/sys`, `~/.ssh`, `~/.gnupg`, `~/.aws`

```toml
[autonomy]
level = "supervised"
workspace_only = true
forbidden_paths = ["/etc", "/root", "/proc", "/sys"]
auto_approve = ["read_file", "memory"]
always_ask = ["shell", "write_file"]
allow_sensitive_file_reads = false
allow_sensitive_file_writes = false
```

---

## WASM Plugins

AgentZero runs plugins in a sandboxed WASM environment with WASI capabilities, strict resource limits, and SHA-256 integrity verification. Plugins implement the same `Tool` trait as native tools — the agent loop treats them identically.

### Four Extension Mechanisms

| Mechanism | Isolation | Overhead | Use Case |
|---|---|---|---|
| **WASM plugins** | Sandboxed (memory + CPU + capability-gated) | ~1-5ms (cached) | Third-party tools, community plugins |
| **FFI plugins** | Host process (not sandboxed) | Native | Embedding in Swift/Kotlin/Python/Node.js apps |
| **Process plugins** | Full (OS process) | ~5-50ms | Any-language tools via stdin/stdout JSON |
| **MCP servers** | Full (separate process) | Network | Tool server interoperability |

### Writing a Plugin (10 Lines)

```rust
use agentzero_plugin_sdk::prelude::*;

declare_tool!("my_tool", execute);

fn execute(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = serde_json::from_str(&input.input)
        .unwrap_or_default();
    let name = req["name"].as_str().unwrap_or("world");
    ToolOutput::success(format!("Hello, {name}!"))
}
```

Build: `cargo build --target wasm32-wasip1 --release`

For a complete example with typed input, `az_log` host calls, `ToolOutput::with_warning`, and WASI filesystem access, see the **reference notepad plugin** at `plugins/agentzero-plugin-reference/notepad/`.

See the [Plugin Authoring Guide](/guides/plugins/) for the full walkthrough.

### Plugin Discovery

Plugins are auto-discovered from three locations (later overrides earlier):

| Path | Scope | Hot-Reload |
|---|---|---|
| `~/.local/share/agentzero/plugins/` | Global (user-wide) | No |
| `$PROJECT/.agentzero/plugins/` | Project-specific | No |
| `./plugins/` | Current working directory (development) | Yes |

### Plugin Lifecycle

```bash
agentzero plugin new --id my-tool --scaffold rust   # Scaffold project
agentzero plugin test --manifest manifest.json --wasm plugin.wasm --execute  # Test
agentzero plugin package --manifest manifest.json --wasm plugin.wasm  # Package
agentzero plugin install --package my-tool.tar       # Install from file
agentzero plugin install my-tool                     # Install from registry
agentzero plugin list                                # List installed
agentzero plugin enable <id> / disable <id>          # Toggle state
agentzero plugin search <query>                      # Search registry
agentzero plugin remove --id my-tool                 # Remove
```

### Security Controls

```toml
[runtime.wasm]
fuel_limit = 1000000         # execution budget
memory_limit_mb = 64         # max memory
max_module_size_mb = 50      # max .wasm file size
allow_workspace_read = false
allow_workspace_write = false
allowed_hosts = []           # network access allowlist

[runtime.wasm.security]
require_workspace_relative_tools_dir = true
reject_symlink_modules = true
reject_symlink_tools_dir = true
capability_escalation_mode = "deny"
module_hash_policy = "warn"  # warn or enforce
```

### WASM Isolation Policy

- Network: **disabled** by default
- Filesystem write: **disabled** by default
- WASI capabilities: granted per-plugin via manifest + policy
- Bounded execution: epoch-based CPU timeout (default: 30s)
- Bounded memory: configurable max memory (default: 256MB)
- Capability validation: undeclared imports fail at load time (not runtime)
- SHA-256 integrity: verified on every install and load

For the full ABI specification, host callbacks, and manifest schema, see the [Plugin API Reference](/reference/plugin-api/).

---

## Dynamic Tools

Dynamic tools are created at runtime by agents and **persist across sessions**. Over time, the system accumulates a library of tools it invented — each encrypted at rest in `.agentzero/dynamic-tools.json`.

### Creating a Tool via Conversation

During any agent session, the agent can call `tool_create` to invent a new tool:

```
You: "I need to transcribe audio files using Whisper"

Agent thinks: No transcription tool exists. I'll create one.
Agent calls tool_create:
{
  "action": "create",
  "description": "A tool that transcribes audio/video files using OpenAI Whisper CLI",
  "strategy_hint": "shell"
}

→ LLM derives the tool definition automatically:
  name: whisper_transcribe
  strategy: shell
  command_template: whisper {{input}} --output_format txt

→ Tool registered immediately — available in this session and every future session.
```

The agent can then use `whisper_transcribe` as a tool in the same conversation, without restarting.

### Creating a Tool via the Gateway API

```bash
# The agent calls tool_create internally, but you can also
# ask the agent to create tools via the gateway:
curl -X POST http://localhost:3000/v1/agent \
  -H "Content-Type: application/json" \
  -d '{
    "message": "Create a tool that checks the weather using wttr.in"
  }'
```

### Execution Strategies

Each dynamic tool wraps one of four execution strategies:

#### Shell Strategy

Executes a shell command with `{{input}}` placeholder substitution.

```json
{
  "name": "youtube_download",
  "description": "Download a YouTube video using yt-dlp",
  "strategy": {
    "type": "shell",
    "command_template": "yt-dlp -o /tmp/%(title)s.%(ext)s {{input}}"
  }
}
```

When the agent calls `youtube_download` with input `"https://youtube.com/watch?v=abc"`, the system runs:
```bash
yt-dlp -o /tmp/%(title)s.%(ext)s https://youtube.com/watch?v=abc
```

#### HTTP Strategy

Calls an HTTP endpoint with the tool input as the request body.

```json
{
  "name": "sentiment_api",
  "description": "Analyze text sentiment via external API",
  "strategy": {
    "type": "http",
    "url": "https://api.example.com/v1/sentiment",
    "method": "POST",
    "headers": {
      "Authorization": "Bearer sk-...",
      "Content-Type": "application/json"
    }
  }
}
```

#### LLM Strategy

Delegates to the LLM with a specialized system prompt. Useful for analysis, review, or transformation tasks that don't need external tools.

```json
{
  "name": "code_reviewer",
  "description": "Review code for bugs, security issues, and style",
  "strategy": {
    "type": "llm",
    "system_prompt": "You are an expert code reviewer. Analyze the following code for bugs, security vulnerabilities, performance issues, and style violations. Be specific and actionable."
  }
}
```

#### Composite Strategy

Chains existing tools sequentially — each step's output becomes the next step's input.

```json
{
  "name": "video_to_summary",
  "description": "Download a video, transcribe it, then summarize",
  "strategy": {
    "type": "composite",
    "steps": [
      { "tool_name": "youtube_download" },
      { "tool_name": "whisper_transcribe" },
      { "tool_name": "code_reviewer", "input_override": "Summarize this transcript" }
    ]
  }
}
```

### Managing Dynamic Tools

The `tool_create` tool supports five actions:

| Action | Description | Example Input |
|---|---|---|
| `create` | Create a new tool from NL description | `{"action": "create", "description": "...", "strategy_hint": "shell"}` |
| `list` | List all dynamic tools with their strategies | `{"action": "list"}` |
| `delete` | Remove a dynamic tool by name | `{"action": "delete", "name": "whisper_transcribe"}` |
| `export` | Export a tool as shareable JSON | `{"action": "export", "name": "whisper_transcribe"}` |
| `import` | Import a tool from JSON (single or array) | `{"action": "import", "json": "{...}"}` |

### Sharing Tools Between Instances

Export a tool on machine A:
```
You: "Export the youtube_download tool"
Agent: Here's the tool definition:
{
  "name": "youtube_download",
  "description": "Download a YouTube video using yt-dlp",
  "strategy": { "type": "shell", "command_template": "yt-dlp -o /tmp/%(title)s.%(ext)s {{input}}" },
  "created_at": 1711234567
}
```

Import it on machine B:
```
You: "Import this tool: { ... paste the JSON ... }"
Agent calls tool_create:
{"action": "import", "json": "{ ... }"}
→ Tool registered and persisted.
```

### How Dynamic Tools Are Discovered

1. **At startup:** `build_runtime_execution()` loads all tools from `.agentzero/dynamic-tools.json` into the tool list alongside built-in tools. The LLM sees them identically.
2. **Mid-session:** The `ToolSource` trait on `DynamicToolRegistry` feeds newly created tools into `build_tool_definitions()` on each agent loop iteration. A tool created via `tool_create` is visible to the LLM on the very next turn.
3. **Tool selection:** Both `KeywordToolSelector` and `HintedToolSelector` match against dynamic tools by name and description — they participate in tool filtering just like built-in tools.
4. **Recipe learning:** When a dynamic tool is used successfully, the `RecipeStore` records it. Future goals matching similar patterns will boost that tool automatically.

### Configuration

Enable dynamic tool creation in `agentzero.toml`:

```toml
[agent]
enable_dynamic_tools = true
```

### Security

- Only **root agents** (depth=0) can create tools — sub-agents cannot
- Shell-strategy tools are validated against the `ShellPolicy` (command allowlists, path restrictions)
- HTTP-strategy tools are validated against the `UrlAccessPolicy` (domain allowlists, private IP blocking)
- LLM-strategy tools use the same provider and billing as the parent agent
- All definitions are encrypted at rest via `EncryptedJsonStore`

### Codegen Strategy

The agent can write Rust source code, compile it to WASM, and load it as a sandboxed tool — all at runtime. This produces tools as capable as hand-written ones, isolated inside the WASM sandbox.

```json
{
  "name": "markdown_to_html",
  "strategy": {
    "type": "codegen",
    "source": "use agentzero_plugin_sdk::prelude::*;\ndeclare_tool!(\"markdown_to_html\", handler);\nfn handler(input: ToolInput) -> ToolOutput { ... }",
    "wasm_path": ".agentzero/codegen/markdown_to_html/target/wasm32-wasip1/release/markdown_to_html.wasm",
    "wasm_sha256": "a1b2c3..."
  }
}
```

The compilation pipeline retries up to 3 times — if `cargo build` fails, errors are fed back to the LLM for correction. A curated allowlist of dependencies (`serde_json`, `regex`, `chrono`, `url`, `base64`, `sha2`, `hex`, `rand`, `csv`) keeps compile times predictable and prevents supply-chain risk.

### Auto-Evolution

Dynamic tools improve themselves over time:

- **Auto-Fix**: Tools with >60% failure rate and 5+ invocations are automatically repaired via LLM-based strategy correction. The evolver provides rich error context including quality stats, generation history, and multi-strategy pivot suggestions (e.g., "this Shell tool keeps failing — try HTTP instead").
- **Auto-Improve**: Tools with >80% success rate and 10+ invocations get optimized variants (`tool_v2`, `tool_v3`). The original is preserved; the variant competes on quality.
- **Anti-loop protections**: One evolution per tool per session, max 5 evolutions per session, generation caps prevent infinite repair loops.

### Tool Gap Detection

The `RecipeStore` monitors failure patterns across sessions. When the same goal pattern fails 3+ times with no matching successful recipe, the system detects a **tool gap** — a recurring need with no tool to fulfill it. This feeds into proactive tool creation: "I noticed we keep failing at PDF conversion, so I'll create a tool for that."

### Insights Report

The `insights_report` tool lets the agent query its own performance history:

```
You: "How are my tools performing?"
Agent calls insights_report: {"focus": "tools"}

→ ## Tool Usage Heatmap
  - **shell**: 142 uses (94% success, 9 failures)
  - **web_fetch**: 87 uses (98% success, 2 failures)
  - **write_file**: 63 uses (100% success, 0 failures)
```

Available focus modes: `summary`, `models`, `tools`, `failures`, `cost`.

### Checkpoint Recovery

File-mutating tools (`write_file`, `apply_patch`, `file_edit`) automatically snapshot the target file before modification. Checkpoints are stored as plain file copies in `.agentzero/checkpoints/<session>/<timestamp>/`. Pre-rollback snapshots enable "undo the undo" — restoring a file also snapshots its current state first.

### Tool Middleware

Tools can be wrapped with composable pre/post interceptors — the same pattern as the provider `LlmLayer` pipeline. Built-in middleware includes:

- **TimingMiddleware**: Logs execution duration and success/failure for every tool call
- **RateLimitMiddleware**: Blocks tool execution when invoked too frequently within a configurable window
- **CheckpointMiddleware**: Snapshots files before write operations (see above)

Custom middleware implements the `ToolMiddleware` trait with `before()` and `after()` hooks.

### Persistence

Dynamic tools survive restarts, updates, and reboots. The encrypted store at `.agentzero/dynamic-tools.json` is the system's growing tool library — portable and backupable.

---

## Skills

Skills are higher-level composable behaviors built on top of tools.

```bash
# List installed skills
agentzero skill list

# Install a skill
agentzero skill install --name my-skill --source local

# Test a skill
agentzero skill test --name my-skill

# Remove a skill
agentzero skill remove --name my-skill
```

---

## MCP (Model Context Protocol)

MCP servers are integrated as **first-class tools**. Each tool exposed by an MCP server is registered individually with a namespaced name (`mcp__{server}__{tool}`), its real description, and its full input schema. The LLM sees and invokes them just like any built-in tool.

### Configuration

MCP server definitions live in dedicated `mcp.json` files, discovered from two locations:

| Location | Path | Scope |
|---|---|---|
| **Global** | `~/.agentzero/mcp.json` | Available to all projects |
| **Project** | `{workspace}/.agentzero/mcp.json` | Project-specific servers |

Both files are optional. Project servers override global ones with the same name. The `AGENTZERO_MCP_SERVERS` env var is supported as a final override layer.

**`mcp.json` format** (matches Claude Code / VS Code convention):

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-filesystem", "/tmp"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "ghp_..." }
    }
  }
}
```

Enable MCP in `agentzero.toml` (the kill-switch):

```toml
[security.mcp]
enabled = true
allowed_servers = []  # empty = allow all configured servers
```

When `allowed_servers` is non-empty, only those named servers are loaded.

### How It Works

At startup, AgentZero connects to each configured MCP server, calls `tools/list`, and registers every discovered tool as its own `Box<dyn Tool>`:

- **Name**: `mcp__filesystem__read_file`, `mcp__github__create_issue`, etc.
- **Description**: From the MCP server's tool metadata
- **Schema**: The `inputSchema` from `tools/list`, passed directly to the LLM

### Session Sharing

Multiple tools from the same server share a single `McpServerConnection` (subprocess + stdin/stdout handles). The first tool call spawns the process; subsequent calls reuse it. If a connection error occurs, the session is cleared and retried once automatically.

### Graceful Degradation

If a server fails to connect at startup (e.g. missing binary, timeout), it is skipped with a warning — other servers and all built-in tools continue to work normally.

---

## Security Defaults Summary

| Component | Setting | Default |
|---|---|---|
| `read_file` max size | `max_read_bytes` | 64 KiB |
| `write_file` max size | `max_write_bytes` | 64 KiB |
| `write_file` enabled | `enabled` | `false` |
| `shell` max args | `max_args` | 8 |
| `shell` max arg length | `max_arg_length` | 128 bytes |
| `shell` max output | `max_output_bytes` | 8 KiB |
| `web_fetch` max size | `max_bytes` | 64 KiB |
| WASM fuel limit | `fuel_limit` | 1,000,000 |
| WASM memory limit | `memory_limit_mb` | 64 MB |
| Private IP blocking | `block_private_ip` | `true` |
| Loopback access | `allow_loopback` | `false` |
| Sensitive files | `allow_sensitive_file_reads` | `false` |
