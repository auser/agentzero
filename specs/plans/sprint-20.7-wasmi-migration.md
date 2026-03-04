# Plan: Replace wasmtime with wasmi (+ optional wasmtime JIT)

## Context

The plugin system uses wasmtime v42 as the WASM runtime. wasmtime is a JIT-based runtime that pulls in Cranelift and 50+ transitive crates, adding significant compile time and binary size. More critically, it **cannot run on embedded targets** (ESP32, bare-metal) because it requires OS-level primitives (mmap, signals, threads).

This means WASM plugins — the primary extensibility mechanism — are only available on desktop/server builds. For a system that targets constrained devices, this is an architectural gap.

**Solution:** Replace wasmtime with **wasmi** as the default WASM runtime. wasmi is a pure-Rust WASM interpreter with no JIT, no OS dependencies, tiny footprint (~100KB), and `no_std` support. Same `.wasm` plugin format — existing SDK, manifests, packaging, and registry are unchanged. Keep wasmtime behind an optional `wasm-jit` feature for power users.

**Performance note:** wasmi is ~10-100x slower than wasmtime JIT for compute-heavy workloads. For our use case (JSON-in/JSON-out tool calls), execution time is dominated by I/O and serde — interpreter overhead is negligible. Fuel-based timeout metering is also more deterministic than epoch interruption and eliminates the need for timer threads.

**Binary size (measured):**

| Build | Profile | Size |
|-------|---------|------|
| Full (all features) | release | 19.0 MB |
| Minimal (no plugins/gateway/tui) | release-min | 5.2 MB |
| Minimal + plugins (wasmtime) | release-min | 9.4 MB |
| **Minimal + plugins (wasmi)** | **release-min** | **~6-7 MB (target)** |
| + native-tls (optional) | release-min | ~5-6 MB |

Wasmtime adds 4.2MB even with fat LTO + opt-level z. wasmi should add ~0.5-1MB.

**Security (P0 — non-negotiable):** wasmi provides identical WASM sandbox guarantees to wasmtime (memory isolation, controlled imports, capability-based WASI). It has been audited twice for safety-critical use. All `WasmIsolationPolicy` controls transfer directly — memory limits, timeout enforcement, filesystem/network gating, host call allowlists. Fuel metering is more deterministic than epoch interruption. Encryption at rest (via agentzero-storage) and TLS for all network communication remain unchanged. Binary size is a goal but never at the expense of security.

**Constrained devices:** wasmi is pure Rust with `no_std` + alloc support. No JIT, mmap, signals, or threads required. Designed for embedded environments (used in Substrate/Polkadot blockchain execution).

## Critical Files

| File | Role |
|------|------|
| `crates/agentzero-plugins/Cargo.toml` | Dependency declarations, feature gates |
| `crates/agentzero-plugins/src/wasm.rs` | Core runtime (1696 lines) — main change |
| `crates/agentzero-infra/Cargo.toml` | Feature propagation |
| `crates/agentzero-infra/src/tools/wasm_bridge.rs` | Consumer of runtime (comment-only change) |
| `crates/agentzero-cli/Cargo.toml` | Feature propagation |
| `bin/agentzero/Cargo.toml` | Feature propagation |
| `Cargo.toml` (workspace root) | Workspace dep declarations |
| `specs/SPRINT.md` | Sprint tracking |

## Unchanged (no modifications needed)

- Plugin SDK (`crates/agentzero-plugin-sdk/`) — compiles to `wasm32-wasip1`, unaffected
- Plugin manifest format, discovery, packaging, registry (`package.rs`)
- Plugin watcher (`watcher.rs`)
- Process plugins (`infra/src/tools/plugin.rs`)
- MCP tools (`infra/src/tools/mcp.rs`)
- `wasm_bridge.rs` logic (only a doc comment changes)

---

## Phase 1: Cargo.toml Changes (5 files)

### 1a. Workspace root `Cargo.toml` — add wasmi workspace deps

```toml
# Add to [workspace.dependencies]:
wasmi = "0.40"
wasmi_wasi = "0.40"
```

Version note: verify latest stable wasmi on crates.io at implementation time. Use matching wasmi/wasmi_wasi versions.

### 1b. `crates/agentzero-plugins/Cargo.toml`

