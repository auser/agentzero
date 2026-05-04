# ADR 0006: Runtime Isolation Tiers: Host, WASM, and MVM

## Status

Accepted

## Context

WASM and MVM solve different isolation problems. WASM is lightweight and portable for constrained helpers. MVM provides a stronger boundary for untrusted language runtimes, shell-heavy tools, and networked services.

## Decision

AgentZero defines explicit runtime isolation tiers: `none`, `host-readonly`, `host-supervised`, `wasm-sandbox`, `mvm-microvm`, and `deny`. WASM is used for low-risk portable tools. MVM is used for high-risk execution: Python, Node, native binaries, package installs, browser automation, MCP servers, untrusted repos, and long-running services.

## Consequences

Runtime selection must be policy-driven. MVM guests receive projected filesystem views, not the host home directory. WASM host calls are explicitly declared and policy checked. Host execution remains supervised unless readonly and safe.
