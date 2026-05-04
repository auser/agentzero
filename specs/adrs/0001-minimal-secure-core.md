# ADR 0001: Minimal Secure Core

## Status

Accepted

## Context

AgentZero must avoid becoming the previous broad platform. The product wedge is a trustworthy local agent harness that can safely operate on private developer data. A small core is easier to reason about, test, audit, and secure.

## Decision

AgentZero is a small local secure session engine. The core includes only session orchestration, provider abstraction, policy evaluation, redaction, audit, a minimal tool registry, project-local configuration, and CLI/TUI entrypoints. Gateway, MCP, ACP, packages, MVM, WASM, swarms, SDKs, and marketplace behavior are adapters or optional modules, not core architecture.

## Consequences

The first implementation must not include swarms, channels, SDKs, hosted dashboards, workflow DAGs, or broad provider catalogs. Every new capability must prove that it belongs inside the secure core or be implemented behind an adapter/package/runtime boundary.
