# Plan: Candle Metal GPU Acceleration

## Context

The Candle local LLM provider was CPU-only because `candle-metal-kernels` was alpha on crates.io. It's now stable at 0.10.1. This plan bumps candle from 0.9 to 0.10, enables the Metal feature gate, and wires up GPU device selection.

## Changes

1. **Workspace Cargo.toml** — Bump `candle-core`, `candle-nn`, `candle-transformers` from 0.9 to =0.10.0
2. **providers/Cargo.toml** — Uncomment `candle-metal` and `candle-cuda` features
3. **infra/cli/bin Cargo.toml** — Propagate `candle-metal` and `candle-cuda` up the crate chain
4. **candle_provider.rs** — Rewrite `select_device()` with Metal/CUDA/auto/CPU support
5. **candle_embedding.rs** — Use `select_device("auto")` instead of hardcoded CPU
6. **Site docs** — Update providers guide, installation, config reference

## Build

```bash
# Apple Silicon GPU
cargo build --release --features candle-metal

# CPU only
cargo build --release --features candle

# NVIDIA GPU
cargo build --release --features candle-cuda
```
