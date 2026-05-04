# ADR 0003: Policy, Redaction, and Audit Wrap Every Action

## Status

Accepted

## Context

Security failures often occur through side paths: tools, logs, adapters, package hooks, editor integrations, and model providers. AgentZero must have a single enforcement path rather than per-feature best effort checks.

## Decision

Every meaningful action must pass through policy evaluation before execution and emit an audit event after decision. Redaction and secret scanning must occur before model calls, network egress, logs, and tool output exposure when sensitive content may be present.

## Consequences

No tool, skill, package, ACP session, model provider, runtime adapter, or gateway endpoint may bypass policy, redaction, or audit. Implementation must prefer centralized typed contracts over informal conventions.
