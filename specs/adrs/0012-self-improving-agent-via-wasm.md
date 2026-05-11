# ADR 0012: Self-Improving Agent via WASM

## Status

Accepted

## Context

AgentZero's core differentiators are its agentic tool loop (LLM → tools → repeat) and its WASM sandbox with runtime isolation. Today these exist independently: the agent loop uses built-in tools (read, list, search, write, edit, shell), and the WASM sandbox executes pre-installed skill modules.

No local AI tool lets an agent **generate new tools at runtime, compile them to WASM, load them inside the sandbox, and use them in subsequent rounds** — all while enforcing policy, audit, and trust boundaries. This is the feature that makes AgentZero unique.

### Threat model

Self-improving agents create new attack surfaces:

- **Prompt injection into WASM generation** (OWASP LLM01): attacker-controlled content tricks the agent into generating malicious tools.
- **WASM sandbox escape** (CVE-2026-34971 class): bugs in wasmtime/Cranelift allow guest-to-host memory access.
- **Confused deputy**: generated tool legitimately calls policy-gated host imports on attacker's behalf.
- **Exfiltration**: backdoored tool reads sensitive files via host imports, encodes data in return value.

Mitigations rely on existing architecture: deny-by-default policy (ADR 0003), untrusted content labels (ADR 0008), capability-based access (ADR 0009), and fuel-limited execution (ADR 0006).

## Decision

### 1. WASM host imports via WIT

Define an `az:host` WIT interface (see ADR 0013) exposing policy-gated host functions to WASM guests:

- `az_read_file(path) -> result<string, string>`
- `az_write_file(path, content) -> result<bool, string>`
- `az_log(message)`
- `az_get_secret(handle) -> result<string, string>`

Each host call goes through the policy engine. Gate behind the existing `WasmHostCall` capability in `agentzero_core::Capability`.

### 2. Two-tier compilation

**Tier 1 — `wasm-encoder` (zero overhead, default)**: Programmatically generate WASM bytecode from templates. The agent describes tool behavior; AgentZero generates WASM sections directly. No compiler toolchain required. Best for structured transformers, parsers, validators.

**Tier 2 — Javy (lightweight, future)**: Embed the Javy runtime (Bytecode Alliance, JS → QuickJS bytecode → WASM). Agent writes JavaScript; output modules are 1–16 KB. Best for complex logic. Deferred to P3.

### 3. Dynamic per-project tool registration

- `az_register_tool(name, schema_json, wasm_bytes)` registers a new tool.
- Tools stored **per-project** in `.agentzero/skills/<tool>/v1/` (not global).
- Directory-based versioning (`v1/`, `v2/`, ...) with `active.json` pointer.
- Trust label: `Untrusted` (ADR 0008). Policy engine evaluates like any other tool.
- Lockfile generated for integrity verification on reload (ADR 0011).

### 4. Agent loop integration

When the agent loop detects a missing capability:

1. Agent writes source code or tool specification.
2. AgentZero compiles to WASM (Tier 1 or Tier 2).
3. Registers as dynamic tool via `az_register_tool`.
4. Tool available in the next round.
5. `max_generation_attempts` (default 2) prevents infinite generation loops.

### 5. Error recovery

- Generated tool fails at runtime → log, disable, fall back to built-in tools.
- Compilation fails → report error, don't register partial tool.
- Fuel limit exceeded → terminate, audit timeout, suggest simpler implementation.

## Consequences

### Positive

- Agents can extend their own capabilities within the sandbox.
- Per-project isolation prevents tool leakage between workspaces.
- Policy engine enforces the same rules on generated tools as built-in tools.
- Users can promote generated tools to the catalog via `az publish`.

### Negative

- Increases attack surface (prompt injection → tool generation → execution).
- WASM sandbox is not a perfect security boundary (JIT bugs are possible).
- `wasm-encoder` templates limit the expressiveness of Tier 1 tools.
- Binary size increases with `wasm-encoder` dependency.

### Mitigations

- All generated WASM labeled `Untrusted` — never becomes trusted instruction (ADR 0008).
- Every host call policy-checked — `WasmHostCall` capability required (ADR 0003).
- Fuel-limited execution prevents infinite loops (ADR 0006).
- `.agentzero/` path blocked from tool access (P0 security hardening).
- WASM import verification rejects modules with undeclared imports.
- Session-scoped approval tracking prevents scope creep.
