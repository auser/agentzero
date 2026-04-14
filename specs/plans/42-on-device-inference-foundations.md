# Plan 42: On-Device Inference Foundations

## Context

AgentZero already ships Candle and llama.cpp providers behind feature flags ([crates/agentzero-providers/Cargo.toml](../../crates/agentzero-providers/Cargo.toml)) and has open work on local inference (`project_candle_gpu.md`), embedded-binary size (target 10.1 MB → 5–8 MB per `project_embedded_size_reduction.md`), and retrieval quality (Plan 41). After auditing patterns from a production Rust on-device-AI runtime that ships LLM/ASR/TTS to iOS/Android/Flutter/Unity, five concrete improvements would close gaps we already have. This plan adopts all five as one cohesive sprint.

The current state, established by direct exploration of the codebase:

- **Provider trait** lives at [crates/agentzero-core/src/types.rs:1109](../../crates/agentzero-core/src/types.rs#L1109). `BuiltinProvider` ([builtin.rs:260](../../crates/agentzero-providers/src/builtin.rs#L260)) and `CandleProvider` ([candle_provider.rs:603](../../crates/agentzero-providers/src/candle_provider.rs#L603)) both implement it directly. Chat templating (`ChatTemplate` enum + `format_prompt`) is already shared in [local_tools.rs:216](../../crates/agentzero-providers/src/local_tools.rs#L216), but **sampling and streaming are duplicated per provider** (`LlamaSampler` in builtin, `LogitsProcessor` in Candle).
- **Device selection** is config-driven only — `CandleConfig.device: String` ("auto"|"metal"|"cuda"|"cpu") consumed by `auto_detect_device()` at [candle_provider.rs:140](../../crates/agentzero-providers/src/candle_provider.rs#L140). No capability struct, no `sysinfo`, no NPU detection. The `agentzero-tools` hardware surface ([hardware.rs](../../crates/agentzero-tools/src/hardware.rs)) returns hardcoded `sim-stm32` / `sim-rpi` boards and never inspects the host.
- **Plugins** use a directory + `manifest.json` + optional Ed25519 signature ([package.rs:42](../../crates/agentzero-plugins/src/package.rs#L42)), API version 2. Models are fetched ad-hoc via `hf-hub` in `model_manager.rs`. There is no signed model bundle format.
- **Feature matrix:** the workspace has zero `compile_error!` guards. The only `build.rs` is `agentzero-config-ui` (a UI placeholder). Several invalid feature combinations compile silently today: `candle-cuda` on macOS, `candle-metal` off-Apple, `candle` or `local-model` on `wasm32`, both encrypted+plain storage.
- **Plan 41** (Tantivy + HNSW retrieval) is independent of all of this — no overlap, no conflict.

The five phases below are ordered by dependency: Phase A is the foundation (capabilities), Phase B is a one-day quick win (compile guards), Phases C–E are progressively larger refactors. Each phase can ship as its own PR; later phases benefit from but do not strictly require earlier ones.

---

## Phase A: `agentzero-core::device` capability detection (HIGH)

Runtime hardware capability struct that backend selection — and tools — can query, replacing today's "string config + cfg-flags" approach. Establishes a typed surface (`HardwareCapabilities`, `GpuType`, `NpuType`, `DetectionConfidence`) that every later phase consumes.

- [ ] **`agentzero-core::device::types`** — `HardwareCapabilities { cpu_cores, total_memory_mb, gpu: GpuType, npu: NpuType, thermal: ThermalState, battery_pct: Option<u8>, memory_confidence: DetectionConfidence }`. Enums: `GpuType { Metal, Cuda, Vulkan, None }`, `NpuType { CoreML, Nnapi, None }`, `ThermalState { Nominal, Fair, Serious, Critical }`, `DetectionConfidence { High, Medium, Low }`. New file `crates/agentzero-core/src/device/types.rs`
- [ ] **`device::common`** — `detect_cpu()`, `detect_memory()` via `sysinfo`. Cross-platform. New file `crates/agentzero-core/src/device/common.rs`
- [ ] **`device::apple`** — `#[cfg(any(target_os="macos", target_os="ios"))]` Metal probe (Metal framework presence), CoreML probe (system version check). New file `crates/agentzero-core/src/device/apple.rs`
- [ ] **`device::linux`** — `#[cfg(target_os="linux")]` CUDA probe via `nvidia-smi` presence + `/proc/driver/nvidia` (no CUDA link required). New file `crates/agentzero-core/src/device/linux.rs`
- [ ] **`device::android`** — `#[cfg(target_os="android")]` NNAPI stub returning `Low` confidence; real probe deferred. New file `crates/agentzero-core/src/device/android.rs`
- [ ] **`device::detect()`** — Top-level entry that composes the platform-specific detectors and returns a `HardwareCapabilities`. New file `crates/agentzero-core/src/device/mod.rs`
- [ ] **`Cargo.toml`** — Add `sysinfo = "0.32"` to `agentzero-core` deps. New top-level mod export in [agentzero-core/src/lib.rs](../../crates/agentzero-core/src/lib.rs).
- [ ] **Wire into Candle backend selection** — `auto_detect_device()` at [candle_provider.rs:140](../../crates/agentzero-providers/src/candle_provider.rs#L140) consults `agentzero_core::device::detect()` first; falls back to existing cfg-based behavior when confidence is `Low`. Selection observable from logs.
- [ ] **Wire into hardware tool surface** — `discover_boards()` at [crates/agentzero-tools/src/hardware.rs](../../crates/agentzero-tools/src/hardware.rs) prepends the live host as a real entry from `device::detect()`, alongside the existing simulator entries.
- [ ] **Tests** — `cargo test -p agentzero-core device::` asserts non-zero CPU + memory on every supported target; on dev mac asserts `gpu == Metal && npu == CoreML && confidence == High`; on Linux CI asserts `gpu` is `None` or `Cuda` with non-`Low` confidence. Existing `cargo test -p agentzero-providers candle` continues to pass with identical device selections.

---

## Phase B: Compile-time feature guards (MEDIUM)

Catch invalid feature combinations at `cargo check` time with actionable messages, instead of cryptic link or runtime errors. Smallest phase, ~50 lines of `compile_error!`. Independent of Phase A and shippable first if needed.

- [ ] **`agentzero-providers/src/lib.rs` guard block** — Top-of-file `compile_error!` blocks for:
  - `candle-cuda` on `target_os = "macos"` → "Use `candle-metal` instead; CUDA is unavailable on Apple Silicon."
  - `candle-metal` on non-Apple targets → "Use `candle-cuda` (NVIDIA) or plain `candle` (CPU)."
  - `candle-cuda` + `candle-metal` simultaneously → "These backends are mutually exclusive; pick one."
  - `candle` or `local-model` with `target_arch = "wasm32"` → "Local inference is not supported on wasm32; use a remote provider."
- [ ] **`agentzero-storage/src/lib.rs` guard block** — `storage-encrypted` + `storage-plain` simultaneously → "Pick exactly one storage backend."
- [ ] **`bin/agentzero/src/main.rs` mirror block** — Same provider guards at the binary entry point, since the binary re-exposes the feature matrix and is the most-likely-wrong-flags entry point.
- [ ] **Multi-line, actionable messages** — Each `compile_error!` includes both the *reason* and the *fix*, never a terse one-liner.
- [ ] **Tests** — `cargo check --target x86_64-apple-darwin --features candle-cuda` fails with the readable Apple/CUDA message. `cargo check --target x86_64-unknown-linux-gnu --features candle-metal` fails. `cargo check --target wasm32-unknown-unknown -p agentzero-providers --features candle` fails. Default-features `cargo check` passes on every supported target (no false positives).

---

## Phase C: `LocalLlm` trait + shared `GenerationLoop` (HIGH) ✅

**Revised from the original tensor-level `InferenceBackend` plan.** After reading the code, the tensor-level trait (`HashMap<String, ArrayD<f32>>`) didn't fit — Candle uses `candle_core::Tensor` + `ModelWeights::forward()` while llama.cpp uses `LlamaBatch` + `LlamaSampler`. The real duplication was the generation loop (tokenize → feed prompt → sample → EOS/repetition → decode → stream), not tensor conversion. A `LocalLlm` trait with 5 simple methods + a shared `GenerationLoop` eliminates ~300 lines of duplicated code and gives the builtin provider real streaming for free.

- [x] **`local_llm.rs`** — `LocalLlm` trait (`tokenize`, `feed_prompt`, `step`, `decode_token`, `is_eos`) + `GenerationLoop` struct. 5 mock-based tests.
- [x] **`CandleLlm`** — Implements `LocalLlm`. Replaced `generate()` + `generate_streaming()` with unified `run_inference()`.
- [x] **`LlamaCppLlm`** — Implements `LocalLlm`. Replaced `generate()` with `run_inference()`. Real token-by-token streaming.
- [x] **Pre-existing bug fix** — `ChatTemplate::detect()` cfg gate narrowed from `any(candle, local-model)` to `candle` only.
- [x] **Tests** — 496 workspace, 303 with candle. 0 clippy warnings. 5 new mock tests.

---

## Phase D: `.azb` model bundle format (MEDIUM) ✅

A signed, manifest-bearing model+config archive that distributes through the same channels as plugins. Replaces ad-hoc `hf-hub` directory layouts with one verifiable file. Largest phase by LoC, but the most isolated — it adds capability without changing existing flows.

- [ ] **`agentzero-providers::bundle`** — New module:
  ```rust
  pub struct AzBundle { manifest: BundleManifest, files: HashMap<String, Vec<u8>> }
  pub struct BundleManifest {
      pub model_id: String,
      pub version: String,
      pub target: String,                // e.g., "any", "aarch64-darwin"
      pub backend: RuntimeType,          // from Phase C
      pub min_runtime_api: u32,
      pub files: Vec<BundleFile>,        // path + sha256 + role (model|tokenizer|config)
      pub signature: Option<String>,     // Ed25519 hex
      pub signing_key_id: Option<String>,
  }
  ```
  Format: tar + zstd, identical to plugin packages so the same tooling validates both. New file `crates/agentzero-providers/src/bundle.rs`.
- [ ] **`bundle_loader`** — `load(path) -> AzBundle`, `verify_signature()`, `extract_to(dir)`. New file `crates/agentzero-providers/src/bundle_loader.rs`.
- [ ] **Shared signing helper** — Extract Ed25519 verification from [crates/agentzero-plugins/src/package.rs:42](../../crates/agentzero-plugins/src/package.rs#L42) into `agentzero-core::signing` so both `PluginManifest` and `BundleManifest` use the same code path. Single cross-crate touch in this phase.
- [ ] **`model_manager` integration** — [crates/agentzero-providers/src/model_manager.rs](../../crates/agentzero-providers/src/model_manager.rs) gains a `load_bundle(path)` path alongside the existing `hf-hub` flow. Bundles take precedence; `hf-hub` remains the fallback for un-bundled models.
- [ ] **CLI subcommands** — `agentzero bundle create <model-dir>`, `agentzero bundle verify <file.azb>`, `agentzero bundle install <file.azb>`. New file `crates/agentzero-cli/src/bundle.rs`. Registered in [crates/agentzero-infra/src/tools/mod.rs](../../crates/agentzero-infra/src/tools/mod.rs).
- [ ] **`min_runtime_api` enforcement** — A bundle declaring `min_runtime_api: 999` is rejected with a clear "bundle requires newer AgentZero" error, mirroring plugin loader behavior.
- [ ] **Tests** — `cargo test -p agentzero-providers bundle::` round-trips create → write → load → verify. Unsigned bundle loads with `SignatureStatus::Unsigned`. Tampered bundle returns `Err(BadSignature)`. End-to-end: `agentzero bundle create ./test-model && agentzero bundle verify out.azb`.

---

### Acceptance Criteria

- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — 0 warnings (per `clippy_zero_tolerance`)
- [ ] `cargo test --workspace` — all tests pass
- [ ] `cargo test -p agentzero-core device::` — capability detection unit tests green
- [ ] `cargo check --target x86_64-apple-darwin --features candle-cuda` fails with the readable guard message
- [ ] `cargo check --target wasm32-unknown-unknown -p agentzero-providers --features candle` fails with the wasm32 guard message
- [ ] `cargo run -p agentzero --features candle -- chat 'hello'` produces output (Phase C smoke test)
- [ ] `cargo build -p agentzero-providers --no-default-features` succeeds with zero C++ tooling installed
- [ ] `agentzero bundle create ./test-model && agentzero bundle verify out.azb` succeeds end-to-end (Phase D)
- [ ] Existing provider behavior unchanged (`auto_detect_device()` selects the same backend as before; chat output identical)

### Out of Scope

- Adopting an `Envelope` IR. Our `Provider` trait already serves this role.
- Vendoring llama.cpp source under `vendor/`. `llama-cpp-2` from crates.io stays the source of truth.
- ASR/TTS modules.
- Duplicating Plan 41 (Tantivy + HNSW retrieval). These five phases are orthogonal to retrieval and can ship in parallel with it.
