# Sprint: AgentZero Bootstrap

## Goal

Establish the documentation, ADR, security, and implementation foundation for AgentZero as a local-first secure AI agent harness.

## Active Plan

- `specs/plans/0001-bootstrap-agentzero.md`

## Current Phase

**Status: PHASE 9 COMPLETE**

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

### Phase 5: First Demo
- [x] Built-in `repo-security-audit` skill with pattern-based scanner.
- [x] Patterns loaded from external `patterns.toml` (extensible without code changes).
- [x] Run against this repository (`agentzero run repo-security-audit`).
- [x] Human-readable markdown audit report with severity, recommendations.
- [x] Report written to `.agentzero/audit/` when project is initialized.
- [x] Malicious fixture tests (secrets, PII, prompt injection, package scripts, sensitive files).
- [x] 12 scanner tests + 3 report tests covering all finding categories.

### Phase 6: Policy Loading, Model Provider, Chat Loop
- [x] Policy YAML/TOML loader (`load_policy_file` parses `.agentzero/policy.yml` into rules).
- [x] `init --private` writes valid TOML policy files.
- [x] `policy status` loads and displays active rules.
- [x] Ollama model provider (`OllamaProvider` — local HTTP client for Ollama REST API).
- [x] Health check, chat completion, configurable model/endpoint.
- [x] Interactive chat loop (`agentzero chat --local` with Ollama backend).
- [x] Policy loaded from project config at chat startup.
- [x] System prompt, conversation history, `/quit` command.

### Phase 7: Tool Use, Streaming, Shell Approval
- [x] Ollama tool calling (model sends tool_calls, session executes them).
- [x] Tool definitions for read/list/search/shell sent to Ollama.
- [x] Multi-round tool loop (up to 5 rounds of tool calls per user message).
- [x] Shell command execution with user approval prompt (y/n).
- [x] Streaming chat support (`chat_streaming` with token-by-token callback).
- [x] Tool results truncated to 2KB to avoid context overflow.
- [x] Session wired into chat with policy-controlled tool executor.
- [x] Audit events emitted for each tool call during chat.

### Phase 8: CLI Polish, File Write Tool, Streaming, Bin Setup
- [x] `--model` flag for `agentzero chat` (e.g. `--model codellama`).
- [x] `--stream` flag for token-by-token output in chat.
- [x] File write tool with user approval prompt (y/n before writing).
- [x] `/tools` slash command shows available tools during chat.
- [x] `/session` slash command shows session info.
- [x] `default-members` in workspace so `cargo run` defaults to CLI binary.
- [x] 5 tool definitions (read, list, search, write, shell).

### Phase 9: Persistence, History, WASM Skeleton
- [x] Audit logging to `.agentzero/audit/` during chat sessions.
- [x] Conversation persistence — saves chat history to `.agentzero/sessions/<id>.json`.
- [x] `agentzero history` command lists past sessions with metadata.
- [x] WASM sandbox engine (`wasmtime`-backed, behind `wasm` feature flag).
- [x] WASM config: memory limits, fuel-based time limits, no ambient filesystem.
- [x] Stub types when `wasm` feature is disabled (compiles without wasmtime).

## Not Yet

- [ ] MVM runtime.
- [ ] ACP adapter.
- [ ] Package installer.
- [ ] Gateway.
- [ ] MCP bridge.
- [ ] Swarms.
- [ ] SDKs.

## Notes

This sprint intentionally prevents platform creep. The first implementation milestone is a small local secure session engine, not a hosted platform, workflow orchestrator, swarm runtime, or package marketplace.
