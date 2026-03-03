# Research: Plugin-First Architecture for AgentZero

**Date**: 2026-03-03
**Status**: Approved — Sprint 20
**Question**: Can we implement more of the library/binary as plugins while keeping it lightweight, performant, easy to extend, production-ready, and integrated?

---

## TL;DR

**Yes.** The project is already 70% there with wasmtime v42, plugin packaging CLI (8 commands), TAR archives with SHA256 verification, and `WasmIsolationPolicy`. The remaining work is upgrading the WASM ABI to support tool I/O, adding WASI capabilities, building a plugin SDK with a `declare_tool!` macro, implementing module caching, hot-reload for development, an FFI plugin bridge, and a git-based registry. The approach is production-proven (Cloudflare Workers, Fastly Compute, Shopify Functions, Envoy proxy).

**Decision**: Proceed with an 8-phase implementation plan. Security is strengthened (not compromised), the <5MB minimal binary is preserved, and all landing page claims are maintained or made real.

---

## Current State

### Existing Plugin Mechanisms

**1. WASM Plugins** (`crates/agentzero-plugins/src/wasm.rs`)
- Runtime: wasmtime v42 (behind `wasm-runtime` feature flag)
- Isolation: `WasmIsolationPolicy` — timeout (30s), memory (256MB), network/fs toggles, host call allowlist
- Plugin packaging: TAR archive with manifest.json + plugin.wasm + SHA256 verification
- Full CLI lifecycle: `plugin new|validate|test|dev|package|install|list|remove`
- **Gap**: ABI is `fn() -> i32` — no way to pass tool input or receive structured output

**2. Process Plugins** (`ProcessPluginTool`)
- Spawns external process, JSON over stdin/stdout
- Already implements the `Tool` trait
- Configured via `AGENTZERO_PLUGIN_TOOL` env var
- ~5-50ms overhead per spawn

**3. MCP Servers** (`McpTool`)
- Model Context Protocol — connects to external tool servers
- Configured via `AGENTZERO_MCP_SERVERS` env var

### Tool Inventory (42 tools)

| Category     | Count | Tools                                                      | Plugin Candidate?                 |
| ------------ | ----- | ---------------------------------------------------------- | --------------------------------- |
| File I/O     | 4     | read_file, write_file, apply_patch, file_edit              | Core (read), Yes (write/edit)     |
| Shell        | 1     | shell                                                      | Core                              |
| Search       | 2     | glob_search, content_search                                | Core                              |
| Memory       | 3     | memory_store, memory_recall, memory_forget                 | Core                              |
| Git          | 1     | git_operations                                             | Yes                               |
| Web          | 5     | web_search, web_fetch, http_request, browser, browser_open | Yes                               |
| Media        | 4     | pdf_read, docx_read, screenshot, image_info                | Yes (except image_info — trivial) |
| Cron         | 7     | cron_add/list/remove/update/pause/resume, schedule         | Yes                               |
| SOPs         | 5     | sop_list/status/advance/approve/execute                    | Maybe (deep integration)          |
| Delegation   | 2     | delegate, delegate_coordination_status                     | No (core runtime)                 |
| SubAgent     | 3     | subagent_spawn/list/manage                                 | No (core runtime)                 |
| Hardware     | 3     | hardware_board_info/memory_map/memory_read                 | Yes                               |
| WASM         | 2     | wasm_module, wasm_tool_exec                                | Yes                               |
| Process      | 1     | process_tool                                               | Maybe                             |
| Task         | 1     | task_plan                                                  | Core                              |
| Config       | 2     | proxy_config, model_routing_config                         | Maybe                             |
| Integrations | 2     | composio, pushover                                         | Yes                               |
| Discovery    | 1     | cli_discovery                                              | Core                              |

---

## Decisions

### What to Extract as Plugins

**Extract as official plugin packs** (ship in registry from day one to seed the ecosystem):