**Current:**
```toml
[features]
default = ["wasm-runtime"]
wasm-runtime = ["dep:wasmtime", "dep:wasmtime-wasi", "dep:tracing"]

[dependencies]
wasmtime = { version = "42", optional = true }
wasmtime-wasi = { version = "42", optional = true }
```

**Target:**
```toml
[features]
default = ["wasm-runtime"]
wasm-runtime = ["dep:wasmi", "dep:wasmi_wasi", "dep:tracing"]
wasm-jit = ["dep:wasmtime", "dep:wasmtime-wasi", "wasm-runtime"]

[dependencies]
wasmi = { workspace = true, optional = true }
wasmi_wasi = { workspace = true, optional = true }
wasmtime = { version = "42", optional = true }
wasmtime-wasi = { version = "42", optional = true }
```

- `wasm-runtime` now pulls wasmi (lightweight, always-on)
- `wasm-jit` adds wasmtime on top (implies `wasm-runtime` so shared types are available)

### 1c. `crates/agentzero-infra/Cargo.toml`

Add `wasm-jit` feature:
```toml
[features]
wasm-plugins = ["dep:agentzero-plugins"]
wasm-jit = ["wasm-plugins", "agentzero-plugins/wasm-jit"]
```

### 1d. `crates/agentzero-cli/Cargo.toml`

Add `wasm-jit` feature:
```toml
wasm-jit = ["plugins", "agentzero-infra/wasm-jit"]
```

### 1e. `bin/agentzero/Cargo.toml`

Add `wasm-jit` feature (NOT in default — opt-in only):
```toml
wasm-jit = ["agentzero-cli/wasm-jit"]
```

---

## Phase 2: wasmi Backend in `wasm.rs`

### Architecture: `#[cfg]` dispatch (not trait abstraction)

Use compile-time `#[cfg]` to select between wasmi and wasmtime backends. This matches the existing pattern in `wasm.rs` (lines 146-966) and avoids runtime overhead from trait dispatch.

```
Lines 1-141:     Shared types (UNCHANGED)

#[cfg(all(feature = "wasm-runtime", not(feature = "wasm-jit")))]
mod runtime_impl { ... }   // NEW: wasmi backend (~500 lines)

#[cfg(feature = "wasm-jit")]
mod runtime_impl { ... }   // EXISTING: wasmtime backend (unchanged, re-gated)

#[cfg(not(feature = "wasm-runtime"))]
mod runtime_impl { ... }   // EXISTING: stub (unchanged)

#[cfg(all(test, feature = "wasm-runtime"))]
mod tests { ... }           // MOSTLY UNCHANGED (see Phase 4)
```

### wasmi API mapping (key differences from wasmtime)

| Concern | wasmtime (current) | wasmi (new) |
|---------|-------------------|-------------|
| Engine creation | `Engine::new(&config)?` (fallible) | `Engine::new(&config)` (infallible) |
| Module loading | `Module::from_file(&engine, path)` | `Module::new(&engine, &std::fs::read(path)?)` |
| WASI setup | `WasiCtxBuilder::new().build_p1()` | `wasmi_wasi::WasiCtxBuilder::new().build()` |
| WASI linking | `wasmtime_wasi::p1::add_to_linker_sync()` | `wasmi_wasi::add_wasi_snapshot_preview1_to_linker()` |
| Memory limits | `StoreLimitsBuilder::new().memory_size(n)` | `ResourceLimiter` trait impl on store data |
| Timeout | Epoch interruption + timer thread | Fuel metering (`store.set_fuel(n)`) |
| Instantiation | `linker.instantiate(&mut store, &module)` | `linker.instantiate(&mut store, &module)?.start(&mut store)?` |
| Host functions | `linker.func_wrap("az", "az_log", \|caller, ...| ...)` | Same API |
| Memory access | `memory.data(&store)` / `memory.data_mut(&mut store)` | Same API |
| Typed functions | `instance.get_typed_func::<P, R>(&mut store, name)` | Same API |
| Module imports | `module.imports()` with `.module()` / `.name()` | Same API |
| Module cache | AOT serialize/deserialize `.cwasm` files | No AOT — `ModuleCache` becomes no-op passthrough |

### Timeout: Fuel metering replaces epoch interruption

wasmi uses **fuel-based metering** instead of epoch interruption:
- Each WASM instruction consumes 1 unit of fuel
- When fuel runs out, execution traps with "out of fuel"
- No timer thread needed — simpler, more deterministic, works on bare-metal

