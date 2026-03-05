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

### Phase 6: SQLite Conversation Memory Encryption — [x] DONE

- [x] Switch `rusqlite` from `bundled` to `bundled-sqlcipher` (workspace Cargo.toml)
- [x] Add `key: Option<&StorageKey>` parameter to `SqliteMemoryStore::open()` with `PRAGMA key`
- [x] Auto-migrate existing plaintext databases via `sqlcipher_export` on first encrypted open
- [x] Update runtime call site (`agentzero-infra/src/runtime.rs`) to pass `StorageKey`
- [x] Update CLI call sites (`agentzero-cli/src/commands/memory.rs`) to pass `StorageKey`
- [x] Tests: encrypted roundtrip, wrong key rejection, plaintext migration preserves data
- [x] Update `THREAT_MODEL.md` with SQLite encryption entries (sections 5.1, 5.2)

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

## Sprint 21: Structured Tool Use

**Goal:** Wire tool schemas into provider API calls so LLMs use native tool-use/function-calling APIs instead of text-based `tool:name input` parsing.

**Predecessor:** Sprint 20 (Plugin Architecture) added `description()` and `input_schema()` to all 50+ tools.

**Branch:** `refactor/crate-consolidation`

### Phase 1: Provider Tool Definitions — [x] DONE

- [x] Add `ToolDefinition`, `ToolUseRequest`, `ToolResultMessage`, `ConversationMessage`, `StopReason` to `core/src/types.rs`
- [x] Extend `ChatResult` with `tool_calls` and `stop_reason` fields, derive `Default`
- [x] Extend `Provider` trait with `complete_with_tools()` default method
- [x] Implement for `AnthropicProvider` (handle `stop_reason: "tool_use"`)
- [x] Implement for `OpenAiCompatibleProvider` (handle `finish_reason: "tool_calls"`)
- [x] Update all `ChatResult` construction sites (10 locations across 5 crates)
- [x] Update `core/src/lib.rs` re-exports
- [x] Add comprehensive tests (core types + both providers)

### Phase 2: Agent Loop — Structured Tool Dispatch — [x] DONE

- [x] Add `build_tool_definitions()` and `has_tool_definitions()` to Agent
- [x] Add `respond_with_tools()` path gated by `config.model_supports_tool_use`
- [x] Keep text-based `parse_tool_calls()` as fallback (no tools with schemas → text path)
- [x] Add `prepare_tool_input()` — single-field extraction for plain-string tools, JSON serialization for complex tools
- [x] Parallel tool call support via `config.parallel_tools` + gated tool fallback
- [x] Loop detection: no-progress, ping-pong, failure streak (all reused from text path)
- [x] Tool errors → `ToolResultMessage { is_error: true }` (LLM sees error and adapts, no abort)
- [x] Add `ToolDefinition::from_tool()` helper
- [x] Add 19 new tests (structured provider, echo/failing/upper tools, all paths covered)

### Phase 3: Conversation Message History — [x] DONE

- [x] Replace `prompt: String` accumulation with `messages: Vec<ConversationMessage>` in structured path
- [x] Memory integration: `memory_to_messages()` converts recent memory to initial conversation context
- [x] Add `ConversationMessage::char_count()` for truncation budgeting
- [x] `truncate_messages()` preserves first user message + most recent messages, drops from middle

### Phase 4: Streaming Tool Use — [x] DONE

- [x] `ToolCallDelta` struct + extended `StreamChunk` with `tool_call_delta` field
- [x] `complete_streaming_with_tools()` added to Provider trait with default impl
- [x] Anthropic SSE: `parse_sse_event()` → `SseEvent` enum (TextDelta, ToolUseStart, ToolUseInputDelta, ContentBlockStop, MessageDelta)
- [x] Anthropic `complete_streaming_with_tools()` with tool call accumulation
- [x] OpenAI SSE: `parse_openai_sse_event()` → `OpenAiSseEvent` enum (ContentDelta, ToolCallDelta, Finished, Done)
- [x] OpenAI `complete_streaming_with_tools()` with tool call accumulation
- [x] Backward-compatible `parse_sse_text_delta()` / `parse_openai_sse_delta()` wrappers
- [x] 22 new SSE parser tests (10 Anthropic, 12 OpenAI)

### Phase 5: Schema Validation + Auto-Documentation — [x] DONE

- [x] Lightweight JSON Schema validator in `core/src/validation.rs` (type, required, properties, items, enum)
- [x] 19 validation tests
- [x] `agentzero tools list/info/schema` CLI commands
- [x] `ToolsCommands` subcommand enum with `--with-schema`, `--json`, `--pretty` flags
- [x] 2 CLI tool integration tests

---

