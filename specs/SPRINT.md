# AgentZero Sprint Plan

## Sprint 20.5: Workspace Consolidation (46 → 16 crates)

**Goal:** Consolidate 30 micro-crates into their natural parents. Each remaining crate corresponds to a real deployment or consumption boundary (CLI, embeddable library, gateway service, FFI bindings, standalone channels, plugin system).

**Branch:** `refactor/crate-consolidation`

### Target State (16 workspace members)

| # | Crate | Absorbs | Status |
|---|-------|---------|--------|
| 1 | `agentzero-core` | `common`, `security`, `delegation`, `routing` | Done |
| 2 | `agentzero-storage` | `crypto`, `memory` | Done |
| 3 | `agentzero-config` | *(none)* | — |
| 4 | `agentzero-providers` | *(none)* | — |
| 5 | `agentzero-auth` | *(none)* | — |
| 6 | `agentzero-tools` | `autonomy`, `hardware`, `cron`, `skills` | Done |
| 7 | `agentzero-infra` | `runtime` | Done |
| 8 | `agentzero-channels` | `leak-guard` | Done |
| 9 | `agentzero-plugins` | *(none)* | — |
| 10 | `agentzero-plugin-sdk` | *(none)* | — |
| 11 | `agentzero-gateway` | *(none)* | — |
| 12 | `agentzero-ffi` | *(none)* | — |
| 13 | `agentzero-cli` | 18 CLI-only crates | Done |
| — | `agentzero-testkit` | *(none)* | — |
| — | `agentzero-bench` | *(none)* | — |
| — | `bin/agentzero` | *(none)* | — |

---

### Phase 1: Merge into `agentzero-core` — [x] DONE (fee6e87)

- [x] Merge `common` (894 lines) → `core/src/common/`
- [x] Merge `security` (1,469 lines) → `core/src/security/`
- [x] Merge `delegation` (182 lines) → `core/src/delegation.rs`
- [x] Merge `routing` (298 lines) → `core/src/routing.rs`
- [x] Update all consumers (~22 source files, ~12 Cargo.toml files)
- [x] Remove 4 crate directories + workspace entries

### Phase 2: Merge into `agentzero-storage` — [x] DONE (8e1114c)

- [x] Merge `crypto` (262 lines) → `storage/src/crypto/`
- [x] Merge `memory` (458 lines) → `storage/src/memory/`
- [x] Add `memory-sqlite` and `memory-turso` feature flags to storage

### Phase 3: Merge into `agentzero-tools` — [x] DONE (8e1114c)

- [x] Merge `autonomy` (378 lines) → `tools/src/autonomy.rs`
- [x] Merge `hardware` (52 lines) → `tools/src/hardware.rs` (gated by `hardware` feature)
- [x] Merge `cron` (179 lines) → `tools/src/cron_store.rs`
- [x] Merge `skills` (283 lines) → `tools/src/skills/`
- [x] Fix clippy `module_inception` (skills.rs → store.rs)

### Phase 4: Merge `leak-guard` into `agentzero-channels` — [x] DONE (8e1114c)

- [x] Merge `leak-guard` (337 lines) → `channels/src/leak_guard.rs`

### Phase 5: Merge `runtime` into `agentzero-infra` — [x] DONE

- [x] Copy `runtime/src/lib.rs` → `infra/src/runtime.rs`
- [x] Move integration test → `infra/tests/`
- [x] Add deps to infra: `agentzero-auth`, `agentzero-config`
- [x] Add feature flags: `memory-sqlite`, `memory-turso`
- [x] Update CLI: `agentzero_runtime::` → `agentzero_infra::runtime::`
- [x] Update FFI: `agentzero_runtime::` → `agentzero_infra::runtime::`
- [x] Update feature chain: CLI `memory-sqlite` → `agentzero-infra/memory-sqlite`
- [x] Remove `agentzero-runtime` from workspace

### Phase 6: Merge 18 CLI-only crates into `agentzero-cli` — [x] DONE

- [x] 7 zero-dep leaf crates: approval, coordination, cost, goals, identity, integrations, multimodal
- [x] 11 storage-dependent crates: health, heartbeat, doctor, daemon, hooks, service, tunnel, peripherals, rag, update, local
- [x] Updated 14 command files with new import paths
- [x] Updated integration tests
- [x] Removed 18 workspace entries + crate directories

### Phase 7: Cleanup + Verification — [x] DONE

- [x] All 30 deleted crate directories removed
- [x] Workspace `Cargo.toml` cleaned: 16 members remain
- [x] Re-exported `tracing` from `agentzero-core`
- [x] `cargo test --workspace` — 1,426 tests pass, 0 failures
- [x] `cargo clippy --workspace` — 0 warnings
- [x] `cargo check --workspace` — clean

---

## Sprint 20.6: Plugin Security Hardening

**Goal:** Fix critical security vulnerabilities and robustness issues in the plugin system before expanding feature surface in Sprint 21.

**Branch:** `feat/plugins`

### Phase 1: Path Traversal Fix in Tar Extraction — [x] DONE

- [x] Reject tar entries containing `..` or starting with `/`
- [x] Reject symlink entries in plugin packages
- [x] Add success-path tests (valid package installs correctly — existing)
- [x] Add negative-path tests (traversal paths, absolute paths, symlinks rejected)

### Phase 2: Semver-Based Version Comparison — [x] DONE

- [x] Add `semver` crate dependency to `agentzero-plugins`
- [x] Use `semver::Version::parse()` in `discover_plugins()` and `list_installed_plugins()`
- [x] Fallback to string comparison for non-semver versions
- [x] Add tests: `"0.2.0"` vs `"0.10.0"` → `0.10.0` wins; `"9.0.0"` vs `"10.0.0"` → `10.0.0` wins

