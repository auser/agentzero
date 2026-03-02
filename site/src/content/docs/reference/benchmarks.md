---
title: Benchmarks
description: Reproducible benchmark commands and baseline outputs for agentzero.
---

This document tracks reproducible benchmark commands and baseline outputs.

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

| Date (UTC) | Commit | Environment | Benchmark | Iterations | min_ms | avg_ms | max_ms | Notes |
|---|---|---|---|---:|---:|---:|---:|---|
| YYYY-MM-DD | `<sha>` | macOS/Linux + CPU | cli_startup | 20 |  |  |  | |
| YYYY-MM-DD | `<sha>` | macOS/Linux + CPU | single_message | 10 |  |  |  | |
| YYYY-MM-DD | `<sha>` | macOS/Linux + CPU | core_loop_single_turn | from criterion |  |  |  | from target/criterion report |
| YYYY-MM-DD | `<sha>` | macOS/Linux + CPU | core_loop_tool_turn | from criterion |  |  |  | from target/criterion report |