| Plugin Pack | Tools | Why Extract |
|-------------|-------|-------------|
| `agentzero-plugin-hardware` | board_info, memory_map, memory_read (3) | Niche, self-contained, no framework deps |
| `agentzero-plugin-integrations` | composio, pushover (2) | Third-party service coupling |
| `agentzero-plugin-cron` | add/list/remove/update/pause/resume/schedule (7) | Not every user needs scheduling |

**Keep native** (core agent capability, security-critical, or deeply coupled):

| Category | Tools | Why Keep Native |
|----------|-------|----------------|
| File I/O | read_file, write_file, file_edit, apply_patch | Uses `agentzero_autonomy` security checks, called constantly |
| Shell | shell | Security-critical (ShellPolicy) |
| Search | glob_search, content_search | Called constantly, zero-overhead required |
| Memory | memory_store/recall/forget | Core state management |
| Planning | task_plan, cli_discovery | Runtime introspection |
| Delegation | delegate, subagent_*, delegate_coordination_status | Deep runtime coupling |
| Git | git_operations | Security-gated but core workflow |
| SOPs | sop_* (5) | Deep workflow integration |
| Web | web_search, browser, browser_open, http_request, web_fetch | Keep native for now; could be plugin later |
| Media | pdf_read, docx_read, screenshot, image_info | Keep native for now; could be plugin later |

**Principle**: Extract niche tools first. Keep high-frequency tools native. Move more tools to plugins as the system matures and if there's demand.

### WASI Support

WASI provides **capability-based security** — a standardized, audited interface for filesystem, environment, clock, and random access. Instead of custom host functions for every OS operation, plugins get WASI capabilities that are explicitly granted or denied per-plugin via the manifest + isolation policy. This is *more secure* than ad-hoc host functions because:
- WASI capabilities are well-defined and auditable
- wasmtime-wasi is maintained by the Bytecode Alliance
- Plugins can't access anything not explicitly granted
- The capability model maps directly to `WasmIsolationPolicy` fields (`allow_fs_write`, `allow_network`)

### Cross-Platform Plugins

WASM is inherently cross-platform. A `.wasm` file compiled on macOS runs identically on Linux x86, Linux ARM, Windows, and Raspberry Pi. Plugin authors compile once, users install anywhere. This maps to AgentZero's "8 Platform Targets" claim — plugins inherit the same reach.

### Plugin Discovery Paths

Three discovery paths, in priority order (later overrides earlier):
1. **Global**: `~/.local/share/agentzero/plugins/` — user-installed plugins
2. **Project**: `$PROJECT/.agentzero/plugins/` — project-specific plugins
3. **Development**: `$CWD/plugins/` — in-development plugins (hot-reload enabled)

### Host Callbacks / Permissions

Plugins declare capabilities in their manifest:
```json
{
  "capabilities": ["wasi:filesystem/read", "wasi:random", "host:log"],
  "allowed_host_calls": ["az_log", "az_read_file"]
}
```

The runtime checks these against the isolation policy before instantiation. Only declared + permitted capabilities are linked. Undeclared imports cause a load-time error.

**Host functions provided** (via wasmtime Linker):
- `az_log(level: i32, ptr: i32, len: i32)` — structured logging to host's tracing
- `az_read_file(path_ptr: i32, path_len: i32) -> i64` — read file (sandboxed to workspace_root)
- `az_http_get(url_ptr: i32, url_len: i32) -> i64` — HTTP GET (requires `allow_network`)
- `az_env_get(key_ptr: i32, key_len: i32) -> i64` — read environment variable
- WASI preview1 capabilities (via `wasmtime-wasi`) — `fd_read`, `fd_write`, `clock_time_get`, `random_get`, gated by policy

### Hot-Reload for Development

Only for `$CWD/plugins/` path. Uses `notify` crate to watch for `.wasm` file changes. When detected:
1. Unload the old `WasmTool` instance
2. Invalidate module cache
3. Load the new `.wasm` and re-instantiate
4. Log the reload event

Not enabled for global/project plugins — those require restart (stability over convenience).