### Phase 3: Watcher Debouncing — [x] DONE

- [x] Add debounce window (200ms) to `PluginWatcher`
- [x] Deduplicate events by path within debounce window
- [x] Add test: rapid writes produce single reload event

### Phase 4: File Locking for Plugin Operations — [x] DONE

- [x] Add `fs2` dependency for cross-platform file locking
- [x] Lock plugin install root during install/remove operations
- [x] Add tests: lock file created during install and remove

### Phase 5: AGENTS.md Compliance — [x] DONE

- [x] Refactor `generate_registry_entry` to use `RegistryEntryParams` struct (Rule 10: >3-4 params)
- [x] Create `docs/security/THREAT_MODEL.md` with plugin system threat model (Rule 7)
- [x] **Rule 9 exception (documented):** `PluginState` uses direct `std::fs` JSON persistence instead of `agentzero-storage`. Plugin metadata (enabled/disabled, version, install source) is non-sensitive and does not warrant encryption-at-rest. Adding `agentzero-storage` as a dependency would tightly couple the plugin crate to the storage backend.

---

## Sprint 20.7: wasmi Runtime Migration + Binary Slimming

**Goal:** Replace wasmtime with wasmi as the default WASM runtime for plugin execution. Enables WASM plugins on constrained devices (ESP32, Raspberry Pi). Keep wasmtime as optional `wasm-jit` feature. Add plugin warming, TLS feature gating, and build variant tooling.

**Branch:** `refactor/crate-consolidation`

### Phase 1: Cargo.toml Feature Restructuring — [x] DONE

- [x] Add wasmi/wasmi_wasi workspace deps
- [x] Restructure agentzero-plugins features (`wasm-runtime` → wasmi, `wasm-jit` → wasmtime)
- [x] Add `wasm-jit` feature propagation through infra → cli → binary
- [x] Add `tls-rustls` / `tls-native` features propagated through binary → cli → channels
- [x] Remove hardcoded `rustls-tls` from reqwest/tokio-tungstenite workspace defaults

### Phase 2: wasmi Backend Implementation — [x] DONE

- [x] Implement wasmi `runtime_impl` in `wasm.rs` (fuel metering, StoreLimits, WASI)
- [x] Implement wasmi `ModuleCache` (no-op passthrough — wasmi has no AOT)
- [x] Register `az_log` and `az_env_get` host functions for wasmi
- [x] v1 and v2 execute paths, import validation

### Phase 3: Re-gate wasmtime Backend — [x] DONE

- [x] Move existing wasmtime `runtime_impl` behind `#[cfg(feature = "wasm-jit")]`

### Phase 4: Test Updates — [x] DONE

- [x] Split `ModuleCache` tests into cfg-gated modules (wasmi vs wasmtime)
- [x] All 95+ plugin tests pass with wasmi backend
- [x] Existing integration tests (SDK, plugin integration) pass unchanged

### Phase 5: Plugin Warming — [x] DONE

- [x] Add `create_engine()`, `compile_module()`, `execute_v2_precompiled()` to wasmi backend
- [x] Add same precompiled methods to wasmtime (`wasm-jit`) backend
- [x] Update `wasm_bridge.rs` `WasmTool` to pre-compile engine+module at init
- [x] Expose `WasmEngine`/`WasmModule` type aliases for downstream crates

### Phase 6: Build Variant Tooling — [x] DONE

- [x] Add `just build`, `build-minimal`, `build-server`, `build-jit`, `build-native-tls`, `build-sizes` commands
- [x] Update `install.sh` with `server` variant (plugins + gateway, no TUI)
- [x] Interactive variant picker: default / server / minimal

---

## Sprint 21: Structured Tool Use (Next)

**Goal:** Wire tool schemas into provider API calls so LLMs use native tool-use/function-calling APIs instead of text-based `tool:name input` parsing.

**Predecessor:** Sprint 20 (Plugin Architecture) added `description()` and `input_schema()` to all 50+ tools.

### Phase 1: Provider Tool Definitions

- [ ] Add `ToolDefinition`, `ToolUseRequest`, `ToolResultMessage` to `core/src/types.rs`
- [ ] Extend `Provider` trait with `complete_with_tools()` default method
- [ ] Implement for `AnthropicProvider` (handle `stop_reason: "tool_use"`)
- [ ] Implement for `OpenAiCompatibleProvider` (handle `finish_reason: "tool_calls"`)

### Phase 2: Agent Loop — Structured Tool Dispatch

- [ ] Add `build_tool_definitions()` to Agent
- [ ] Add `respond_with_tools()` path gated by `config.model_supports_tool_use`
- [ ] Keep text-based `parse_tool_calls()` as fallback
- [ ] Input validation against tool schemas
- [ ] Parallel tool call support via native provider response

### Phase 3: Conversation Message History

- [ ] Replace `prompt: String` accumulation with `messages: Vec<ConversationMessage>`
- [ ] Memory integration for structured messages
- [ ] Truncation preserves most recent tool interactions

### Phase 4: Streaming Tool Use

- [ ] Anthropic SSE: `content_block_start/delta/stop` for tool_use blocks
- [ ] OpenAI SSE: `tool_calls` field accumulation
- [ ] Mixed text + tool_use streaming

### Phase 5: Schema Validation + Auto-Documentation

- [ ] Lightweight JSON Schema validator in `core/src/validation.rs`
- [ ] `agentzero tools list/info/schema` CLI commands

Previous sprint archived to `specs/sprints/20-plugin-architecture.md`.
