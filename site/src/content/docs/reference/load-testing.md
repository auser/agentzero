---
title: Load Testing
description: Pure-Rust load harness for the AgentZero gateway, plus baseline measurements.
---

A pure-Rust load harness for the gateway lives in the `agentzero-gateway` crate at `tests/load_baseline.rs`. It spawns the gateway in-process, hammers the cheapest unauthenticated endpoints, and reports throughput, error count, and latency percentiles.

The harness is gated behind `#[ignore]` so it does not run during `cargo test`. Invoke it explicitly when you want a baseline.

## Running

```bash
# Default: 10 seconds, 64 concurrent clients, port 18800
cargo test --release -p agentzero-gateway --test load_baseline -- --ignored --nocapture load_baseline

# Custom configuration via env vars
AZ_LOAD_DURATION_SECS=30 \
AZ_LOAD_CONCURRENCY=128 \
AZ_LOAD_PORT=18800 \
cargo test --release -p agentzero-gateway --test load_baseline -- --ignored --nocapture load_baseline
```

Always pass `--release`. A debug build will dramatically under-report what production deployments can do.

## What it measures

For each of `GET /health/live`, `GET /health`, `GET /metrics`:

- **Total requests** issued during the run window
- **Effective requests/second** (total / wall-clock seconds)
- **Error count** (any non-2xx response or transport error)
- **p50 / p95 / p99 / max** latency in milliseconds

The harness uses a single `reqwest::Client` with connection pooling so we measure server-side throughput, not TCP open or TLS handshake cost.

## Baseline measurements

Captured 2026-04-07 on a development MacBook Pro (Apple Silicon, 8 cores). Five-second windows, `--no-auth`, rate limiting disabled, `--release`.

### 32 concurrent clients

| Endpoint          | Reqs    | Errors | RPS    | p50      | p95      | p99      | Max      |
|-------------------|--------:|-------:|-------:|---------:|---------:|---------:|---------:|
| GET /health/live  | 340,093 | 0      | 68,010 | 0.45 ms  | 0.64 ms  | 0.78 ms  | 7.17 ms  |
| GET /health       | 349,942 | 0      | 69,980 | 0.45 ms  | 0.60 ms  | 0.69 ms  | 1.35 ms  |
| GET /metrics      | 171,173 | 0      | 34,231 | 0.91 ms  | 1.34 ms  | 1.55 ms  | 49.25 ms |

### 256 concurrent clients

| Endpoint          | Reqs    | Errors | RPS    | p50      | p95      | p99      | Max      |
|-------------------|--------:|-------:|-------:|---------:|---------:|---------:|---------:|
| GET /health/live  | 339,871 | 0      | 67,934 | 3.58 ms  | 4.16 ms  | 8.20 ms  | 44.88 ms |
| GET /health       | 342,826 | 0      | 68,521 | 3.66 ms  | 4.17 ms  | 5.48 ms  | 17.11 ms |
| GET /metrics      | 169,060 | 0      | 33,780 | 7.36 ms  | 11.66 ms | 13.93 ms | 58.41 ms |

### What this tells us

- **Throughput cap on this hardware: ~68,000 RPS** for cheap endpoints. Going from 32 to 256 concurrent clients (8x more) leaves throughput nearly identical, which means the gateway is **CPU-bound, not concurrency-limited**.
- **Graceful degradation under contention.** With 8x the concurrent connections, latency increases proportionally (p99 from 0.78 ms to 8.20 ms) but the error count stays at zero. No timeouts, no dropped connections, no 5xx responses.
- **`/metrics` is roughly 2x heavier** than `/health/live` because the Prometheus exporter walks the metrics registry on every call. This is the expected cost.
- **Tail latency stays controlled.** Even under 256-way contention, p99 is well under 10 ms and the worst observed call is under 60 ms.

## Why these endpoints

`/health/live` is the cheapest call possible: no auth, single tokio task spawn, ~10-byte response. It establishes the raw routing/serialization throughput ceiling. `/health` is similar but slightly larger. `/metrics` is the only baseline endpoint that does meaningful work — it's a useful proxy for "the gateway is doing more than memcpy".

We deliberately do **not** load-test `/api/chat`, `/v1/runs`, or any endpoint that calls into an LLM provider. Those numbers would be dominated by provider latency, not gateway behavior. For agent-loop benchmarks, configure a fake provider via `agentzero-testkit` and target a separate harness.

## Capacity planning rules of thumb

- One process on commodity hardware (8 cores / 16 GB) handles **~50–70k RPS of cheap traffic** with sub-10 ms p99.
- For real workloads dominated by LLM provider calls, the gateway is never the bottleneck — provider latency (hundreds of milliseconds to seconds) dominates by orders of magnitude.
- If you need >70k RPS of *cheap* traffic (e.g., aggressive Prometheus scraping), run multiple gateway instances behind a load balancer rather than vertically scaling.
- The default `MiddlewareConfig` rate limit is **600 requests / 60 seconds = 10 RPS** per global window. That is **not** a production setting — it's a safe default. Tune `rate_limit_max` and `rate_limit_per_identity` for your deployment in `agentzero.toml` under `[gateway]`.

## What to do if the baseline drops

If a future run shows the cheap-endpoint RPS dropping by more than 20% on the same hardware:

1. `git bisect` between the last known-good run and the current commit, narrowing on commits that touched `agentzero-gateway`, `axum`, `tower`, `hyper`, or `tokio`.
2. Profile with [`cargo flamegraph`](https://github.com/flamegraph-rs/flamegraph) or [`samply`](https://github.com/mstange/samply) to find the new hot spot.
3. Check the middleware stack — newly added layers are the most common cause of latency regressions.
4. Check for accidental allocations in hot paths: `String::new()` in loops, `format!` where `write!` would do, etc.

## Known gaps

- The harness does not yet exercise authenticated endpoints. Adding bearer-token support is straightforward when needed.
- WebSocket and SSE streaming have a different load profile and warrant their own harness.
- The harness does not run in CI. It is intentionally local-only because the numbers depend on hardware and CI runners are too noisy to produce useful regression thresholds. If you want CI tracking, capture *relative* numbers between runs on the same runner, not absolute thresholds.