### FFI-Based Plugin Creation

Two paths for non-Rust plugin authors:

**Path A: WASM (any language → .wasm)**
- Rust, C, C++, Go, AssemblyScript, Zig — compile to `wasm32-wasip1`
- Python — via `componentize-py`
- All use the `az_tool_execute` ABI

**Path B: FFI bridge (runtime registration)**
- Add `register_tool(name, callback)` to the FFI controller API
- Swift/Kotlin/Python/Node.js register a closure/callback as a tool
- The FFI layer wraps it in a struct implementing `Tool` trait
- No WASM compilation needed — tools run in the host process (not sandboxed)
- Simpler but less isolated than WASM

### Registry: Git-Based (Not Deferred)

A git repo (`github.com/agentzero-project/plugins`) with an index. `plugin search` clones/fetches the index, searches locally. `plugin install <id>` reads the index entry, downloads `.tar` from author's GitHub Release URL, verifies SHA256. Seeded with official plugin packs (hardware, integrations, cron) from day one.

---

## Complete Feature Flag Map

All features forwarded through `bin/agentzero/Cargo.toml`:

```toml
[features]
default = ["memory-sqlite", "plugins", "gateway", "tui", "interactive"]
minimal = ["memory-sqlite"]

# Memory backends
memory-sqlite = ["agentzero-cli/memory-sqlite"]
memory-turso = ["agentzero-cli/memory-turso"]

# Plugin system (WASM runtime + plugin CLI + discovery)
plugins = ["agentzero-cli/plugins"]

# HTTP gateway (axum)
gateway = ["agentzero-cli/gateway"]

# Terminal UI (ratatui + crossterm)
tui = ["agentzero-cli/tui"]

# Interactive prompts (inquire + console)
interactive = ["agentzero-cli/interactive"]

# RAG / retrieval-augmented generation
rag = ["agentzero-cli/rag"]

# Hardware tools (board info, memory map)
hardware = ["agentzero-cli/hardware"]

# Communication channels (Telegram, Discord, Slack, etc.)
channels-standard = ["agentzero-cli/channels-standard"]
```

New features to add:

```toml
# WASI support for plugins (filesystem, env, clock, random)
wasi = ["agentzero-cli/wasi"]

# Hot-reload for CWD plugins (development only)
plugin-dev = ["agentzero-cli/plugin-dev"]
```

---

## Feasibility Assessment

### Lightweight?

**Yes.** Three-tier approach:

| Tier          | Mechanism          | Binary Impact                          | Runtime Overhead     |
| ------------- | ------------------ | -------------------------------------- | -------------------- |
| Core          | Always compiled    | ~2-3MB base                            | Zero                 |
| Feature-gated | Cargo `--features` | Compiled out when not needed           | Zero                 |
| Dynamic WASM  | Runtime loaded     | +15-20MB for wasmtime runtime (opt-in) | ~1-5ms/call (cached) |

A minimal core build (`--no-default-features --features minimal`) produces a 5.2MB binary. Full build includes wasmtime at 18MB.

### Performant?

**Yes.** The key insight: agent loops are **LLM-bound, not tool-bound**.

| Operation                | Latency        | Context                                 |
| ------------------------ | -------------- | --------------------------------------- |
| LLM API call             | 1,000-10,000ms | Dominates the loop                      |
| WASM plugin (cached)     | 1-5ms          | Module deserialized from disk           |
| WASM plugin (first load) | 50-100ms       | One-time compilation, cached thereafter |
| Process plugin           | 5-50ms         | Per invocation                          |
| Native tool call         | <1ms           | Direct function call                    |

WASM overhead is <0.5% of total agent loop time. Wasmtime supports `Module::serialize()`/`Module::deserialize()` for AOT-compiled module caching — subsequent loads skip compilation entirely.

### Easy to Extend?

**Yes**, with the plugin SDK:

```rust
use agentzero_plugin_sdk::prelude::*;

declare_tool!("my_tool", execute);

fn execute(input: ToolInput) -> ToolOutput {
    ToolOutput::success(format!("Hello from plugin! Got: {}", input.input))
}
```

