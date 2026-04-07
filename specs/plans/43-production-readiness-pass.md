# Plan 43: Production Readiness Pass — Load Testing, Codegen Safety, Unwrap Audit

## Context

After Sprints 82 (retrieval) and 83 (device detection + feature guards) shipped, I audited what's still gating "production-ready" status for AgentZero. Three concrete items emerged as the highest-leverage next steps. None of them are large refactors — each is a focused operational hardening pass.

The ordering below is by risk-to-confidence ratio: load testing surfaces unknown failure modes (highest unknown), codegen safety closes a known dangerous capability (highest known), and the unwrap audit is cleanup with measurable result.

---

## Phase A: Pure-Rust Gateway Load Harness (HIGH)

Build a load-test harness that hammers the gateway from a known starting point and reports throughput, latency percentiles (p50/p95/p99), and the breaking point where errors begin. Pure Rust so it ships with the workspace and runs in CI without external tools (`wrk`, `vegeta`, `k6` all forbidden by `AgentZero stays standalone`).

**Why now:** We have no idea how the gateway behaves under load. Every operational confidence claim ("can handle X RPS", "p99 latency is Y ms") is currently a guess.

**Scope:**

- [ ] **`crates/agentzero-gateway/benches/load.rs`** — New harness that:
  - Spawns the gateway in-process via `tokio::task::spawn` (using existing `serve_plain` path with `--no-auth` so no test fixtures)
  - Hammers `/health/live` (cheapest endpoint, no auth) to establish raw throughput baseline
  - Hammers `/health` and `/metrics` for slightly heavier baselines
  - Hammers `POST /pair` to exercise a write path that doesn't require auth
  - Configurable via env: `AZ_LOAD_DURATION_SECS`, `AZ_LOAD_CONCURRENCY`, `AZ_LOAD_ENDPOINT`
  - Reports per-endpoint: total requests, RPS, error count, p50/p95/p99 latency in milliseconds, max latency
- [ ] **`hdrhistogram` workspace dep** — for accurate percentile tracking. Optional to keep light, but it's the right choice for the math.
- [ ] **Ignored by default** — `#[ignore]` at the test level so `cargo test` doesn't run it; invoked explicitly via `cargo test --release -p agentzero-gateway -- --ignored load_baseline`
- [ ] **Documented in runbook** — Add `docs/runbooks/load-testing.md` with the invocation command, what each baseline measures, and how to interpret the output
- [ ] **Capture findings** — Run the harness on the dev machine, capture the numbers, write them down. The point of this work isn't "ship a harness", it's "know our breaking point". The runbook captures the actual measured baseline.

**Acceptance:**
- `cargo test --release -p agentzero-gateway -- --ignored load_baseline` runs end-to-end without panicking
- Output reports RPS and percentiles for at least three endpoints
- We have a documented "this is our baseline RPS / p99 / breaking point" entry in the runbook

---

## Phase B: Codegen Tool Safety Rails (HIGH)

The dynamic Codegen strategy (Sprint 80) compiles LLM-generated Rust to WASM and hot-loads it. Today there are no resource caps and no kill-switch — the LLM can generate code that loops forever, allocates unbounded memory, or shells out (within the WASM sandbox limits). Production deployments cannot ship this without bounds.

**Why now:** This is the most powerful and most dangerous feature in the codebase. Even with WASM sandboxing, an unbounded execution can DOS the orchestrator.

**Scope:**

- [ ] **Per-execution resource caps** — When the codegen tool runs a generated WASM module via `WasmPluginRuntime::execute_v2_precompiled`, enforce:
  - Wall-clock timeout (configurable, default 30 seconds)
  - Fuel/instruction limit (already supported by wasmi — wire it up if not already)
  - Memory cap (already a sandbox property — verify and document)
- [ ] **Audit log entry per generated tool** — Every successful codegen compile + execute writes a structured audit event with: tool name, source SHA-256, timestamp, who triggered it (agent ID), success/failure, output truncated to 1KB
- [ ] **Kill-switch flag** — `[runtime] codegen_enabled = true|false` in the TOML config (default `true` for backward compat, recommend `false` for production deployments). When `false`, `tool_create(strategy_hint: "codegen", ...)` returns an explicit error message
- [ ] **Gateway endpoint** — `POST /v1/runtime/codegen-disable` (admin-only) flips the runtime kill-switch without requiring a config reload + restart
- [ ] **Tests** — Codegen with timeout exceeded errors out cleanly, kill-switch off blocks creation, audit log records every execution with correct fields
- [ ] **Documentation** — Section in `docs/runbooks/incident-response.md` on how to disable codegen during an incident

**Acceptance:**
- Generated WASM that infinite-loops is killed within the configured timeout
- Setting `codegen_enabled = false` blocks new codegen tools at creation time
- `POST /v1/runtime/codegen-disable` flips the runtime flag
- Audit log shows one entry per generated-tool execution

---

## Phase C: `.unwrap()` Audit in Production Code (MEDIUM)

Per the `no_unwrap_in_production` feedback policy, audit non-test code paths for `.unwrap()` calls and replace them with `.expect("descriptive message")` or proper error propagation.

**Why now:** Cleanup, clear feedback policy, low risk. Ideal companion to the larger phases above.

**Scope:**

- [ ] **Audit script** — One-liner to find every `.unwrap()` outside `#[cfg(test)]` blocks and `tests/` directories. Output goes into a triage file.
- [ ] **Categorize** — Each hit is one of:
  - **Safe by construction** (e.g., `Mutex` lock that can't panic in this context) → convert to `.expect()` with a reason
  - **Should propagate** → return `Result` instead, bubble the error
  - **Genuinely impossible** (e.g., regex compiled from a literal at startup) → `.expect()` with the reason
- [ ] **Fix the high-leverage hits first** — request/response handling paths, gateway handlers, anywhere a panic translates to a 500 + dropped client connection. Defer cosmetic fixes.
- [ ] **Documented count** — Before/after counts in the commit message. Goal: zero `.unwrap()` in `crates/agentzero-gateway/`, `crates/agentzero-orchestrator/`, `crates/agentzero-infra/`, `crates/agentzero-providers/` outside tests.

**Acceptance:**
- Audit script committed to `scripts/check-unwrap.sh`
- The four "hot" crates above have 0 `.unwrap()` in non-test code
- `cargo clippy --workspace --all-targets` still 0 warnings
- All tests still pass

---

## Out of Scope

- Telemetry validation against a real backend (Honeycomb/Jaeger). Worth doing but requires a real account and a deployment, not a code change.
- SLO definitions and error budgets. That's an operations exercise, not a code one.
- HNSW corruption recovery. Worth doing but separate, larger sprint.
- Sprint 83 Phase C (InferenceBackend refactor) and Phase D (build.rs marker pattern). Both still queued.

---

## Acceptance Criteria (Sprint 84)

- [ ] Phase A: load harness exists, runs cleanly, produces a documented baseline
- [ ] Phase B: codegen has timeout + kill-switch + audit log, all three tested
- [ ] Phase C: zero `.unwrap()` in the four hot crates outside tests
- [ ] `cargo clippy --workspace --all-targets` — 0 warnings
- [ ] `cargo test --workspace` — all tests pass
- [ ] No regressions in any existing test