```rust
const FUEL_PER_MS: u64 = 100_000; // ~100M instructions/sec = 100K/ms

fn compute_fuel_from_timeout(timeout_ms: u64) -> u64 {
    timeout_ms.saturating_mul(FUEL_PER_MS)
}
```

Error mapping: detect "out of fuel" in trap message and map to the same "plugin execution exceeded time limit" error string that tests assert on.

### Memory limits: `ResourceLimiter` trait

```rust
struct PluginLimiter { max_memory_bytes: usize }

impl wasmi::ResourceLimiter for PluginLimiter {
    fn memory_growing(&mut self, _current: usize, desired: usize, _max: Option<usize>) -> Result<bool, _> {
        Ok(desired <= self.max_memory_bytes)
    }
    fn table_growing(&mut self, _current: usize, _desired: usize, _max: Option<usize>) -> Result<bool, _> {
        Ok(true)
    }
}
```

### ModuleCache: no-op for wasmi

wasmi is an interpreter — no AOT compilation, no `.cwasm` files. `ModuleCache::load_or_compile()` simply reads and compiles from source each time. wasmi compilation is fast (parsing, no codegen), so this is acceptable.

```rust
pub struct ModuleCache;
impl ModuleCache {
    pub fn load_or_compile(engine: &wasmi::Engine, wasm_path: &Path, _expected_sha256: &str) -> anyhow::Result<wasmi::Module> {
        let bytes = std::fs::read(wasm_path)?;
        wasmi::Module::new(engine, &bytes).map_err(|e| anyhow!("failed to compile: {e}"))
    }
}
```

### WASI preopened directory (filesystem access)

wasmi_wasi uses `wasi-common` under the hood. The preopened directory API differs:

```rust
// wasmi_wasi approach:
use wasmi_wasi::sync::Dir;
let dir = Dir::open_ambient_dir(&options.workspace_root, wasmi_wasi::sync::ambient_authority())?;
wasi_builder.preopened_dir(dir, ".");
```

Verify exact API at implementation time — the `wasmi_wasi` crate's `WasiCtxBuilder` may have different method signatures than wasmtime_wasi.

---

## Phase 3: Re-gate Existing wasmtime Backend

Wrap the existing `runtime_impl` module (lines 146-892) in `#[cfg(feature = "wasm-jit")]` instead of `#[cfg(feature = "wasm-runtime")]`. No other changes to wasmtime code.

---

## Phase 4: Test Updates

### Existing tests (lines 972-1696)

Most tests use the public API (`WasmPluginRuntime::new()`, `execute()`, `execute_v2()`) and WAT fixtures. They compile and run against whichever backend is active. **No changes needed** except:

1. **Timeout tests** (`execute_times_out_long_running_module`, `v2_execute_times_out`): These assert `err.to_string().contains("exceeded time limit")`. Our wasmi fuel-exhaustion handler maps to the same message — tests pass unchanged.

2. **Memory limit test** (`execute_rejects_module_exceeding_memory_limit`): Tests that `(memory 40)` fails with 1MB limit. wasmi's `ResourceLimiter` rejects the memory grow at instantiation — should produce a similar error. Verify and adjust assertion if the error message differs.

3. **ModuleCache tests** (lines 1608-1695): These directly reference `wasmtime::Config` and `wasmtime::Engine`. Must be split:

```rust
#[cfg(all(test, feature = "wasm-jit"))]
mod cache_tests_wasmtime { /* existing 3 tests unchanged */ }

#[cfg(all(test, feature = "wasm-runtime", not(feature = "wasm-jit")))]
mod cache_tests_wasmi {
    // Test that ModuleCache::load_or_compile works (no .cwasm files)
    // Simpler: just verify compile-from-source succeeds
}
```

### New tests for wasmi backend

Per AGENTS.md Rule 1, add comprehensive tests:
- **Success path**: wasmi v1 execute round-trip, wasmi v2 execute round-trip (covered by existing tests running under wasmi)
- **Negative path**: fuel exhaustion produces correct error message
- **Edge case**: `ModuleCache` with wasmi (no AOT, just compile)

### CI matrix

Run tests with both backends:
```bash
cargo test -p agentzero-plugins                          # default (wasmi)
cargo test -p agentzero-plugins --features wasm-jit      # wasmtime JIT
```

---

## Phase 5: Plugin Warming (Pre-compilation Cache)

Currently, every `execute()` call creates a new Engine, compiles the module from disk, and tears it down. This is wasteful, especially on constrained devices.