Compiles to WASM, packages with existing CLI, installs to plugin directory, auto-discovered on next agent start.

**Language support**: WASM target means plugins can be written in Rust, Go, C/C++, AssemblyScript, Zig, or any language that compiles to WASM. FFI bridge supports Swift, Kotlin, Python, and Node.js without WASM compilation.

### Production-Ready?

**Yes.** This pattern is proven at scale:

| System             | Plugin Mechanism              | Scale                               |
| ------------------ | ----------------------------- | ----------------------------------- |
| Cloudflare Workers | WASM (V8 isolates + wasmtime) | Millions of req/sec                 |
| Fastly Compute     | WASM (wasmtime)               | Edge compute at CDN scale           |
| Shopify Functions  | WASM (wasmtime-based)         | Runs custom logic on every checkout |
| Envoy Proxy        | WASM filters (Proxy-WASM)     | Service mesh data plane             |
| Figma              | WASM plugins                  | Millions of users                   |
| VS Code            | Process-based extensions      | Dominant IDE                        |
| Zed Editor         | WASM extensions               | Sandboxed editor plugins            |

Wasmtime is maintained by the Bytecode Alliance (Mozilla, Fastly, Intel, Microsoft). It's the most mature WASM runtime in the Rust ecosystem.

### Integrated?

**Yes — this is the strongest point.** The `Tool` trait is clean and simple:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult>;
}
```

A `WasmTool` bridge struct implements `Tool` by delegating to the WASM runtime. From the agent loop's perspective, a WASM plugin tool is indistinguishable from a native tool. The existing security policy (`ToolSecurityPolicy`) and tool registration in `default_tools()` work unchanged.

---

## WASM ABI v2

The single biggest piece of missing infrastructure is upgrading the WASM plugin contract to support tool I/O.

### Current ABI (v1)
```
// Plugin exports:
fn run() -> i32    // No input, no output, just a status code
```

### ABI v2 Contract
```
// Plugin exports:
az_alloc(size: i32) -> i32                         // Bump allocator
az_tool_name() -> i64                              // Packed ptr|len
az_tool_execute(input_ptr: i32, input_len: i32) -> i64  // JSON in → JSON out
az_tool_schema() -> i64                            // Optional: input schema

// Host provides (via Linker, gated by policy):
az_log(level: i32, msg_ptr: i32, msg_len: i32)
az_read_file(path_ptr: i32, path_len: i32) -> i64
az_http_get(url_ptr: i32, url_len: i32) -> i64
az_env_get(key_ptr: i32, key_len: i32) -> i64
```

Host writes input JSON to WASM linear memory via `az_alloc`, calls `az_tool_execute`, reads output JSON back via the packed `ptr|len` return value. The plugin SDK handles all serialization.

### JSON Protocol

```json
// Input:
{"input": "...", "workspace_root": "..."}

// Output:
{"output": "...", "error": null}
```

### Module Caching Strategy
```
$DATA_DIR/plugins/{id}/{version}/
├── manifest.json
├── plugin.wasm          # Source WASM module
└── .cache/
    └── plugin.cwasm     # AOT-compiled, platform-specific (~1-5ms load)
    └── source.sha256    # Hash of plugin.wasm for cache invalidation
```

`unsafe Module::deserialize_file()` is mitigated by SHA256 hash match + wasmtime version mismatch = automatic recompile.

---

## Plugin SDK + `declare_tool!` Macro

### Complete Plugin Example

```rust
// my-plugin/src/lib.rs
use agentzero_plugin_sdk::prelude::*;

declare_tool!("weather_lookup", execute);

