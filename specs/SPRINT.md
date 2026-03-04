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
