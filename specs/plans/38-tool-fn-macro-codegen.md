# Plan: `#[tool_fn]` Macro + WASM Codegen Strategy

## Context

AgentZero's self-improvement story ("Describe it. It builds itself. And remembers.") needs tools to be easy to define — both by developers and by the agent itself. Today, dynamic tools use Shell/HTTP/LLM/Composite strategies which work but are limited in capability. A new `Codegen` strategy would let the agent write Rust tool source, compile to WASM, and load it via the existing plugin system — creating tools that are as capable as hand-written ones.

Phase 1 builds the `#[tool_fn]` macro that makes tool authoring trivial for developers (native tools). Phase 2 adds the WASM codegen strategy where the LLM generates code using `declare_tool!` (the plugin SDK macro, not `#[tool_fn]`) — since WASM plugins are synchronous and use a different ABI than native tools.

---

## Phase 1: `#[tool_fn]` Function-Level Macro

### What Already Exists
- `#[tool(name, description)]` struct-level attribute → [tool_attr.rs](crates/agentzero-macros/src/tool_attr.rs)
- `#[derive(ToolSchema)]` → JSON schema from struct fields → [tool_schema.rs](crates/agentzero-macros/src/tool_schema.rs)
- 72 tools using the current struct+trait pattern in `crates/agentzero-tools/src/`

### The Enhancement

Transform a plain async function into a full `Tool` implementation:

```rust
/// Fetch a URL and return a summary of its content.
#[tool_fn(name = "summarize_url")]
async fn summarize_url(
    /// The URL to fetch and summarize
    url: String,
    /// Maximum words in the summary
    #[serde(default)]
    max_words: Option<u32>,
    #[ctx] ctx: &ToolContext,
) -> anyhow::Result<ToolResult> {
    let body = reqwest::get(&url).await?.text().await?;
    Ok(ToolResult::text(truncate_words(&body, max_words.unwrap_or(200))))
}
```

**Generates:**
1. Input struct `SummarizeUrlInput` with `#[derive(Deserialize)]`
2. Tool struct `SummarizeUrlTool` (unit or with state)
3. `Tool` trait impl (name from attr, description from doc comment, schema from params)
4. Inner function with original body

### Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Macro name | `#[tool_fn]` | Keeps existing `#[tool]` struct-level macro backward-compatible |
| Description source | Doc comment on function | Natural Rust idiom, no duplication |
| Context param | `#[ctx] ctx: &ToolContext` | Clearly separates context from schema params |
| Stateful tools | `#[state] config: &Config` | Generates struct with field + `new(config)` constructor |
| Schema generation | Reuse helpers from `tool_schema.rs` | DRY — same type mapping logic |
| Serde attrs | Pass through to generated input struct | `#[serde(default)]`, `#[serde(rename)]` etc. |

### Implementation Steps

**Step 1: Add macro entry point**
- **File:** [crates/agentzero-macros/src/lib.rs](crates/agentzero-macros/src/lib.rs)
- Add `#[proc_macro_attribute] pub fn tool_fn(...)` delegating to new module

**Step 2: Implement `tool_fn.rs`**
- **New file:** `crates/agentzero-macros/src/tool_fn.rs`
- Parse `syn::ItemFn`, separate `#[ctx]`/`#[state]` params from input params
- Extract doc comments from function + params
- Generate input struct, tool struct, trait impl, inner function
- Edge cases: zero input params, all-optional, `Vec<T>`, complex types

**Step 3: Extract shared schema helpers**
- **File:** [crates/agentzero-macros/src/tool_schema.rs](crates/agentzero-macros/src/tool_schema.rs)
- Extract `rust_type_to_json_type()`, `is_option()`, `inner_type()` as reusable fns
- Both `#[derive(ToolSchema)]` and `#[tool_fn]` use the same helpers

**Step 4: Tests**
- **New file:** `crates/agentzero-macros/tests/tool_fn_tests.rs`
- Basic function, optional params, `#[state]`, no input, doc comments, serde passthrough, error cases

**Step 5: Convert 2-3 simple tools as proof**
- `PdfReadTool` ([pdf_read.rs](crates/agentzero-tools/src/pdf_read.rs)) — ~150 lines → ~30 lines
- One more stateless tool

---

## Phase 2: WASM Codegen Strategy

