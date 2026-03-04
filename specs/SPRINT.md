# AgentZero Sprint Plan

## Sprint 20: Plugin Architecture

**Goal:** Build a complete plugin ecosystem — WASM ABI v2 with WASI, plugin SDK with `declare_tool!` macro, module caching, hot-reload, FFI plugin bridge, official plugin packs, and a git-based registry. Security is strengthened, <5MB minimal binary is preserved, all landing page claims maintained.

**Research:** [specs/research/002-plugin-first-architecture.md](research/002-plugin-first-architecture.md)

### Phase 1: WASM ABI v2 + WASI Foundation (Days 1-4) ✅

- [x] Add `wasmtime-wasi` dependency behind `wasm-runtime` feature in `agentzero-plugins`
- [x] Define ABI v2 types: `WasmExecutionResultV2`, `WasmToolInput`, `WasmToolOutput`, `WasmV2Options`
- [x] Implement `execute_v2_with_policy()` in `wasm.rs`:
  - [x] Create `WasiP1Ctx` with capabilities based on `WasmIsolationPolicy`
  - [x] Register host functions via `linker.func_wrap()` (`az_log` always, `az_env_get` gated)
  - [x] Add WASI preview1 via `wasmtime_wasi::p1::add_to_linker_sync()` (gated by policy)
  - [x] Write input JSON to WASM linear memory via `az_alloc` export
  - [x] Call `az_tool_execute(ptr, len) -> i64` (packed ptr|len return)
  - [x] Read output JSON from WASM memory
- [x] Bump `CURRENT_RUNTIME_API_VERSION` to 2 in `package.rs`
- [x] `capabilities: Vec<String>` already in manifest schema (via `WasmV2Options`)
- [x] Keep existing v1 `execute()`/`execute_with_policy()` for backward compat
- [x] Add `allow_fs_read` field to `WasmIsolationPolicy`
- [x] Add `preflight_v2()` for v2 modules (skips v1 import validation)
- [x] `validate_v2_imports()` allows WASI + az namespace, rejects undeclared
- [x] `TimerGuard` drop pattern for clean epoch timer shutdown
- [x] Tests (7 new, 23 total): v2 round-trip, missing az_alloc, az_log host function, undeclared host function rejection, v2 timeout, pack/unpack ptr_len

### Phase 2: WasmTool Bridge + Module Caching (Days 5-7) ✅

- [x] Create `crates/agentzero-infra/src/tools/wasm_bridge.rs` with `WasmTool` struct
- [x] Implement `Tool` trait for `WasmTool` (delegates to `execute_v2` via `tokio::spawn_blocking`)
- [x] Add `ModuleCache` to `wasm.rs`: `load_or_compile(engine, wasm_path, sha256) -> Module`
  - [x] AOT `.cwasm` + `source.sha256` sidecar for cache invalidation
  - [x] `unsafe Module::deserialize_file()` — mitigated by SHA256 match + wasmtime version mismatch = recompile
  - [x] Cache miss is non-fatal (logs warning, falls back to fresh compilation)
- [x] Cache AOT modules at `{plugin_dir}/.cache/plugin.cwasm` + `source.sha256`
- [x] Add optional `agentzero-plugins` dep behind `wasm-plugins` feature in `agentzero-infra`
- [x] Add `enable_wasm_plugins: bool` to `ToolSecurityPolicy` (default false)
- [x] Feature-gated placeholder in `default_tools()` for Phase 3 discovery wiring
- [x] Tests (3 new, 26 total): cache creation/invalidation, corrupt cache fallback, SHA mismatch recompile

### Phase 3: Plugin Discovery + Hot-Reload (Days 8-11) ✅

- [x] Add `discover_plugins()` in `package.rs` scanning global → project → CWD paths
  - [x] Three-tier priority: global → project → CWD (later overrides earlier)
  - [x] Supports versioned layout (`<id>/<version>/manifest.json`) and flat layout (`<id>/manifest.json`)
  - [x] Picks latest version when multiple versions exist
  - [x] CWD plugins marked `dev_mode: true`
