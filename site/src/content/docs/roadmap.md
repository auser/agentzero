---
title: Roadmap
description: AgentZero 6-week development roadmap from foundation through stabilization.
---

## Week 1: Foundation (Phase 0)
1. Set up workspace, linting, formatting, tests, CI.
2. Add CLI shell with `onboard`, `agent`, `status` commands.
3. Create ADR process and scope guardrails.
4. Exit criteria:
- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` all pass.

## Week 2: Core Contracts (Phase 1)
1. Define core domain types and traits:
- `Provider`
- `MemoryStore`
- `Tool`
2. Add a tiny `Agent` orchestrator that depends only on traits.
3. Exit criteria:
- No infra types imported by core crate.
- Unit tests for trait-based orchestration.

## Week 3: Infra Implementations (Phase 1)
1. Add OpenAI-compatible provider client.
2. Add SQLite memory store.
3. Add `read_file` and `shell` tools with explicit allowlists.
4. Exit criteria:
- Integration tests with mock provider response.
- Tool execution denied when allowlists fail.

## Week 4: Agent Loop Hardening (Phase 2)
1. Add max-iterations and timeout guards.
2. Add structured event logging.
3. Add deterministic transcript tests.
4. Exit criteria:
- Agent cannot loop forever.
- Tool and provider errors are surfaced clearly.

## Week 5: Config + Security Baseline (Phase 3)
1. TOML config + env overrides.
2. Secret redaction in logs.
3. Safe defaults for path/command allowlists.
4. Exit criteria:
- Config validation rejects dangerous defaults.
- Security tests for redaction and blocked operations.

## Week 6: Stabilize and Measure (Phase 4)
1. Add latency/error metrics.
2. Add failure-injection tests (timeouts, malformed output, sqlite lock).
3. Add minimal benchmark harness.
4. Exit criteria:
- Stable behavior under common failures.
- Baseline performance report checked into `docs/benchmarks.md`.

## Work Rules
- Add one capability per PR.
- Every feature needs: tests, docs, and one explicit non-goal.
- Do not add daemon/channels/hardware until Phase 4 is complete.