### What Already Exists
- **Plugin SDK:** `declare_tool!` macro in [agentzero-plugin-sdk](crates/agentzero-plugin-sdk/src/lib.rs) — generates ABI v2 exports
- **WASM Runtime:** wasmi (interpreter) + wasmtime (JIT) with fuel/epoch-based timeouts in [wasm.rs](crates/agentzero-plugins/src/wasm.rs)
- **Plugin Discovery:** Three-tier scanning (global/project/dev) with manifest validation in [package.rs](crates/agentzero-plugins/src/package.rs)
- **Pre-compilation:** `.wasm` modules compiled once and cached as `Arc<WasmModule>`
- **Isolation:** `WasmIsolationPolicy` — memory limits, execution timeout, fs/network gating, CoW overlay
- **CLI Scaffolding:** `agentzero plugin new --scaffold rust` generates a complete project in [plugin.rs](crates/agentzero-cli/src/commands/plugin.rs:602-700)
- **Dynamic Registry:** `DynamicToolRegistry` with 4 strategies (Shell/HTTP/LLM/Composite) in [dynamic_tool.rs](crates/agentzero-infra/src/tools/dynamic_tool.rs)
- **tool_create tool:** LLM-driven tool creation in [tool_create.rs](crates/agentzero-infra/src/tools/tool_create.rs)

### The Enhancement

Add a 5th strategy `Codegen` to `DynamicToolStrategy`:

```rust
Codegen {
    source: String,          // Rust source using #[tool_fn] or declare_tool!
    wasm_path: PathBuf,      // Compiled .wasm location
    wasm_sha256: String,     // Integrity hash
    compile_error: Option<String>,  // Last compile error (for retry/evolution)
}
```

### Pipeline

```
Agent decides it needs a new tool
  → tool_create(action: "create", strategy_hint: "codegen", description: "...")
    → LLM generates Rust source using declare_tool! macro
    → Write source to .agentzero/codegen/<tool_name>/src/lib.rs
    → Scaffold Cargo.toml + .cargo/config.toml
    → cargo build --target wasm32-wasip1 --release
    → If compile fails: feed errors back to LLM for retry (up to 3 attempts)
    → Compute SHA-256 of .wasm
    → Write manifest.json
    → Register in DynamicToolRegistry with strategy: Codegen
    → Load pre-compiled module into WasmPluginRuntime cache
    → Tool is immediately available to the agent
```

### Implementation Steps

**Step 6: Add `Codegen` variant to `DynamicToolStrategy`**
- **File:** [dynamic_tool.rs](crates/agentzero-infra/src/tools/dynamic_tool.rs)
- Add variant with `source`, `wasm_path`, `wasm_sha256`, `compile_error` fields
- Execution: delegate to `WasmPluginRuntime::execute_v2_precompiled()`

**Step 7: Build the compilation pipeline**
- **New file:** `crates/agentzero-infra/src/tools/codegen.rs`
- `CodegenCompiler` struct with methods:
  - `check_toolchain()` — verify `wasm32-wasip1` target installed, offer to install via `rustup target add`
  - `scaffold_project(name, source)` — write `Cargo.toml`, `.cargo/config.toml`, `src/lib.rs` to `.agentzero/codegen/<name>/`
  - `compile(project_dir)` → `Result<PathBuf, CompileError>` — spawn `cargo build`, capture stderr, return `.wasm` path
  - `compute_hash(wasm_path)` → SHA-256 hex digest
  - `load_module(wasm_path)` → pre-compile and cache `WasmModule`
- The `Cargo.toml` template depends only on `agentzero-plugin-sdk` and `serde_json`
- Compilation timeout: 60 seconds (configurable)

**Step 8: LLM source generation prompt**
- **File:** [tool_create.rs](crates/agentzero-infra/src/tools/tool_create.rs)
- Add `"codegen"` to `strategy_hint` enum values
- New system prompt for codegen strategy that instructs LLM to generate:
  ```rust
  use agentzero_plugin_sdk::prelude::*;
  declare_tool!("tool_name", handler);
  fn handler(input: ToolInput) -> ToolOutput { ... }
  ```
- Include compile error feedback loop: if `cargo build` fails, send errors to LLM and retry (max 3 attempts)

**Step 9: Codegen execution in DynamicTool**
- **File:** [dynamic_tool.rs](crates/agentzero-infra/src/tools/dynamic_tool.rs)
- In `DynamicTool::execute()`, add `Codegen` arm:
  - Load pre-compiled WASM module (cache in registry)
  - Execute via `WasmPluginRuntime::execute_v2_precompiled()`
  - Track quality metrics (success/failure/latency)

**Step 10: Integration with plugin discovery (optional)**
- Codegen tools live in `.agentzero/codegen/` which can be added as a discovery tier
- Or: codegen tools bypass discovery and are loaded directly from `DynamicToolRegistry`
- Recommend: bypass discovery (simpler, codegen tools are already tracked in registry)