- [x] Wire discovery into `default_tools()` behind `wasm-plugins` feature
  - [x] `discover_plugins()` called with policy-provided directory overrides
  - [x] Each discovered plugin loaded as `WasmTool` with default isolation policy
  - [x] Invalid plugins warned via tracing and skipped
- [x] `enable_wasm_plugins: bool` added to `ToolSecurityPolicy` (done in Phase 2)
- [x] Expand `PluginConfig` in `agentzero-config`:
  - [x] `wasm_enabled: bool` → maps to `enable_wasm_plugins`
  - [x] `global_plugin_dir`, `project_plugin_dir`, `dev_plugin_dir` overrides
  - [x] Plugin dir paths carried through `ToolSecurityPolicy` to `default_tools()`
- [x] Create `crates/agentzero-plugins/src/watcher.rs` using `notify` crate (behind `plugin-dev` feature)
  - [x] `PluginWatcher::start(dir)` watches for `.wasm` create/modify events
  - [x] Channel-based API: `try_recv()`, `recv_timeout()`, `drain()`
  - [x] Ignores non-`.wasm` files
- [x] `plugin-dev` feature forwarded through CLI → bin crate
- [x] Tests (12 new, 38 total in plugins): empty dirs, versioned/flat discovery, tier override, latest version, invalid manifest skip, missing wasm skip, watcher creation/modification/ignore/missing-dir

### Tool Trait Schema Enhancement ✅

- [x] Add `description() -> &'static str` default method to `Tool` trait in `agentzero-core`
- [x] Add `input_schema() -> Option<serde_json::Value>` default method (JSON Schema format)
- [x] Both methods have backward-compatible defaults (`""` and `None`)
- [x] Add `description()` to all 50+ tool implementations across `agentzero-tools` and `agentzero-infra`
- [x] Add `input_schema()` with full JSON Schema to 20 highest-value tools:
  - [x] File I/O: read_file, write_file, file_edit, apply_patch
  - [x] Shell: shell
  - [x] Search: glob_search, content_search
  - [x] Git: git_operations
  - [x] Memory: memory_store, memory_recall, memory_forget
  - [x] Web: web_search, web_fetch, http_request, browser, browser_open
  - [x] Media: pdf_read, docx_read, screenshot, image_info
- [x] Add `description()` only to remaining tools (cron, SOP, hardware, WASM, delegate, subagent, etc.)
- [x] `cargo check` passes for agentzero-core, agentzero-tools, agentzero-infra
- [x] 222 tests pass across modified crates

**Purpose:** Enables structured tool-use APIs (Anthropic tool_use, OpenAI function calling), input validation, auto-generated documentation, and plugin SDK schema propagation.

### Phase 4: Plugin SDK + `declare_tool!` Macro (Days 12-15) ✅

- [x] Create `crates/agentzero-plugin-sdk/` crate (deps: `serde` + `serde_json` only)
- [x] Implement `ToolInput`, `ToolOutput` types (Serialize/Deserialize, success/error/with_warning constructors)
- [x] Implement `declare_tool!` macro generating: `az_alloc`, `az_tool_name`, `az_tool_execute` exports
  - [x] Uses Rust allocator (`Vec::leak`) instead of raw bump allocator — no conflicts with std allocator
  - [x] `pack_ptr_len` helper matches runtime's encoding (bits 0-31 = ptr, bits 32-63 = len)
  - [x] Handles invalid JSON input gracefully (returns structured error, not WASM trap)
- [x] Add `src/prelude.rs` re-exporting `ToolInput`, `ToolOutput`, `declare_tool!`
- [x] Add to workspace `Cargo.toml` members + dependencies
- [x] Update `plugin new --scaffold rust` to generate SDK-based project template
  - [x] Generates: `Cargo.toml` (cdylib), `.cargo/config.toml` (wasm32-wasip1), `src/lib.rs`, `manifest.json`
  - [x] Shows build + package commands after scaffolding
