---
title: Plugin API Reference
description: WASM ABI v2 specification, host callbacks, capabilities, and manifest schema for AgentZero plugins.
---

This document specifies the WASM plugin ABI, host callback functions, capability model, and manifest schema.

## ABI v2 Specification

### Plugin Exports

Every ABI v2 plugin must export these functions:

| Export | Signature | Description |
|---|---|---|
| `az_alloc` | `(size: i32) -> i32` | Bump allocator — returns pointer to `size` bytes in linear memory |
| `az_tool_name` | `() -> i64` | Returns packed `ptr \| len` of the tool name string |
| `az_tool_execute` | `(input_ptr: i32, input_len: i32) -> i64` | Main entry point — receives JSON input, returns packed `ptr \| len` of JSON output |
| `az_tool_schema` | `() -> i64` | *(Optional)* Returns packed `ptr \| len` of a JSON schema describing accepted input |

### Packed Return Values

Functions returning `i64` use a packed pointer/length encoding:

```
i64 = (ptr as i64) | ((len as i64) << 32)
```

The host unpacks this to read `len` bytes starting at `ptr` from the plugin's linear memory.

### Input JSON Protocol

The host writes this JSON to the plugin's linear memory via `az_alloc`:

```json
{
  "input": "<string from LLM tool call>",
  "workspace_root": "/absolute/path/to/workspace"
}
```

### Output JSON Protocol

The plugin returns this JSON via `az_tool_execute`:

```json
{
  "output": "<result string returned to the agent>",
  "error": null
}
```

On error:

```json
{
  "output": "",
  "error": "description of what went wrong"
}
```

### Memory Management

Plugins use a bump allocator exported as `az_alloc`. The SDK provides a built-in 1MB bump allocator. For custom allocators, ensure `az_alloc(size)` returns a valid pointer to `size` contiguous bytes in linear memory.

The host calls `az_alloc` to allocate space for the input JSON before calling `az_tool_execute`. The plugin calls `az_alloc` internally for the output JSON.

---

## Host Callbacks

Host functions are provided via the wasmtime `Linker` and gated by the plugin's declared capabilities and the isolation policy. A plugin can only call host functions that are both declared in its manifest and permitted by the runtime policy.

### `az_log`

```
az_log(level: i32, msg_ptr: i32, msg_len: i32)
```

Write a structured log message to the host's tracing infrastructure.

| Parameter | Description |
|---|---|
| `level` | Log level: 0 = error, 1 = warn, 2 = info, 3 = debug, 4 = trace |
| `msg_ptr` | Pointer to UTF-8 message in plugin linear memory |
| `msg_len` | Length of the message in bytes |

**Capability required:** `host:az_log`

### `az_read_file`

```
az_read_file(path_ptr: i32, path_len: i32) -> i64
```

Read a file from the workspace. Returns packed `ptr|len` of the file contents written to plugin memory via `az_alloc`.

| Parameter | Description |
|---|---|
| `path_ptr` | Pointer to UTF-8 file path in plugin memory |
| `path_len` | Length of the path in bytes |
| Returns | Packed `ptr \| len` of file contents, or `0` on error |

**Capability required:** `wasi:filesystem/read` or `host:az_read_file`

**Security:** Paths are sandboxed to `workspace_root`. Path traversal (`..`) is rejected. Symlinks are resolved and validated.

### `az_http_get`

```
az_http_get(url_ptr: i32, url_len: i32) -> i64
```

Perform an HTTP GET request. Returns packed `ptr|len` of the response body.

| Parameter | Description |
|---|---|
| `url_ptr` | Pointer to UTF-8 URL in plugin memory |
| `url_len` | Length of the URL in bytes |
| Returns | Packed `ptr \| len` of response body, or `0` on error |

**Capability required:** `host:az_http_get`

**Policy required:** `allow_network = true` in the WASM isolation policy. The URL is also checked against the URL access policy (private IP blocking, domain allowlist/blocklist).

### `az_env_get`

```
az_env_get(key_ptr: i32, key_len: i32) -> i64
```

Read an environment variable. Returns packed `ptr|len` of the value.

| Parameter | Description |
|---|---|
| `key_ptr` | Pointer to UTF-8 variable name in plugin memory |
| `key_len` | Length of the name in bytes |
| Returns | Packed `ptr \| len` of value, or `0` if not set |

**Capability required:** `host:az_env_get`

---

## WASI Capabilities

WASI preview1 capabilities are provided via `wasmtime-wasi` and gated by the isolation policy. These are standard WASI interfaces, not AgentZero-specific.

| Capability | WASI Functions | Policy Gate |
|---|---|---|
| `wasi:filesystem/read` | `fd_read`, `fd_seek`, `path_open` (read-only) | `allow_workspace_read` |
| `wasi:filesystem/read-write` | All filesystem functions | `allow_workspace_write` |
| `wasi:random` | `random_get` | Always available |
| `wasi:clock` | `clock_time_get`, `clock_res_get` | Always available |

WASI filesystem access is sandboxed to the workspace root directory. Plugins cannot access files outside the workspace regardless of capabilities.

---

## Manifest Schema

### Full Example