**Step 11: Tests**
- Compile a minimal `declare_tool!` plugin from source at test time
- Verify execution through the `Codegen` strategy
- Test compile-error feedback loop (intentional syntax error → retry → fix)
- Test quality tracking (invocation counts, success rate)

---

## Files to Modify/Create

### Phase 1 (Macro)
| File | Change |
|---|---|
| `crates/agentzero-macros/src/lib.rs` | Add `tool_fn` entry point |
| `crates/agentzero-macros/src/tool_fn.rs` | **New** — macro implementation |
| `crates/agentzero-macros/src/tool_schema.rs` | Extract shared helpers |
| `crates/agentzero-tools/src/pdf_read.rs` | Convert as proof-of-concept |
| `crates/agentzero-macros/tests/tool_fn_tests.rs` | **New** — tests |

### Phase 2 (Codegen)
| File | Change |
|---|---|
| `crates/agentzero-infra/src/tools/dynamic_tool.rs` | Add `Codegen` variant + execution |
| `crates/agentzero-infra/src/tools/codegen.rs` | **New** — compiler pipeline |
| `crates/agentzero-infra/src/tools/tool_create.rs` | Add codegen strategy + LLM prompt |
| `crates/agentzero-infra/src/tools/mod.rs` | Wire up codegen module |

---

## Verification

### Phase 1
1. `cargo test -p agentzero-macros` — macro tests pass
2. `cargo test -p agentzero-tools` — converted tools pass existing tests
3. `cargo clippy --workspace` — zero warnings
4. Compare generated schema against hand-written schema for converted tools

### Phase 2
1. `cargo test -p agentzero-infra` — codegen tests pass (requires `wasm32-wasip1` target)
2. Manual test: `tool_create(action: "create", strategy_hint: "codegen", description: "a tool that reverses a string")`
3. Verify the generated tool executes correctly through the agent
4. Test compile failure → LLM retry → success path
5. Verify quality tracking updates after invocations
6. `cargo clippy --workspace` — zero warnings

---

---

## Additional Considerations

### Dependency Resolution for Codegen Tools
The scaffolded `Cargo.toml` starts with only `agentzero-plugin-sdk` + `serde_json`. But LLM-generated tools may need more (e.g., `regex`, `chrono`, `url`, `base64`). 

**Approach:** Maintain a curated **allowlist** of pre-approved crates with pinned versions in `CodegenCompiler`. The LLM prompt includes the allowlist so it knows what's available. If the LLM requests an unlisted crate, the compiler rejects it with an error suggesting alternatives. This keeps compile times predictable and prevents supply-chain risk.

**Initial allowlist:** `serde_json`, `regex`, `chrono`, `url`, `base64`, `sha2`, `hex`, `rand`, `csv`, `serde` (+ derive).

### SDK Availability
`agentzero-plugin-sdk` must be resolvable by `cargo build` inside `.agentzero/codegen/<tool>/`. Options:
1. **Path dependency** pointing to the installed AgentZero's SDK crate — works when built from source
2. **Publish to crates.io** — works everywhere but adds a release dependency
3. **Vendor a pre-built copy** — bundle SDK source in `.agentzero/codegen/.sdk/`

**Recommendation:** Start with path dependency (simplest, works for our use case). Add crates.io fallback later when plugin ecosystem matures.

### Toolchain Bootstrapping
When `wasm32-wasip1` target isn't installed:
- `check_toolchain()` detects this via `rustup target list --installed`
- If missing: log a clear error message with the install command (`rustup target add wasm32-wasip1`)
- Do NOT auto-install — modifying the user's toolchain without consent is too aggressive
- If `rustc` is entirely absent: codegen strategy is unavailable, `tool_create` falls back to Shell/LLM strategies with a warning

### Shared Build Cache
Each codegen tool gets its own Cargo project, but they share dependencies. Without optimization, every new tool recompiles `serde_json` etc. from scratch.

**Approach:** All codegen projects use a shared `CARGO_TARGET_DIR` at `.agentzero/codegen/.target/`. First build is slow (~30s), subsequent builds only compile the new tool's `lib.rs` (~2-5s). Set via env var in the `cargo build` subprocess.

### Rebuild Avoidance
On agent restart, don't recompile tools whose source hasn't changed:
- Store source hash in `DynamicToolDef` alongside `wasm_sha256`
- On load: if `.wasm` exists and source hash matches → skip compilation, load cached `.wasm`
- If `.wasm` missing or hash mismatch → recompile

