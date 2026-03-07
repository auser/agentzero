---
title: Plugin Authoring Guide
description: Build, test, and publish WASM plugins for AgentZero using the plugin SDK.
---

AgentZero supports extending the agent with custom tools via WASM plugins. Plugins run in a sandboxed WebAssembly environment with strict resource limits, capability-based security, and SHA-256 integrity verification.

## Prerequisites

- Rust toolchain with `wasm32-wasip1` target: `rustup target add wasm32-wasip1`
- AgentZero CLI with the `plugins` feature enabled (included in the default build)

## Quick Start

### 1. Scaffold a New Plugin

```bash
agentzero plugin new --id my-tool --scaffold rust
cd my-tool/
```

This generates:

```
my-tool/
├── Cargo.toml          # [lib] crate-type = ["cdylib"]
├── manifest.json       # Plugin metadata + capabilities
├── src/lib.rs          # Tool implementation
└── .cargo/config.toml  # Build target = "wasm32-wasip1"
```

### 2. Write the Tool Logic

Use the `declare_tool!` macro from `agentzero-plugin-sdk`:

```rust
// src/lib.rs
use agentzero_plugin_sdk::prelude::*;

declare_tool!("my_tool", execute);

fn execute(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let name = req["name"].as_str().unwrap_or("world");
    ToolOutput::success(format!("Hello, {name}!"))
}
```

### 3. Build to WASM

```bash
cargo build --target wasm32-wasip1 --release
```

### 4. Test Locally

Drop the WASM binary into `./plugins/` for instant auto-discovery:

```bash
mkdir -p plugins/my-tool/0.1.0
cp target/wasm32-wasip1/release/my_tool.wasm plugins/my-tool/0.1.0/plugin.wasm
cp manifest.json plugins/my-tool/0.1.0/
agentzero agent "use my_tool to say hello to Ari"
```

Or use the built-in test command:

```bash
agentzero plugin test --manifest manifest.json \
    --wasm target/wasm32-wasip1/release/my_tool.wasm --execute
```

### 5. Package and Install

```bash
agentzero plugin package --manifest manifest.json \
    --wasm target/wasm32-wasip1/release/my_tool.wasm \
    --out my-tool-0.1.0.tar
agentzero plugin install --package my-tool-0.1.0.tar
```

You can also install directly from an `https://` or `http://` URL without downloading manually:

```bash
agentzero plugin install --url https://example.com/releases/my-tool-0.1.0.tar
```

The CLI fetches the archive, verifies its SHA-256 hash against the manifest, and installs it in a single step. This works with any URL that serves a valid plugin tarball — GitHub Releases, self-hosted artifact servers, or a private registry.

### 6. Publish

```bash
agentzero plugin publish --registry github.com/agentzero-project/plugins
```

---

## Plugin Dependencies

A plugin can declare that it depends on other plugins. AgentZero resolves and installs those dependencies automatically when you install the plugin.

### Declaring Dependencies in the Manifest

Add an optional `dependencies` array to `manifest.json`:

```json
{
  "id": "my-advanced-tool",
  "version": "0.2.0",
  "entrypoint": "az_tool_execute",
  "wasm_file": "plugin.wasm",
  "wasm_sha256": "",
  "capabilities": ["host:az_log"],
  "allowed_host_calls": ["az_log"],
  "min_runtime_api": 2,
  "max_runtime_api": 2,
  "dependencies": [
    { "id": "base-utils", "version_req": ">=0.1.0, <1.0.0" },
    { "id": "http-helpers", "version_req": "^0.3.0" }
  ]
}
```

Each entry in `dependencies` has:

| Field | Description |
|---|---|
| `id` | Plugin ID of the dependency (must exist in the registry) |
| `version_req` | Semver requirement string (e.g., `"^1.0"`, `">=0.2.0, <1.0.0"`) |

The `dependencies` field is optional and defaults to an empty list. Existing manifests without it continue to work without changes.

### Installing with Dependency Resolution

When you install a plugin that has dependencies, pass `--registry-url` so the CLI knows where to look up dependency download URLs:

```bash
agentzero plugin install --url https://example.com/releases/my-advanced-tool-0.2.0.tar \
    --registry-url https://example.com/registry/index.json
```

Or when installing by plugin ID from the default registry:

```bash
agentzero plugin install my-advanced-tool --registry-url https://example.com/registry/index.json
```

The resolver:

1. Fetches and parses the manifest of the plugin being installed.
2. Looks up each dependency in the registry index.
3. Picks the highest version that satisfies the `version_req`.
4. Recursively resolves transitive dependencies.
5. Installs all missing dependencies before the top-level plugin.

Circular dependencies are detected automatically — the CLI reports an error and aborts instead of looping.

### Refreshing the Registry Index

To pull the latest plugin metadata from a remote registry:

```bash
agentzero plugin refresh --registry-url https://example.com/registry/index.json
```

Both `https://` and `http://` registry URLs are supported. The index is cached locally for subsequent `install` and `search` commands.

---

## Plugin SDK

### Cargo.toml

```toml
[package]
name = "my-tool"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
agentzero-plugin-sdk = "0.3.0"
serde_json = "1"
```

