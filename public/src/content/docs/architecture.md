---
title: Architecture
description: High-level view of the agentzero runtime and crate boundaries.
---

This document provides a high-level view of the current `agentzero` runtime and crate boundaries.

## Crate Diagram

```mermaid
flowchart TD
    U[User / CLI Invocation] --> B[bin/agentzero]
    B --> C[crates/agentzero-cli]

    C --> CFG[crates/agentzero-config]
    C --> CORE[crates/agentzero-core]
    C --> RT[crates/agentzero-runtime]
    C --> GW[crates/agentzero-gateway]
    C --> INFRA[crates/agentzero-infra]
    C --> MEM[crates/agentzero-memory]
    C --> PROV[crates/agentzero-providers]
    C --> SEC[crates/agentzero-security]
    C --> TOOLS[crates/agentzero-tools]
    C --> WASM[crates/agentzero-plugins]
    C --> TK[crates/agentzero-testkit]

    CORE --> INFRA
    INFRA --> TOOLS
    INFRA --> SEC
    RT --> CORE
    RT --> CFG
    RT --> INFRA
```

## Command Execution Flow

```mermaid
sequenceDiagram
    participant User
    participant Bin as bin/agentzero
    participant CLI as agentzero-cli
    participant Config as agentzero-config
    participant Core as agentzero-core
    participant Infra as agentzero-infra
    participant Provider as agentzero-providers
    participant Memory as sqlite/turso memory crate
    participant Tools as agentzero-tools

    User->>Bin: agentzero <command>
    Bin->>CLI: parse_cli_from + execute
    CLI->>Config: load config + policy
    alt command=agent
        CLI->>Core: Agent::respond(message)
        Core->>Provider: complete(prompt)
        Core->>Memory: recent/append
        Core->>Tools: tool execution via infra registry
        Core-->>CLI: response text
    else command=status
        CLI->>Memory: recent()
    else command=gateway
        CLI->>Infra: build runtime deps
        CLI->>Bin: start HTTP server loop
    else command=doctor
        CLI->>CLI: run local diagnostics
    end
    CLI-->>User: stdout/stderr + exit code
```

## Current Responsibilities

- `bin/agentzero`: Thin executable entrypoint and process exit behavior.
- `agentzero-cli`: Command parsing, command dispatch, UX, diagnostics, and orchestration glue.
- `agentzero-runtime`: Runtime orchestration for agent execution flows used by CLI commands.
- `agentzero-config`: Typed config model, validation, dotenv/env/file layering, policy loading.
- `agentzero-core`: Agent domain loop and trait-driven orchestration.
- `agentzero-providers`: OpenAI-compatible provider implementation and retry/error mapping.
- `agentzero-memory`: Unified memory crate (SQLite default + optional Turso/libsql backend via feature).
- `agentzero-tools`: Hardened tool implementations (`read_file`, `write_file`, `shell`) with policy gates.
- `agentzero-security`: Redaction and security policy utilities used by infra/runtime paths.
- `agentzero-infra`: Integration layer for provider/memory/tool wiring and optional plugin/mcp tools.
- `agentzero-gateway`: HTTP service surface for runtime access and health/ping.
- `agentzero-plugins`: plugin packaging/lifecycle and WASM preflight/runtime policy checks.
- `agentzero-testkit`: Reusable test doubles/mocks for provider, memory, and tool trait testing.

## Security Boundaries

- Tool execution is policy-gated from config (`[security.*]`) and fails closed by default.
- Optional capabilities (`write_file`, `mcp`, process plugins) require explicit enablement.
- Audit events can be enabled via `[security.audit]` for traceability of execution steps.
- Config validation enforces bounded values and safe URL/path constraints before runtime execution.

## Notes

- Runtime orchestration is being moved from CLI command handlers into `agentzero-runtime`; some CLI paths may still have light transitional glue.
- Doctor diagnostics are currently CLI-local checks; deeper daemon/scheduler freshness checks are tracked in Sprint 11.
