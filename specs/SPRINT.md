# Sprint: AgentZero Bootstrap

## Goal

Establish the documentation, ADR, security, and implementation foundation for AgentZero as a local-first secure AI agent harness.

## Active Plan

- `specs/plans/0001-bootstrap-agentzero.md`

## Current Phase

**Status: PHASE 3 COMPLETE**

## Tasks

### Phase 0: Documentation Gate
- [x] Define `specs/project.md`.
- [x] Add `specs/security-model.md`.
- [x] Add ADRs 0001-0010.
- [x] Add bootstrap plan and Claude Code prompts.
- [x] Run `just ci` in target repository.

### Phase 1: Rust Workspace
- [x] Create Rust workspace (7 crates: core, policy, audit, tools, skills, sandbox, cli).
- [x] Implement CLI skeleton (`doctor`, `demo`, `init`, `chat`, `run`, `policy`, `audit`, `vault`).

### Phase 2: CLI Commands
- [x] `agentzero init --private` creates `.agentzero/` with policy.yml.
- [x] `agentzero doctor` reports crate status and project config.
- [x] `agentzero policy status` shows loaded policy.
- [x] `agentzero audit tail` reads JSONL audit logs.
- [x] `agentzero vault list` reports configured handles.

### Phase 3: Security Primitives
- [x] Data classification enum with model routing rules (ADR 0002).
- [x] Policy engine with rule-based evaluation and deny-by-default (ADR 0003).
- [x] Redaction interface with token-preserving placeholders.
- [x] Secret handles with capability-based access (ADR 0009).
- [x] Trust source labels for content provenance (ADR 0008).
- [x] Model routing decisions (local/remote/redact/deny).
- [x] Typed action kinds for audit events.
- [x] Audit event schema with JSONL sink and in-memory sink.
- [x] Approval scope model.
- [x] `agentzero demo` exercises all security primitives end-to-end.

### Phase 4: Minimal Session Engine
- [x] Local-only session mode (`SessionMode::LocalOnly`).
- [x] Model provider abstraction (`ModelProvider` trait, `LocalStubProvider`).
- [x] Supervised tool invocation (`ToolExecutor` with policy checks + path validation).
- [x] Read/list/search tools (real filesystem operations).
- [x] Proposed edit output (`propose_edit` tool — output only, no writes).
- [x] Shell approval flow (policy-based `RequiresApproval` for shell commands).
- [x] Centralized tracing crate (`agentzero-tracing` wrapping `tracing` + `tracing-subscriber`).
- [x] Session engine (`agentzero-session` — ties model/tools/policy/audit together).

### Phase 5: First Demo (next)
- [ ] Built-in `repo-security-audit` skill integration.
- [ ] Run against this repository.
- [ ] Produce `.agentzero/audit/<session>.jsonl`.
- [ ] Produce human-readable audit report.
- [ ] Add malicious fixture tests.

## Not Yet

- [ ] MVM runtime.
- [ ] WASM runtime.
- [ ] ACP adapter.
- [ ] Package installer.
- [ ] Gateway.
- [ ] MCP bridge.
- [ ] Swarms.
- [ ] SDKs.

## Notes

This sprint intentionally prevents platform creep. The first implementation milestone is a small local secure session engine, not a hosted platform, workflow orchestrator, swarm runtime, or package marketplace.