## Sprint 22: Streaming Agent Loop, Gateway Wiring & Hardening

**Goal:** Wire streaming end-to-end from provider layer through agent loop, runtime, CLI, and gateway. Close critical gaps: system prompt support, MCP connection caching, tool schema coverage, FFI parity.

**Branch:** `feat/streaming-gateway`

### Phase 1: Streaming Agent Loop — [x] DONE

- [x] Add `StreamSink` type alias to `types.rs`, re-export from `lib.rs`
- [x] Add `Agent::respond_streaming()` — timeout wrapper passing sink to `respond_inner`
- [x] Modify `respond_with_tools()` to accept `Option<StreamSink>`, call `complete_streaming_with_tools()` when present
- [x] Modify `call_provider_with_context()` to accept `Option<StreamSink>`, call `complete_streaming()` for text-only path
- [x] 8 new tests: streaming text-only, single tool call, multi-iteration, timeout, no-schema fallback, tool error recovery, parallel tools, done-chunk sentinel
- [x] 190 core tests pass, 0 clippy warnings

### Phase 2: Runtime Streaming Channel — [x] DONE

- [x] Add `run_agent_streaming()` returning `(UnboundedReceiver<StreamChunk>, JoinHandle<Result<RunAgentOutput>>)`
- [x] Extract `build_runtime_execution()` for shared setup between streaming/non-streaming
- [x] Tests (3): receiver delivers chunks, handle resolves to output, output text matches accumulated chunks
- [x] 17 runtime tests pass, 0 clippy warnings

### Phase 3: CLI `--stream` Flag — [x] DONE

- [x] Add `--stream` flag to Agent command in `cli.rs`
- [x] In `AgentCommand::run()`: dispatch to `run_streaming()` when `--stream` set
- [x] `run_streaming()`: loop `rx.recv()`, `write!` + `flush` each delta
- [x] Tests (2): stream flag defaults false, error propagation with --stream
- [x] 7 agent tests + 1 integration test pass

### Phase 4: System Prompt Support — [x] DONE

- [x] Add `ConversationMessage::System { content }` variant to `types.rs`
- [x] Add `system_prompt: Option<String>` to `AgentConfig` and `AgentSettings`
- [x] In `respond_with_tools()`: prepend `System` message when configured
- [x] Anthropic: extract system from messages → `MessagesRequest.system` field; filter from messages
- [x] OpenAI: map `System` → `{"role": "system", "content": "..."}`
- [x] Thread `system_prompt` through `runtime.rs` from config
- [x] Fix `delegate.rs`: use `<system>...</system>` tags for single-shot, `AgentConfig.system_prompt` for agentic
- [x] Tests (8): serde round-trip, char_count, agent prepend, agent omit-when-none, agent persist-across-iterations, config default, Anthropic filter+extract, OpenAI system role
- [x] 198 core tests, 125 provider tests, 16 delegate tests pass, 0 clippy warnings

### Phase 5: Gateway Agent Wiring — [x] DONE

- [x] Add `agentzero-infra`, `agentzero-config`, `agentzero-core`, `async-stream` to gateway Cargo.toml
- [x] Add `config_path`/`workspace_root` to `GatewayState` and `GatewayRunOptions`
- [x] Load `[gateway]` config from TOML; enforce `allow_public_bind`; wire perplexity filter from security config
- [x] Replace `api_chat` echo with `run_agent_once()` (returns 503 without config)
- [x] Replace `v1_chat_completions` echo with `run_agent_once()` + model override passthrough
- [x] Add SSE streaming to `v1_chat_completions` when `stream: true` via `async_stream` + `axum::response::Sse`
- [x] Replace `ws_chat` echo with `run_agent_streaming()` — delta frames + `{"type":"done"}` sentinel
- [x] Use `pairing_code_valid()` (TTL-aware) instead of raw code access in pair handler
- [x] Tests (5): api_chat 503 without config, v1_chat_completions 503, stream 503, config fields active, pairing TTL expiry
- [x] 55 gateway tests pass, 0 clippy warnings

### Phase 6: MCP Connection Caching — [x] DONE

- [x] Add `McpSession` struct with cached stdin/stdout/child, auto-incrementing request ID, `tool_schemas` HashMap
- [x] Add `sessions: HashMap<String, Arc<Mutex<Option<McpSession>>>>` to `McpTool` for per-server caching
- [x] `spawn_session()` initializes handshake + caches `inputSchema` from `tools/list`
- [x] `execute()` lazily connects, calls `call_tool_cached()`, on error clears slot and retries once
- [x] Implement `input_schema()` for `McpTool` (dispatcher schema: server/tool/arguments)
- [x] Tests (5): parse input, reject unknown server, reject unknown request, input_schema returns schema, session slots created
- [x] 35 infra tests pass, 0 clippy warnings