```json
{
  "id": "weather-lookup",
  "version": "0.1.0",
  "entrypoint": "az_tool_execute",
  "wasm_file": "plugin.wasm",
  "wasm_sha256": "a1b2c3d4e5f6...",
  "capabilities": [
    "wasi:filesystem/read",
    "wasi:random",
    "host:az_log",
    "host:az_http_get"
  ],
  "allowed_host_calls": [
    "az_log",
    "az_http_get"
  ],
  "min_runtime_api": 2,
  "max_runtime_api": 2
}
```

### Field Reference

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | Yes | Unique plugin identifier. Lowercase alphanumeric + hyphens. |
| `version` | string | Yes | Semantic version (e.g., `"1.2.3"`). |
| `entrypoint` | string | Yes | Name of the WASM export to call. Use `"az_tool_execute"` for ABI v2. |
| `wasm_file` | string | Yes | Filename of the compiled WASM module within the package. |
| `wasm_sha256` | string | Yes | SHA-256 hash of the WASM file. Set automatically by `plugin package`. |
| `capabilities` | string[] | Yes | List of WASI capabilities and host functions the plugin requires. |
| `allowed_host_calls` | string[] | Yes | Subset of host functions the plugin may invoke. |
| `min_runtime_api` | integer | Yes | Minimum compatible runtime API version. |
| `max_runtime_api` | integer | Yes | Maximum compatible runtime API version. |

### Capability Validation

At load time, the runtime:

1. Reads the manifest's `capabilities` list
2. Checks each capability against the `WasmIsolationPolicy`
3. Links only the permitted capabilities into the WASM instance
4. Rejects the plugin with an error if any **required** capability is denied by policy

Undeclared imports (capabilities the plugin tries to use but didn't declare) cause a **load-time error**, not a runtime error. This is fail-closed by design.

---

## Module Caching

Compiled WASM modules are cached as AOT-compiled native code for fast subsequent loads.

### Cache Layout

```
{plugin_dir}/.cache/
├── plugin.cwasm     # AOT-compiled, platform-specific
└── source.sha256    # SHA-256 of the source plugin.wasm
```

### Cache Behavior

| Scenario | Action |
|---|---|
| Cache hit (SHA-256 matches) | Deserialize cached `.cwasm` (~1-5ms) |
| Cache miss (no cache file) | Compile from `.wasm`, write cache (~50-100ms) |
| Cache stale (SHA-256 mismatch) | Recompile and update cache |
| Cache corrupt | Log warning, recompile from source |
| wasmtime version mismatch | Recompile (detected by deserialization failure) |

Cache misses are non-fatal — the plugin always falls back to fresh compilation.

---

## Isolation Policy

The WASM isolation policy controls the sandbox boundaries for all plugins:

```toml
[runtime.wasm]
fuel_limit = 1000000         # Execution budget (CPU limit)
memory_limit_mb = 64         # Maximum linear memory
max_module_size_mb = 50      # Maximum .wasm file size
allow_workspace_read = false  # WASI filesystem read access
allow_workspace_write = false # WASI filesystem write access
allowed_hosts = []           # Network access domain allowlist

[runtime.wasm.security]
require_workspace_relative_tools_dir = true
reject_symlink_modules = true
reject_symlink_tools_dir = true
capability_escalation_mode = "deny"
module_hash_policy = "warn"  # "warn" or "enforce"
```

---

## Runtime API Versioning

| API Version | ABI | Features |
|---|---|---|
| 1 | `fn run() -> i32` | No input/output. Status code only. |
| 2 | `az_tool_execute(ptr, len) -> i64` | JSON input/output, WASI capabilities, host callbacks, module caching |

Plugins declare their compatible version range via `min_runtime_api` / `max_runtime_api`. The runtime rejects plugins outside its supported range with a clear error message.

v1 plugins running on a v2 runtime receive an "upgrade to SDK v2" error.

---

## Plugin State

The plugin state file tracks installed plugins and their enabled/disabled status:

```json
// ~/.local/share/agentzero/plugins/state.json
{
  "plugins": {
    "weather-lookup": {
      "version": "0.1.0",
      "enabled": true,
      "installed_at": "2026-03-01T12:00:00Z",
      "source": "registry"
    }
  }
}
```

Discovery reads this file and skips disabled plugins. A missing state file means all installed plugins are enabled (backward compatible).

---

## Registry Index Format

Each plugin in the registry has an index entry:

```json
{
  "id": "hardware-tools",
  "name": "Hardware Tools",
  "description": "Board info, memory maps, and hardware introspection",
  "author": "agentzero-project",
  "repository": "https://github.com/agentzero-project/plugin-hardware-tools",
  "license": "MIT",
  "categories": ["hardware", "system"],
  "keywords": ["board", "memory", "hardware"],
  "versions": [
    {
      "version": "1.2.0",
      "min_runtime_api": 2,
      "max_runtime_api": 2,
      "sha256": "abc123...",
      "download_url": "https://github.com/.../releases/download/v1.2.0/hardware-tools-1.2.0.tar",
      "size_bytes": 245760,
      "published_at": "2026-03-01T12:00:00Z"
    }
  ]
}
```

Download URLs point to the plugin author's GitHub Releases. The registry stores only metadata.