**Add in-memory module caching to `WasmTool`:**

- At `WasmTool::from_manifest()` time: parse the `.wasm` file into `wasmi::Module`, wrap in `Arc<Module>`, store in the `WasmTool` struct
- At `execute()` time: reuse the pre-compiled `Module`, only create a new `Store` per call (cheap — just allocates the store state)
- Share a single `wasmi::Engine` across all plugins (Engine is thread-safe and cheap to clone)

**Changes to `wasm_bridge.rs`:**
```rust
pub struct WasmTool {
    name: &'static str,
    manifest: PluginManifest,
    wasm_path: PathBuf,
    policy: WasmIsolationPolicy,
    engine: wasmi::Engine,           // NEW: shared engine
    module: Arc<wasmi::Module>,      // NEW: pre-compiled module
}
```

This eliminates disk I/O and module parsing from the hot path. On ESP32, this is the difference between ~100ms and ~1ms per plugin call.

**Note:** This requires the wasmi types to be exposed from `agentzero-plugins` to `agentzero-infra`. Add a `WasmEngine` and `WasmModule` type alias in the public API, gated by `#[cfg(feature = "wasm-runtime")]`, so the bridge can hold onto them without depending on wasmi directly.

For the wasmtime (`wasm-jit`) backend, the same pattern applies — pre-compile with `Module::from_file()` at init, reuse at execution.

## Phase 6: Update `wasm_bridge.rs`

Comment change at line 17/88 plus structural changes for plugin warming (see Phase 5).

---

## Phase 7: Binary Slimming — TLS Backend Feature Gate

**Security note:** rustls remains the default TLS backend (memory-safe, no system dependencies, no OpenSSL CVEs). native-tls is offered as an opt-in alternative for users who already manage system OpenSSL and want smaller binaries. Encryption and TLS are non-negotiable at all layers.

### Changes to workspace root `Cargo.toml`:

```toml
# Change reqwest to not hardcode TLS:
reqwest = { version = "0.12", default-features = false, features = ["json", "stream"] }
```

### Feature propagation chain:

```toml
# bin/agentzero/Cargo.toml
default = ["memory-sqlite", "plugins", "gateway", "tui", "interactive", "tls-rustls"]
tls-rustls = ["agentzero-cli/tls-rustls"]    # DEFAULT — secure, no system deps
tls-native = ["agentzero-cli/tls-native"]    # Optional — requires system OpenSSL

# crates/agentzero-cli/Cargo.toml
tls-rustls = ["reqwest/rustls-tls"]
tls-native = ["reqwest/native-tls"]
```

Also align `tokio-tungstenite` TLS features with the same gate.

### Constrained build commands:

```bash
# Recommended (rustls, ~6MB):
cargo build --profile release-min --no-default-features --features memory-sqlite,plugins,tls-rustls

# Optional (native-tls, ~5MB, requires system OpenSSL):
cargo build --profile release-min --no-default-features --features memory-sqlite,plugins,tls-native
```

### Files affected:
- `Cargo.toml` (workspace root) — decouple TLS from reqwest defaults
- `bin/agentzero/Cargo.toml` — add tls-rustls/tls-native features
- `crates/agentzero-cli/Cargo.toml` — propagate TLS features

## Phase 8: Update `specs/SPRINT.md`

Add new sprint section:

```markdown
## Sprint 20.7: wasmi Runtime Migration

**Goal:** Replace wasmtime with wasmi as the default WASM runtime for plugin execution. Enables WASM plugins on constrained devices (ESP32, Raspberry Pi). Keep wasmtime as optional `wasm-jit` feature. Add plugin warming for fast execution.

**Branch:** `refactor/crate-consolidation`

### Phase 1: Cargo.toml Feature Restructuring
- [ ] Add wasmi/wasmi_wasi workspace deps
- [ ] Restructure agentzero-plugins features (wasm-runtime → wasmi, wasm-jit → wasmtime)
- [ ] Add wasm-jit feature propagation through infra → cli → binary

### Phase 2: wasmi Backend Implementation
- [ ] Implement wasmi runtime_impl in wasm.rs (fuel metering, ResourceLimiter, WASI)
- [ ] Implement wasmi ModuleCache (no-op passthrough)
- [ ] Register az_log and az_env_get host functions for wasmi

### Phase 3: Re-gate wasmtime Backend
- [ ] Move existing runtime_impl behind #[cfg(feature = "wasm-jit")]

### Phase 4: Plugin Warming
- [ ] Pre-compile modules at WasmTool::from_manifest() time
- [ ] Share Engine across plugins, store Arc<Module> per plugin
- [ ] Expose WasmEngine/WasmModule type aliases from agentzero-plugins

### Phase 5: TLS Backend Feature Gate
- [ ] Remove TLS from reqwest workspace default features
- [ ] Add tls-rustls / tls-native features propagated through binary → cli
- [ ] Align tokio-tungstenite TLS features
- [ ] Verify builds compile with each TLS backend

### Phase 6: Tests & Verification
- [ ] Verify all existing tests pass with wasmi backend
- [ ] Split ModuleCache tests by feature gate
- [ ] Add wasmi-specific fuel exhaustion tests
- [ ] Add plugin warming tests (pre-compiled module reuse)
- [ ] Verify all tests pass with wasm-jit feature
- [ ] cargo fmt, clippy, test --workspace all pass
- [ ] Binary size: minimal+plugins+tls-rustls+release-min < 7MB
- [ ] Binary size: minimal+plugins+tls-native+release-min < 6MB (optional)
```

---

## Sprint 20.8: Dependency Slimming (Future)

**Goal:** Further reduce binary size and compile times for constrained deployments.

### Phase 1: Lightweight HTTP Client
- [ ] Add ureq as alternative HTTP client behind `http-minimal` feature
- [ ] Replace reqwest in agentzero-tools (web_fetch, http_request, web_search, composio, pushover)
- [ ] Keep reqwest for providers (SSE streaming) and cli/local/pull.rs (streaming)
- [ ] Remove reqwest from agentzero-auth (currently unused)

### Phase 2: System SQLite Option
- [ ] Add `sqlite-system` feature that disables rusqlite `bundled`
- [ ] Saves ~1-2MB by linking system libsqlite3 instead of compiling from C source
- [ ] Constrained builds: `--features sqlite-system` (requires libsqlite3-dev on host)

### Phase 3: Dependency Audit
- [ ] Profile remaining large deps for optimization opportunities
- [ ] Evaluate lighter alternatives where security is not compromised
- [ ] Document recommended feature sets for each deployment target (ESP32, RPi, server, desktop)

---

## Verification

1. `cargo test -p agentzero-plugins` — all tests pass with wasmi
2. `cargo test -p agentzero-plugins --features wasm-jit` — all tests pass with wasmtime
3. `cargo test --workspace` — full workspace passes
4. `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings
5. `cargo clippy --workspace --all-targets --features wasm-jit -- -D warnings` — zero warnings
6. `cargo fmt --all -- --check` — formatted
7. Build without `plugins` feature: `cargo build -p agentzero --no-default-features --features memory-sqlite` — compiles (stub runtime)
8. Binary size: `cargo build --release` before/after — confirm reduction from 19MB baseline
9. Binary size: `cargo build --profile release-min --no-default-features --features memory-sqlite,plugins,tls-rustls` — target <7MB
10. Binary size: `cargo build --profile release-min --no-default-features --features memory-sqlite,plugins,tls-native` — target <6MB (optional)
11. TLS: `cargo build --no-default-features --features memory-sqlite,tls-native` — compiles with native-tls
12. TLS: `cargo build --no-default-features --features memory-sqlite,tls-rustls` — compiles with rustls

## Future Work (out of scope for this sprint)

- **reqwest → ureq**: Add ureq as a lightweight alternative HTTP client for tools that don't need streaming (~modest savings since providers still need reqwest)
- **rusqlite system vs bundled**: Feature-gate `rusqlite/bundled` so constrained builds can link system sqlite (~1-2MB saving)
- Note: ratatui is already feature-gated behind `tui`, gateway behind `gateway` — both excluded from minimal builds
- Note: channels are a **core requirement** (always compiled) — all channels must be available for cross-channel message handling

## Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| wasmi_wasi API differs from wasmtime_wasi for preopened dirs | Medium | Verify during implementation; may need adapter code |
| Fuel metering is not exact time correlation | Low | Use conservative FUEL_PER_MS; document approximation |
| wasmi instantiate+start semantics differ | Medium | Use `instantiate()?.start()?` pattern; test thoroughly |
| Memory limit error messages differ between runtimes | Low | Adjust test assertions per backend via cfg |
