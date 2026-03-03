---
title: Benchmarks
description: Reproducible benchmark commands and baseline outputs for agentzero.
---

This document tracks reproducible benchmark commands and baseline outputs.

## Build Profiles

AgentZero ships two build variants:

| Variant | Profile | Features | Binary Size (macOS arm64) | Crate Count |
|---|---|---|---|---|
| **default** | `release` | All (TUI, WASM plugins, gateway, interactive) | ~18 MB | ~625 |
| **minimal** | `release-min` | Core only (memory-sqlite) | ~5.2 MB | ~262 |

Build commands:

```bash
# Default build
cargo build -p agentzero --release

# Minimal build (size-optimized: fat LTO + opt-level z)
cargo build -p agentzero --profile release-min --no-default-features --features minimal
```

## Prerequisites

- Build release binary once:

```bash
cargo build -p agentzero --release
```

- For default single-message benchmark (`agent -m ...`), set:

```bash
export OPENAI_API_KEY="sk-..."
```

## Command Set

Run criterion core-loop benchmarks (offline, no external provider call):

```bash
cargo bench -p agentzero-bench --bench core_loop
```

Criterion writes reports under `target/criterion/` with per-benchmark latency stats.

Run CLI startup benchmark script:

```bash
scripts/bench-cli-startup.sh --iterations 20 --command "--help"
```

Run single-message benchmark script:

```bash
scripts/bench-single-message.sh --iterations 10 --message "hello benchmark"
```

If you want a provider-free path for script validation:

```bash
scripts/bench-single-message.sh --iterations 10 --command "status --json"
```

## Baseline Template

Record your local measurements in this table after each benchmark run:

| Date (UTC) | Commit | Environment | Variant | Benchmark | Iterations | min_ms | avg_ms | max_ms | Notes |
|---|---|---|---|---|---:|---:|---:|---:|---|
| 2026-03-03 | `HEAD` | macOS arm64 (M-series) | default | cli_startup | 20 | 19.05 | 42.90 | 443.08 | --help |
| 2026-03-03 | `HEAD` | macOS arm64 (M-series) | minimal | cli_startup | 20 | 21.18 | 41.20 | 307.36 | --help |
