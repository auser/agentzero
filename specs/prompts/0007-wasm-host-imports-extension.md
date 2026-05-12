# Extend WASM Host Imports for Plugin Ecosystem

You are working in the AgentZero repository on branch `feat/wasm-host-imports`.

## Goal

Extend the WASM sandbox host imports so that WASM plugins can do real filesystem work — list directories, create directories, check file existence, append to files, and get the current time. These are needed for the brain plugin (ADR 0015) and any future plugin that manages files.

The WIT spec has already been updated to `az:host@0.2.0` in `crates/agentzero-sandbox/wit/az-host.wit`. Your job is to wire the runtime.

## Problem: String Return

The current host imports (`az::read_file`, `az::write_file`, `az::log`) use a Phase 1 hack: they pass strings from guest to host via `(ptr, len)` pairs in linear memory, but **host-to-guest string returns don't work**. `read_file` returns `i32` (0/1) and discards the file content (see `wasm.rs:223`: `Ok(_content) => return 0`). The comment on line 213-215 says this will be fixed with the component model.

For the new imports to be useful, WASM guests need to receive string data back from the host. You need to solve this before wiring the new imports.

### Recommended approach: shared memory buffer protocol

Until WIT component model adoption (Phase 2), use a simple shared memory protocol:

1. Host writes result string into guest's linear memory at a known buffer offset
2. Host returns `(ptr, len)` pair so the guest knows where the data is
3. Guest pre-allocates a buffer and exports its location, or host calls a guest-exported `alloc(len) -> ptr` function

The `alloc` approach is cleaner — the guest exports `fn alloc(size: i32) -> i32` which allocates in guest memory and returns a pointer. The host calls `alloc`, writes the string, then returns `(ptr, len)`.

Look at how wasmtime examples handle this. The guest needs to export:
```
export alloc: func(size: i32) -> i32
```

Then `read_file` becomes:
```
az::read_file(path_ptr, path_len) -> i64
// Returns: high 32 bits = ptr to result in guest memory, low 32 bits = len
// Or returns -1 on error (error message written to a known error buffer)
```

## Current State

### `WasmHostCallbacks` trait (`crates/agentzero-sandbox/src/wasm.rs:16-23`)
```rust
pub trait WasmHostCallbacks: Send + Sync {
    fn read_file(&self, path: &str) -> Result<String, String>;
    fn write_file(&self, path: &str, content: &str) -> Result<bool, String>;
    fn log(&self, message: &str);
}
```

### `DenyAllHostCallbacks` (`wasm.rs:27-37`)
Stubs that return errors for all operations.

### Linker registrations (`wasm.rs:196-257`)
Three `linker.func_wrap("az", ...)` calls for `log`, `read_file`, `write_file`.

### `SessionHostCallbacks` (`crates/agentzero-session/src/wasm_host.rs:27-53`)
Implements the trait by delegating to `ToolExecutor`. Each method does policy-checked I/O.

### `ToolExecutor` methods already available (`crates/agentzero-session/src/tool_exec.rs`)
- `read_file(path)` — line 61
- `list_dir(path)` — line 93
- `write_file(path, content)` — line 259
- `edit_file(path, old, new)` — line 175
- `search_files(path, pattern)` — line 133
- `shell_command(command)` — line 303

Note: `list_dir` already exists in `ToolExecutor`. No `append_file`, `create_dir`, or `file_exists` yet — you'll need to add those.

### WIT spec (`crates/agentzero-sandbox/wit/az-host.wit`)
Already updated to `@0.2.0` with:
- `append-file`, `list-dir`, `create-dir`, `file-exists` in `filesystem` interface
- `now` in new `clock` interface

## Tasks

### 1. Solve host-to-guest string passing

Fix `read_file` so the guest actually receives file content. Then apply the same pattern to all string-returning host imports.

This is the hardest part. Everything else is mechanical once this works.

### 2. Add 5 methods to `WasmHostCallbacks` trait

```rust
pub trait WasmHostCallbacks: Send + Sync {
    fn read_file(&self, path: &str) -> Result<String, String>;
    fn write_file(&self, path: &str, content: &str) -> Result<bool, String>;
    fn append_file(&self, path: &str, content: &str) -> Result<bool, String>;
    fn list_dir(&self, path: &str) -> Result<Vec<String>, String>;
    fn create_dir(&self, path: &str) -> Result<bool, String>;
    fn file_exists(&self, path: &str) -> Result<bool, String>;
    fn log(&self, message: &str);
    fn now(&self) -> String;
}
```

Update `DenyAllHostCallbacks` to reject all new operations (except `now` which can return the real time — it's not security-sensitive).

### 3. Register new host functions in the Linker

Add `linker.func_wrap("az", ...)` calls for:
- `az::append_file(path_ptr, path_len, content_ptr, content_len) -> i32` — same pattern as `write_file`
- `az::list_dir(path_ptr, path_len) -> i64` — needs string return (JSON array of names)
- `az::create_dir(path_ptr, path_len) -> i32` — returns 0/1
- `az::file_exists(path_ptr, path_len) -> i32` — returns 0 (exists) / 1 (not exists) / -1 (error)
- `az::now() -> i64` — needs string return (ISO 8601 string)

### 4. Add `ToolExecutor` methods

In `crates/agentzero-session/src/tool_exec.rs`, add:
- `append_file(path, content)` — policy-checked, uses `FileWrite` capability
- `create_dir(path)` — policy-checked, uses `FileWrite` capability
- `file_exists(path)` — policy-checked, uses `FileRead` capability

These follow the same pattern as existing methods: validate path, check policy, do I/O, return `ToolResult`.

### 5. Implement `SessionHostCallbacks` for new methods

In `crates/agentzero-session/src/wasm_host.rs`, add implementations that delegate to `ToolExecutor` following the existing `read_file`/`write_file` pattern.

### 6. Add codegen template (optional but useful)

Add a `DirectoryLister` or `FileSystemPlugin` template to `crates/agentzero-sandbox/src/codegen.rs` that imports the new functions. This validates the Linker wiring end-to-end.

### 7. Tests

- Trait methods return correct results with allowed policy
- Trait methods denied by default policy
- `append_file` creates file if missing, appends if exists
- `create_dir` is idempotent
- `file_exists` returns true/false correctly
- `now` returns valid ISO 8601
- `list_dir` returns entry names without full paths
- String return mechanism works end-to-end (guest receives actual content from `read_file`)
- Path traversal blocked for all new operations
- `.agentzero/` blocked for all new operations

### 8. Update SPRINT.md

Check off the Phase 24 items as you complete them:
```
- [ ] Extended filesystem host imports: list-dir, create-dir, file-exists, append-file
- [ ] Clock host import: now (ISO 8601)
- [ ] WIT spec bumped to az:host@0.2.0
```

## Constraints

- Never use `.unwrap()` in production code
- Zero clippy warnings
- All new methods go through the policy engine (except `now`)
- Follow existing error handling patterns (`thiserror::Error`)
- Use `agentzero_tracing::{info, warn, debug}` for logging
- Include Cargo.lock in any commit

## Order of Operations

1. String return mechanism first (everything depends on it)
2. `WasmHostCallbacks` trait extension + `DenyAllHostCallbacks`
3. `ToolExecutor` new methods
4. `SessionHostCallbacks` new implementations
5. Linker registrations
6. Tests
7. SPRINT.md update