- [x] Tests (10 native + 3 integration):
  - [x] Native: ToolInput/ToolOutput serde, pack_ptr_len roundtrip/zero/max, sdk_alloc, macro expansion
  - [x] ABI pointer tests gated to `target_pointer_width = "32"` (native 64-bit truncates packed pointers)
  - [x] Integration: sample plugin built to `wasm32-wasip1` → loaded by WasmPluginRuntime → executed via `execute_v2_with_policy` → output verified (greeting, workspace root, error handling)

### Phase 5: Extract Official Plugin Packs (Days 16-19)

- [ ] Extract `agentzero-plugin-hardware` (3 tools: board_info, memory_map, memory_read)
- [ ] Extract `agentzero-plugin-integrations` (2 tools: composio, pushover)
- [ ] Extract `agentzero-plugin-cron` (7 tools: cron_add/list/remove/update/pause/resume, schedule)
- [ ] Each pack: uses `declare_tool!`, has manifest, includes integration tests
- [ ] Verify plugins load and execute correctly via WasmTool bridge

### Phase 6: CLI Enhancements + State Management (Days 20-22)

- [ ] Add `PluginState` / `PluginStateEntry` structs in `package.rs`
- [ ] Implement `plugin enable <id>` / `plugin disable <id>` subcommands
- [ ] Implement `plugin info <id>` subcommand
- [ ] Implement `plugin install --url <url>` (download, verify SHA256, install)
- [ ] Implement `plugin update [<id>]` (check registry for newer versions)
- [ ] Implement `plugin search <query>` (search registry index)
- [ ] Implement `plugin outdated` (list plugins with updates available)
- [ ] Update discovery to check `state.json` and skip disabled plugins
- [ ] Tests: enable/disable toggle, remote install + SHA256 verify, missing state = all enabled

### Phase 7: FFI Plugin Bridge (Days 23-25)

- [ ] Add `ToolCallback` trait to `crates/agentzero-ffi/src/lib.rs` via `#[uniffi::export(callback_interface)]`
- [ ] Add `register_tool()` method to `AgentZeroController`
- [ ] Create `crates/agentzero-infra/src/tools/ffi_bridge.rs` with `FfiTool` struct
- [ ] Implement `Tool` trait for `FfiTool` (delegates to callback)
- [ ] Tests: register + execute FFI tool, error propagation, tool appears in tool list

### Phase 8: Git-Based Registry (Days 26-30)

- [ ] Create registry repo structure (index/, categories.json, featured.json)
- [ ] Implement `plugin publish` (generate index JSON, open PR via `gh`)
- [ ] Implement registry client (clone/fetch index, cache for 1 hour)
- [ ] Seed registry with Phase 5 plugin packs
- [ ] Static website generated from registry repo (GitHub Pages)
- [ ] Tests: search returns results, install from registry + SHA256, update detects newer versions

### Verification (End-to-End)

- [ ] `cargo build -p agentzero --release` compiles with full plugin support
- [ ] `cargo build -p agentzero --profile release-min --no-default-features --features minimal` stays ~5MB
- [ ] `cargo test --workspace` — all existing tests pass
- [ ] Sample plugin: build → package → install → discover → execute → verify output
- [ ] CWD hot-reload: drop `.wasm` → tool reloads without restart
- [ ] `plugin disable` → not loaded next startup
- [ ] `plugin search` → finds registry results
- [ ] FFI tool registration works from Python/Swift
- [ ] Plugin timeout/memory limits enforced
- [ ] Plugin failure returns error, never crashes agent
- [ ] Binary size budgets unchanged (default: 30MB, minimal: 6MB)

### Current Measurements (Baseline from Sprint 19)

| Metric                     | Default (release) | Minimal (release-min) |
| -------------------------- | ----------------- | --------------------- |
| Binary size (macOS arm64)  | 18 MB             | 5.2 MB (4.95 MiB)     |
| Unique crate deps          | ~625              | 262                   |
| Cold-start (`--help`, min) | ~19ms             | ~21ms                 |
| Cold-start (`--help`, avg) | ~43ms             | ~41ms                 |

Previous sprint archived to `specs/sprints/19-lightweight-binary-landing-page-benchmarks.md`.
