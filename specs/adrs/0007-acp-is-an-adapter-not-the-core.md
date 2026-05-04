# ADR 0007: ACP Is an Adapter, Not the Core

## Status

Accepted

## Context

ACP is strategically valuable for Zed and other editor integrations, but it must not define AgentZero's internal architecture. Editor integrations introduce privacy boundaries around buffers, unsaved files, selections, diagnostics, terminals, and workspace context.

## Decision

AgentZero will support ACP as an editor interoperability adapter after the CLI session engine is stable. ACP must call into the same session engine, policy engine, redaction engine, tool runtime, audit log, and provider router used by the CLI.

## Consequences

ACP profiles must define what editor context AgentZero can see. The default profile must not expose whole-workspace context or remote model access. ACP must produce audit events and enforce the same policy as all other interfaces.
