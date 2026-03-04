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

### Phase 2: WasmTool Bridge + Module Caching (Days 5-7)

- [ ] Create `crates/agentzero-infra/src/tools/wasm_bridge.rs` with `WasmTool` struct
- [ ] Implement `Tool` trait for `WasmTool` (delegates to `execute_v2` via `tokio::spawn_blocking`)
- [ ] Add `ModuleCache` to `wasm.rs`: `load_or_compile(engine, wasm_path, sha256) -> Module`
- [ ] Cache AOT modules at `{plugin_dir}/.cache/plugin.cwasm` + `source.sha256`
- [ ] Add optional `agentzero-plugins` dep behind `wasm-plugins` feature in `agentzero-infra`
- [ ] Tests: WasmTool executes v2 plugin, cache creation/invalidation, corrupt cache fallback

### Phase 3: Plugin Discovery + Hot-Reload (Days 8-11)

- [ ] Add `discover_plugins()` in `package.rs` scanning global → project → CWD paths
- [ ] Wire discovery into `default_tools()` behind `wasm-plugins` feature
- [ ] Add `enable_wasm_plugins: bool` to `ToolSecurityPolicy`
- [ ] Expand `PluginConfig` in `agentzero-config`
- [ ] Create `crates/agentzero-plugins/src/watcher.rs` using `notify` crate (behind `plugin-dev` feature)
- [ ] Hot-reload: watch `$CWD/plugins/` for `.wasm` changes, invalidate cache, reload
- [ ] Add `wasi` and `plugin-dev` features through CLI → bin crate feature chain
- [ ] Tests: empty dirs = zero overhead, valid plugin discovery, CWD dev_mode, hot-reload, invalid manifest = warn + skip

### Phase 4: Plugin SDK + `declare_tool!` Macro (Days 12-15)

- [ ] Create `crates/agentzero-plugin-sdk/` crate (deps: `serde` + `serde_json` only)
- [ ] Implement `ToolInput`, `ToolOutput`, `declare_tool!` macro
- [ ] Macro generates: `az_alloc`, `az_tool_name`, `az_tool_execute` exports with bump allocator
- [ ] Add `src/prelude.rs` re-exporting public API
- [ ] Add to workspace `Cargo.toml` members + dependencies
- [ ] Update `plugin new --scaffold rust` to generate SDK-based project template
- [ ] Tests: sample plugin builds to `wasm32-wasip1`, macro generates correct ABI, integration test (build → package → install → discover → execute)

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

| Metric | Default (release) | Minimal (release-min) |
|---|---|---|
| Binary size (macOS arm64) | 18 MB | 5.2 MB (4.95 MiB) |
| Unique crate deps | ~625 | 262 |
| Cold-start (`--help`, min) | ~19ms | ~21ms |
| Cold-start (`--help`, avg) | ~43ms | ~41ms |

Previous sprint archived to `specs/sprints/19-lightweight-binary-landing-page-benchmarks.md`.