### Types

```rust
/// Input passed from the agent to your plugin.
pub struct ToolInput {
    pub input: String,           // JSON string from the LLM
    pub workspace_root: String,  // Absolute path to workspace root
}

/// Output returned from your plugin to the agent.
pub struct ToolOutput {
    pub output: String,
    pub error: Option<String>,
}

impl ToolOutput {
    /// Create a successful result.
    pub fn success(output: impl Into<String>) -> Self;

    /// Create an error result.
    pub fn error(msg: impl Into<String>) -> Self;

    /// Create a successful result with a non-fatal warning.
    /// Use for idempotent operations where the outcome is correct
    /// but the caller should know about an edge condition.
    pub fn with_warning(output: impl Into<String>, warning: impl Into<String>) -> Self;
}
```

### The `declare_tool!` Macro

```rust
declare_tool!("tool_name", handler_function);
```

This macro generates all required WASM ABI v2 exports:
- `az_alloc` — bump allocator for linear memory
- `az_tool_name` — returns the tool name
- `az_tool_execute` — entry point: receives JSON input, returns JSON output

You only write the handler function.

### Host Calls

The runtime provides host functions in the `"az"` WASM import module. Declare them as `extern "C"` imports in your plugin:

```rust
#[link(wasm_import_module = "az")]
extern "C" {
    fn az_log(level: i32, msg_ptr: *const u8, msg_len: i32);
}

fn log(level: i32, msg: &str) {
    unsafe { az_log(level, msg.as_ptr(), msg.len() as i32) }
}

const LOG_ERROR: i32 = 0;
const LOG_WARN:  i32 = 1;
const LOG_INFO:  i32 = 2;
const LOG_DEBUG: i32 = 3;
```

`az_log` is always available. Other host calls like `az_env_get` require `"az::az_env_get"` in the manifest's `allowed_host_calls`.

### Typed Input (Recommended)

Instead of manually indexing into `serde_json::Value`, define a typed request struct:

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct Request {
    action: String,
    #[serde(default)]
    note_id: Option<String>,
}

fn execute(input: ToolInput) -> ToolOutput {
    let req: Request = match serde_json::from_str(&input.input) {
        Ok(r) => r,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };
    // ...
}
```

This gives compile-time field checking and self-documenting code.

---

## Reference Plugin

The **notepad** plugin at `plugins/agentzero-plugin-reference/notepad/` is the canonical example for plugin authors. It demonstrates every SDK pattern in ~180 lines:

| Pattern | How |
|---------|-----|
| Typed `#[derive(Deserialize)]` input | Flat request struct with optional fields |
| `az_log` host call | `extern "C"` import + safe wrapper |
| `ToolOutput::with_warning` | Idempotent delete of non-existent note |
| WASI filesystem | Flat `.md` files in `.agentzero/notepad/` |
| Path security | Validates note IDs against `/`, `\`, `..` |
| Action dispatch | `write`, `read`, `list`, `delete` |

Build and test:

```bash
cd plugins/agentzero-plugin-reference/notepad
cargo build --target wasm32-wasip1 --release
# Integration tests (from repo root):
cargo test -p agentzero-plugins --test reference_plugin_integration --features wasm-runtime
```

---

## Manifest

Every plugin requires a `manifest.json`:

```json
{
  "id": "my-tool",
  "version": "0.1.0",
  "entrypoint": "az_tool_execute",
  "wasm_file": "plugin.wasm",
  "wasm_sha256": "",
  "capabilities": ["host:az_log"],
  "allowed_host_calls": ["az_log"],
  "min_runtime_api": 2,
  "max_runtime_api": 2
}
```

| Field | Description |
|---|---|
| `id` | Unique plugin identifier (lowercase, hyphens) |
| `version` | Semantic version |
| `entrypoint` | WASM export to call (`az_tool_execute` for ABI v2) |
| `wasm_file` | Filename of the compiled WASM module |
| `wasm_sha256` | SHA-256 hash of the WASM file (set automatically by `plugin package`) |
| `capabilities` | WASI capabilities and host functions this plugin needs |
| `allowed_host_calls` | Specific host functions the plugin may invoke |
| `min_runtime_api` / `max_runtime_api` | Compatible runtime API version range |

### Capabilities

Capabilities declare what your plugin needs access to. The runtime only grants capabilities that are both declared in the manifest and permitted by the isolation policy.

| Capability | Description |
|---|---|
| `wasi:filesystem/read` | Read files (sandboxed to workspace) |
| `wasi:filesystem/read-write` | Read and write files |
| `wasi:random` | Access to random number generation |
| `wasi:clock` | Access to wall clock and monotonic time |
| `host:az_log` | Structured logging to the host |
| `host:az_read_file` | Read file via host function |
| `host:az_http_get` | HTTP GET via host (requires `allow_network`) |
| `host:az_env_get` | Read environment variable via host |

---

## Plugin Discovery

Plugins are discovered from three locations, checked in priority order (later overrides earlier):

| Path | Scope | Hot-Reload |
|---|---|---|
| `~/.local/share/agentzero/plugins/` | Global (user-wide) | No |
| `$PROJECT/.agentzero/plugins/` | Project-specific | No |
| `./plugins/` | Current working directory (development) | Yes |

On startup, the agent scans all three directories, loads valid manifests, and registers plugins alongside native tools. A plugin in `./plugins/` takes highest priority — useful for testing a development version over an installed one.

### Directory Structure

**Versioned layout** (installed plugins):

```
plugins/my-tool/0.1.0/
├── manifest.json
├── plugin.wasm
└── .cache/
    ├── plugin.cwasm     # AOT-compiled (auto-generated)
    └── source.sha256    # Cache invalidation hash