fn execute(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let city = req["city"].as_str().unwrap_or("unknown");
    ToolOutput::success(format!("Weather for {city}: 72°F, sunny"))
}
```

```toml
# my-plugin/Cargo.toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
agentzero-plugin-sdk = "0.1.4"
serde_json = "1"
```

```json
// my-plugin/manifest.json
{
  "id": "weather-lookup",
  "version": "0.1.0",
  "entrypoint": "az_tool_execute",
  "wasm_file": "plugin.wasm",
  "wasm_sha256": "",
  "capabilities": ["host:az_http_get"],
  "allowed_host_calls": ["az_http_get"],
  "min_runtime_api": 2,
  "max_runtime_api": 2
}
```

### Build + Install
```bash
cargo build --target wasm32-wasip1 --release
agentzero plugin package --manifest manifest.json \
  --wasm target/wasm32-wasip1/release/my_plugin.wasm
agentzero plugin install --package my-plugin-0.1.0.tar
```

Or for development, copy the `.wasm` to `./plugins/` and hot-reload picks it up.

### SDK Crate Structure

New crate `agentzero-plugin-sdk` depends only on `serde` + `serde_json` (zero agentzero deps):

```rust
pub struct ToolInput { pub input: String, pub workspace_root: String }
pub struct ToolOutput { pub output: String, pub error: Option<String> }

impl ToolOutput {
    pub fn success(output: impl Into<String>) -> Self { ... }
    pub fn error(msg: impl Into<String>) -> Self { ... }
}

#[macro_export]
macro_rules! declare_tool { ... }
// Generates: az_alloc, az_tool_name, az_tool_execute exports
// Includes: 1MB bump allocator, JSON serialization, error wrapping
```

---

## FFI Plugin Bridge

### Architecture

```rust
// crates/agentzero-ffi/src/lib.rs
#[uniffi::export]
fn register_tool(&self, name: String, callback: Arc<dyn ToolCallback>) -> Result<(), AgentZeroError>

#[uniffi::export(callback_interface)]
pub trait ToolCallback: Send + Sync {
    fn execute(&self, input: String, workspace_root: String) -> Result<String, String>;
}
```

### Python Example

```python
from agentzero_ffi import AgentZeroController, ToolCallback

class WeatherTool(ToolCallback):
    def execute(self, input: str, workspace_root: str) -> str:
        import json
        data = json.loads(input)
        return json.dumps({"output": f"Weather for {data['city']}: sunny"})

controller = AgentZeroController(config)
controller.register_tool("weather_lookup", WeatherTool())
```

**Tradeoff**: FFI tools run in the host process (not sandboxed like WASM). They have full access to the process memory. This is by design — FFI users are embedding the runtime in their own application and trust their own code.

---

## Plugin Development Experience

### Create → Build → Test → Publish in 5 minutes

```bash
# 1. Scaffold a new plugin project
agentzero plugin new --id my-tool --scaffold rust

# 2. Write the tool logic using the SDK (src/lib.rs)
# declare_tool!("my_tool", execute);

# 3. Build to WASM
cargo build --target wasm32-wasip1 --release

# 4. Test locally — CWD plugins are auto-discovered
mkdir -p plugins/my-tool/0.1.0
cp target/wasm32-wasip1/release/my_tool.wasm plugins/my-tool/0.1.0/plugin.wasm
cp manifest.json plugins/my-tool/0.1.0/
agentzero agent "use my_tool to say hello"
# → agent finds the plugin in ./plugins/, loads it, calls it

# 5. Package and publish when ready
agentzero plugin package --manifest manifest.json \
    --wasm target/wasm32-wasip1/release/my_tool.wasm