### Phase 7: Tool Schema Coverage — [x] DONE

- [x] Add `input_schema()` to 28 remaining tools across 14 files:
  - Batch A (5): cli_discovery, delegate_coordination_status, url_validation, proxy_config, pushover (hardware_board_info was already counted here)
  - Batch B (7): composio, agents_ipc, delegate, model_routing_config, task_plan, process_tool, hardware_board_info/memory_map/memory_read
  - Batch C (7): schedule, cron_add/list/remove/update/pause/resume
  - Batch D (5): sop_list/status/advance/approve/execute
  - Batch E (4): subagent_spawn/list/manage, wasm_module/wasm_tool
- [x] WasmTool (wasm_bridge) and ProcessPluginTool (plugin.rs) skipped — dynamic plugin wrappers with no fixed schema
- [x] MCP tool `input_schema()` already added in Phase 6
- [x] 243 tools tests pass, 0 clippy warnings

### Phase 8: FFI Node Bindings Parity — [x] DONE

- [x] Add `get_config()` → `NodeAgentZeroConfig` (with `From<CoreConfig>` reverse conversion)
- [x] Add `update_config(config)` → `()`
- [x] Add `register_tool(name, description)` with `NodeStubCallback` implementing `ToolCallback`
- [x] Add `send_message_async(message)` → `Promise<NodeAgentResponse>` via `spawn_blocking`
- [x] Add `registered_tool_names()` → `Vec<String>`
- [x] 19 FFI tests pass (core-level tests already cover get_config, update_config, register_tool round-trips)

### Phase 9: Infra Integration Tests — [x] DONE

- [x] 8 new integration tests in `crates/agentzero-infra/tests/runtime_integration.rs`
- [x] Tests: run_agent_once_with_mock_provider, run_agent_streaming_delivers_chunks, run_agent_once_records_audit_events, run_agent_once_provider_error_propagates, run_agent_streaming_handle_resolves_to_output, default_tools_read_file_present, default_tools_write_file_absent_by_default, default_tools_all_have_schemas

Previous sprint archived to `specs/sprints/20-plugin-architecture.md`.

---

## Sprint 22H: Hardening, Coverage & Polish

**Goal:** Close correctness gaps, wire dead code, harden error paths, expand test coverage, and polish documentation. Audit-driven from post-Sprint 21 codebase review.

**Branch:** `feat/plugins`

### Phase 1: Correctness (P0)

- [x] Wire `validate_json` into tool dispatch — call schema validation in `prepare_tool_input()` before execution, return error on violation
- [x] Register `WebFetchTool`, `UrlValidationTool`, `AgentsIpcTool`, `HttpRequestTool` in `default_tools()` with policy flags
- [x] Fix `WasmTool::description()` and `FfiTool::description()` — return actual manifest description instead of placeholder
- [x] Document `agentzero tools list/info/schema` CLI commands in site docs

### Phase 2: Security & Correctness (P1)

- [x] Fix dead config knobs — investigated: all 4 fields (`context_aware_parsing`, `enable_composio`, `enable_pushover`, `require_first_visit_approval`) are already properly wired through config → policy → enforcement
- [x] Replace `eprintln!` with `tracing::warn!` in WASM runtimes
- [x] Fix `model_supports_tool_use` default to `false` (unknown models should not assume tool support)
- [x] Replace unsafe `unwrap()` calls in `validation.rs` with `map_or` / `if let Some`

### Phase 3: Test Coverage (P2)

- [x] Add tests for `wasm_bridge.rs` — 7 tests added in Sprint 22
- [x] Add tests for `runtime::parse_hook_mode()` — 4 tests: trims_whitespace, rejects_wrong_case, rejects_empty_string, rejects_whitespace_only
- [x] Add gateway TCP-level integration test — `tcp_health_endpoint_over_real_listener` already exists
- [x] Add full-loop agent integration test with real tool — `full_loop_agent_with_tool_call_round_trip` (ScriptedToolProvider + SchemaEchoTool)

### Phase 4: Polish (P3)

- [x] Add config validation for `gateway.port`, `gateway.host`, `autonomy.level`, `max_cost_per_day_cents` + 5 negative-path tests
- [x] Add `//!` module-level doc comments — all 15 `lib.rs` files already have them
- [x] Fix AGENTS.md Rule 12 doc path references (`tools-plugins.md` → `tools.md`, `gateway-api.md` → `gateway.md`)
- [x] Replace `fs2` with `fd-lock` — already done (no `fs2` in workspace)
- [x] Daemon `std::thread::sleep` — investigated: used in sync signal-handling context (SIGKILL wait), not applicable for tokio