### Hot-Loading: No Restart Required
New codegen tools are available **immediately** without restarting the gateway/daemon. This works because:

1. `DynamicToolRegistry` implements `ToolSource` trait ([agent.rs:348](crates/agentzero-core/src/agent.rs#L348))
2. The agent queries `ToolSource::additional_tools()` on **every tool loop iteration** ([agent.rs:437](crates/agentzero-core/src/agent.rs#L437)) — "The agent queries this source on each tool loop iteration to discover newly registered tools"
3. When `tool_create` registers a new codegen tool, the `RwLock<Vec<DynamicToolDef>>` is updated in-place
4. Next loop iteration picks up the new tool automatically

**Three hot-reload systems already exist:**
1. **Config:** `ConfigWatcher` polls mtime every 2s → `watch::Receiver<AgentZeroConfig>` → gateway reads via `effective_*()` methods ([lib.rs:302-323](crates/agentzero-gateway/src/lib.rs#L302-L323))
2. **Dynamic tools:** `ToolSource::additional_tools()` queried every agent loop iteration → new `DynamicToolRegistry` entries picked up immediately ([agent.rs:437](crates/agentzero-core/src/agent.rs#L437))
3. **Dev plugins:** `PluginWatcher` uses `notify` crate to watch `.wasm` file changes with debounce ([watcher.rs](crates/agentzero-plugins/src/watcher.rs))

**What we need to add for codegen specifically:**
- The pre-compiled `WasmModule` must be cached in a shared `Arc<RwLock<HashMap<String, WasmModule>>>` inside `CodegenCompiler` (or the registry itself)
- When `DynamicTool::execute()` runs a `Codegen` strategy, it looks up the cached module — no disk I/O on the hot path
- If the module isn't cached (e.g., after daemon restart), it lazy-loads from the `.wasm` file and caches for subsequent calls
- The `WasmEngine` is already shared as `Arc<WasmEngine>` across all plugins — codegen tools reuse the same engine instance

This is the same pattern the existing plugin system uses: compile once, cache the module, create a cheap `Store` per execution.

**Minor gap: tool policy propagation.** `enable_dynamic_tools` and `ToolSecurityPolicy` are read once in `build_runtime_execution()`. If someone changes tool policy in the config mid-session, it won't take effect until the next session. This is acceptable for now — tool policy changes are rare and should take effect on next request, not mid-conversation. A future enhancement could subscribe to `live_config` changes and update the tool set, but it's not needed for this plan.

### Security: Sandbox is Sufficient
LLM-generated code runs inside the WASM sandbox with `WasmIsolationPolicy`:
- No network access by default
- No filesystem write by default
- Memory-limited (256MB)
- Execution-time-limited (30s fuel)
- CoW overlay for any writes

The sandbox policy is the security boundary, not source review. This is the same trust model as the existing Shell strategy (which runs arbitrary commands). The WASM sandbox is actually **more** secure since it's capability-based.

### Garbage Collection
As tools are created, evolved, and deleted, stale projects accumulate:
- When a `Codegen` tool is deleted from `DynamicToolRegistry`, also delete its `.agentzero/codegen/<name>/` directory and `.wasm` file
- Add a `codegen_gc()` method to `CodegenCompiler` that removes directories not referenced by any registered tool
- Run GC on agent startup (lightweight — just a directory scan)

### `#[tool_fn]` vs `declare_tool!` — Two Contexts
These serve different purposes and should NOT be confused:

| | `#[tool_fn]` (Phase 1) | `declare_tool!` (Phase 2 codegen) |
|---|---|---|
| **Context** | Native Rust tools compiled into the binary | WASM plugin tools |
| **Execution** | Async, has `ToolContext` | Synchronous, `ToolInput` → `ToolOutput` |
| **Dependencies** | Full `agentzero-core` | Only `agentzero-plugin-sdk` |
| **Who writes it** | Developers | LLM (via `tool_create`) |
| **ABI** | Direct Rust trait dispatch | WASM ABI v2 (alloc/execute/name exports) |

The codegen LLM prompt must target `declare_tool!`, not `#[tool_fn]`.

---

## Future Extensions (not in scope)
- Hot-reload: watch `.agentzero/codegen/` for source changes, recompile automatically
- Evolution: when a codegen tool's quality score drops, trigger LLM to generate improved version (generation + 1)
- Tool sharing: export codegen tools as `.tar` bundles with source + WASM for other agents
- Allowlist expansion: let agents request new crate additions (with human approval gate)