agentzero plugin publish --registry github.com/agentzero-project/plugins
```

### Three Ways to Load Plugins During Development

| Method | When to use | How |
| --- | --- | --- |
| **CWD `./plugins/`** | Active development, rapid iteration | Drop WASM + manifest into `./plugins/{id}/{version}/` — agent auto-discovers on start, hot-reload on change |
| **`plugin dev` command** | Quick test/validation without full agent | `agentzero plugin dev --execute` runs preflight + execution in isolation |
| **`plugin install --file`** | Testing the install flow end-to-end | `agentzero plugin install --file my-tool-0.1.0.tar` installs to `$DATA_DIR/plugins/` |

---

## Plugin CLI Surface

### Current (8 commands)

```
agentzero plugin new       # Create manifest template
agentzero plugin validate  # Validate a manifest
agentzero plugin test      # Run preflight checks (+ optional execution)
agentzero plugin dev       # Iterative dev loop
agentzero plugin package   # Bundle into .tar archive
agentzero plugin install   # Install from local .tar archive
agentzero plugin list      # List installed plugins
agentzero plugin remove    # Remove installed plugin(s)
```

### New Commands (Phase 6)

```
agentzero plugin search <query>          # Search plugin registry
agentzero plugin info <id>               # Show plugin details
agentzero plugin install <id>[@version]  # Install from registry
agentzero plugin install --url <url>     # Install from URL
agentzero plugin update [<id>]           # Update one or all plugins
agentzero plugin enable <id>             # Enable a disabled plugin
agentzero plugin disable <id>            # Disable without removing
agentzero plugin outdated                # Show plugins with newer versions
agentzero plugin publish                 # Publish to registry (opens PR)
```

### Plugin State File

```json
// ~/.local/share/agentzero/plugins/state.json
{
  "plugins": {
    "hardware-tools": {
      "version": "1.2.0",
      "enabled": true,
      "installed_at": "2026-03-01T12:00:00Z",
      "source": "registry"
    }
  }
}
```

---

## Git-Based Registry

### Structure

A git repo (`github.com/agentzero-project/plugins`) with an index:

```
registry/
├── index/
│   ├── hardware-tools.json
│   ├── cron-suite.json
│   └── weather-lookup.json
├── categories.json
├── featured.json
└── README.md
```

### How It Works

- `plugin search` — clones/fetches index, searches locally (cached for 1 hour)
- `plugin install <id>` — reads index entry, downloads .tar from author's GitHub Release URL, verifies SHA256
- `plugin publish` — generates/updates index JSON, opens PR to registry repo
- Download URLs point to plugin authors' GitHub Releases — registry stores only metadata

### Trust Model (3 layers)

1. **Registry review** — PRs reviewed by maintainers (human curation)
2. **SHA256 verification** — CLI checks hash on every download
3. **WASM sandbox** — even malicious plugins can't escape isolation

### Cost

| Component | Cost | Notes |
| --- | --- | --- |
| Registry git repo | Free | GitHub public repo |
| Static website | Free | GitHub Pages / Cloudflare Pages |
| CI/CD for site builds | Free | GitHub Actions |
| Plugin binary hosting | Free | Authors host on their own GitHub Releases |
| Domain name | ~$12/yr | plugins.agentzero.dev |

**Total: ~$12/year.** Scales to hundreds of plugins with zero additional cost.

---

## Architecture Summary

```
┌─────────────────────────────────────────────────────┐
│                    Agent Loop                        │
│  ┌──────────────────────────────────────────────┐   │
│  │          Vec<Box<dyn Tool>>                   │   │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐     │   │
│  │  │ Native   │ │ WasmTool │ │ FfiTool  │     │   │
│  │  │ Tools    │ │ (bridge) │ │ (bridge) │     │   │
│  │  │ (core)   │ │          │ │          │     │   │
│  │  └──────────┘ └─────┬────┘ └─────┬────┘     │   │
│  └──────────────────────┼───────────┼───────────┘   │
│                         │           │                │
│              ┌──────────▼────┐  ┌───▼──────────┐    │
│              │ WasmPlugin    │  │ FFI Callback  │    │
│              │ Runtime       │  │ (host process)│    │
│              │ ┌───────────┐ │  └──────────────┘    │
│              │ │ WASI      │ │                       │
│              │ │ sandbox   │ │                       │
│              │ │ + host fn │ │                       │
│              │ └───────────┘ │                       │
│              └───────────────┘                       │
└─────────────────────────────────────────────────────┘
```

### Extension Mechanisms

| Mechanism | Who extends | Friction | Isolation | Speed |
| --- | --- | --- | --- | --- |
| **Native (Rust tools)** | Core team | High (Rust + recompile) | None (same process) | Native |
| **Feature flags** | Core team, at build time | Low (toggle features) | Compile-time | Native |
| **WASM plugins** | Anyone, at deploy time | Medium (SDK + package) | Strong (sandbox) | ~1-5ms overhead |
| **FFI plugins** | Embedding developers | Low (callback registration) | None (host process) | Native |
| **Process plugins** | Anyone, any language | Low (any executable) | Full (OS process) | ~5-50ms overhead |
| **MCP servers** | Anyone, any language | Low (server protocol) | Full (separate process) | Network overhead |

---

## Does This Compromise Our Goals?

### Security — Strengthened, not compromised

| Mechanism | Before (native only) | After (with plugins) |
|-----------|---------------------|---------------------|
| Tool isolation | Runtime boolean flags (`ToolSecurityPolicy`) | WASM sandbox: physical memory isolation, epoch-based CPU limits, capability-gated fs/network |
| Third-party code | Must fork repo, get PR merged | Runs in sandbox. Can't corrupt host memory, can't access undeclared resources |
| Host callbacks | N/A | Manifest declares capabilities, policy validates before linking. Undeclared imports = load-time error |
| Integrity | N/A | SHA256 verification on every download + install |
| FFI tools | N/A | Run in host process (not sandboxed) — but these are authored by the embedding developer, who already has full process access |

**The plugin system makes untrusted code SAFER to run, not less safe.**

### Safety — Preserved (double layer)

- WASM linear memory is bounds-checked at the VM level — a plugin cannot access memory outside its allocation
- The only `unsafe` code is `Module::deserialize_file()` — mitigated by SHA256 hash match + wasmtime version mismatch = automatic recompile
- `Box::leak()` for plugin names: ~20 bytes per plugin, agent processes are short-lived. Negligible.
- Rust's compile-time guarantees apply to all host code. WASM's runtime guarantees apply to all plugin code.

### <5MB Runtime — Preserved

The `minimal` feature build excludes `plugins` (which includes wasmtime). Measured in Sprint 19:

| Build | Size | Includes plugins? |
|-------|------|-------------------|
| Minimal (`release-min`) | **5.2 MB** | No |
| Default (`release`) | 18 MB | Yes (wasmtime) |

Adding `wasmtime-wasi` (~0.5-1MB additional in the default build) does not affect the minimal build at all. The `notify` crate for hot-reload is behind its own `plugin-dev` feature. **The 5.2MB minimal claim is untouched.**

### UX — Improved

| Before | After |
|--------|-------|
| Adding a tool = write Rust, add to `default_tools()`, recompile, ship release | `agentzero plugin install weather-tools` → restart → done |
| Writing a tool requires knowledge of 44-crate workspace | `declare_tool!("my_tool", execute);` — 10 lines of Rust |
| Dev iteration = recompile entire binary | Drop `.wasm` in `./plugins/` → hot-reload in seconds |
| Non-Rust developers can't contribute tools | WASM: any language. FFI: Swift/Kotlin/Python/Node.js callbacks |

### Landing Page Claims — All Preserved or Strengthened

| Claim | Status | Impact |
|-------|--------|--------|
| ~5MB Minimal Binary | **Preserved** | Minimal build excludes plugins entirely |
| ~19ms Cold Start | **Preserved** | Minimal build cold start unchanged; default build adds <10ms for plugin discovery with cached modules |
| 35+ LLM Providers | **Unchanged** | Providers are a separate system |
| 45+ Built-in Tools | **Grows** | Plugins add tools, they don't replace native ones (initially) |
| 8 Platform Targets | **Unchanged** | WASM plugins are cross-platform by nature |
| Single Binary | **Preserved** | Plugins are optional add-ons, not requirements |
| Rust Memory Safety | **Strengthened** | Rust + WASM = two layers of memory safety |
| Security First | **Strengthened** | WASM sandbox > runtime policy flags |
| WASM Plugin Sandbox | **Made real** | Currently aspirational claim becomes functional |
| FFI Bindings | **Extended** | FFI can now register tools, not just consume the runtime |
| Self-Update | **Unchanged** | |
| MCP Support | **Unchanged** | |

### Extensibility — Massively expanded

| Before | After |
|--------|-------|
| Only core team can add tools | Anyone can publish WASM plugins |
| Feature flags for modularity | Feature flags + installable plugins + FFI callbacks |
| 1 extension mechanism (MCP) | 4 mechanisms: native, WASM, process, FFI |

---

## Risks & Tradeoffs

| Risk                                | Mitigation                                                                                  |
| ----------------------------------- | ------------------------------------------------------------------------------------------- |
| WASM can't do everything native can | Host function bindings for common ops (fs, HTTP). WASI for standard interfaces. |
| wasmtime adds ~15-20MB to binary    | Behind `wasm-runtime` feature flag. Users who don't need plugins can opt out (minimal build). |
| Two code paths (native + plugin)    | Start with 3 extracted tools. Don't over-extract. Keep deeply-integrated tools native.    |
| Plugin developer experience         | SDK + `declare_tool!` macro + existing CLI + dev loop + hot-reload.                          |
| Security surface                    | WasmIsolationPolicy + WASI capability model + SHA256 verification + manifest-declared capabilities. |
| ABI versioning                      | `min_runtime_api`/`max_runtime_api` fields in PluginManifest for forward compat.    |
| Plugin system complexity            | Contained in one crate (`agentzero-plugins`). Rest of system unchanged. |

---

## Implementation Phases

| Phase | Days | Delivers |
|-------|------|----------|
| 1: ABI v2 + WASI | 3-4 | Plugins receive input/output, WASI capabilities |
| 2: WasmTool + caching | 2-3 | Transparent Tool trait integration, 1-5ms cached loads |
| 3: Discovery + hot-reload | 3-4 | Zero-config loading from 3 paths, dev hot-reload |
| 4: Plugin SDK + macro | 3-4 | 10-line plugin authoring, `declare_tool!` macro, scaffold command |
| 5: Official plugin packs | 3-4 | Hardware, integrations, cron extracted to plugins |
| 6: CLI + state | 2-3 | Enable/disable, remote install, search, publish |
| 7: FFI plugin bridge | 2-3 | Swift/Kotlin/Python/Node.js tool registration via callback |
| 8: Registry | 3-5 | Git-based community distribution, static website |
| **Total** | **22-30** | **Complete plugin ecosystem** |

See `specs/SPRINT.md` for detailed task breakdown.

---

## Key Files

| File                                          | Role                                         |
| --------------------------------------------- | -------------------------------------------- |
| `crates/agentzero-plugins/src/wasm.rs`        | WASM runtime, isolation, execution           |
| `crates/agentzero-plugins/src/package.rs`     | Plugin manifest, packaging, discovery        |
| `crates/agentzero-plugins/src/watcher.rs`     | Hot-reload file watcher (new)                |
| `crates/agentzero-infra/src/tools/mod.rs`     | Tool registration (`default_tools()`)        |
| `crates/agentzero-infra/src/tools/wasm_bridge.rs` | WasmTool → Tool trait bridge (new)       |
| `crates/agentzero-infra/src/tools/ffi_bridge.rs`  | FfiTool → Tool trait bridge (new)        |
| `crates/agentzero-infra/src/tools/plugin.rs`  | ProcessPluginTool                            |
| `crates/agentzero-core/src/types.rs`          | Tool trait, ToolContext, ToolResult          |
| `crates/agentzero-plugin-sdk/`                | Plugin SDK crate (new)                       |
| `crates/agentzero-cli/src/commands/plugin.rs` | Plugin CLI commands                          |
| `crates/agentzero-ffi/src/lib.rs`             | FFI controller + ToolCallback                |
| `crates/agentzero-tools/`                     | All native tool implementations              |