```

**Flat layout** (development convenience):

```
plugins/my-tool/
├── manifest.json
└── plugin.wasm
```

Both layouts are auto-detected. The flat layout is useful during development — no version subdirectory needed. When multiple versions exist in a versioned layout, the latest version (lexicographic) is loaded.

---

## Configuration

Enable WASM plugins and optionally override the default directories in `agentzero.toml`:

```toml
[security.plugin]
enabled = false             # Legacy process plugin (unchanged)
wasm_enabled = true         # Enable WASM plugin discovery

# Optional directory overrides (defaults shown):
# global_plugin_dir = "~/.local/share/agentzero/plugins"
# project_plugin_dir = ".agentzero/plugins"
# dev_plugin_dir = "plugins"
```

---

## Hot-Reload (Development)

When the `plugin-dev` feature is enabled, the agent watches `./plugins/` for `.wasm` file changes using the `notify` crate. When a change is detected:

1. The old plugin instance is unloaded
2. The module cache is invalidated
3. The new `.wasm` is loaded and re-instantiated
4. A reload event is logged

**Development workflow:**

```bash
# Terminal 1: watch + rebuild
cargo watch -x 'build --target wasm32-wasip1 --release' \
    -s 'cp target/wasm32-wasip1/release/my_tool.wasm plugins/my-tool/0.1.0/plugin.wasm'

# Terminal 2: agent picks up changes automatically
agentzero agent --interactive
```

Hot-reload is only enabled for `./plugins/` (CWD). Global and project plugins require a restart for stability.

---

## Security

### WASM Sandbox

Every plugin runs inside a WebAssembly sandbox with:

- **Memory isolation** — plugins cannot access host memory outside their linear memory allocation
- **CPU limits** — epoch-based timeout prevents infinite loops (default: 30s)
- **Memory limits** — configurable max memory (default: 256MB)
- **Capability gating** — filesystem, network, and host function access must be declared and permitted
- **SHA-256 verification** — integrity checked on install and every load

### Isolation Policy

```toml
[runtime.wasm]
fuel_limit = 1000000
memory_limit_mb = 64
max_module_size_mb = 50
allow_workspace_read = false
allow_workspace_write = false
allowed_hosts = []
```

### Trust Model

| Layer | Protection |
|---|---|
| Registry review | Human-curated PRs to the registry repo |
| SHA-256 verification | CLI checks hash on every download and install |
| WASM sandbox | Physical memory isolation, CPU limits, capability-gated I/O |

---

## CLI Commands

```bash
# Development
agentzero plugin new --id <id> --scaffold rust   # Scaffold a new plugin project
agentzero plugin validate --manifest manifest.json  # Validate manifest
agentzero plugin test --manifest manifest.json --wasm plugin.wasm --execute  # Test
agentzero plugin dev --manifest manifest.json --wasm plugin.wasm  # Dev loop
agentzero plugin package --manifest manifest.json --wasm plugin.wasm  # Package

# Installation
agentzero plugin install --package my-tool.tar    # Install from local file
agentzero plugin install my-tool                  # Install from registry
agentzero plugin install --url <url>              # Install from URL
agentzero plugin update [<id>]                    # Update plugins
agentzero plugin remove --id my-tool              # Remove plugin

# Inventory
agentzero plugin list                             # List installed plugins
agentzero plugin info <id>                        # Plugin details
agentzero plugin search <query>                   # Search registry
agentzero plugin outdated                         # Check for updates

# State
agentzero plugin enable <id>                      # Enable a disabled plugin
agentzero plugin disable <id>                     # Disable without removing

# Publishing
agentzero plugin publish                          # Submit to registry
```

---

## Non-Rust Plugins

Any language that compiles to `wasm32-wasip1` can be used to write plugins:

| Language | Compiler | Notes |
|---|---|---|
| Rust | `cargo build --target wasm32-wasip1` | First-class support via SDK |
| C/C++ | `wasi-sdk` / `clang --target=wasm32-wasip1` | Manual ABI implementation |
| Go | `GOOS=wasip1 GOARCH=wasm go build` | Larger binary size |
| Zig | `zig build -Dtarget=wasm32-wasi` | Good WASM support |
| AssemblyScript | `asc --target wasm32-wasi` | TypeScript-like syntax |

For languages that cannot compile to WASM, see the [FFI Bindings](/agentzero/guides/ffi-bindings/) guide for registering tools directly from Swift, Kotlin, Python, or Node.js via the callback interface.
