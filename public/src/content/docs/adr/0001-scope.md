---
title: "ADR-0001: Initial Scope Boundaries"
description: Decision record defining AgentZero v0 scope restrictions.
---

## Status
Accepted

## Context
The reference project has a broad surface (daemon, channels, hardware, cron, gateway, auth variants, plugin runtime).
A learning clone should isolate core ideas without inheriting complexity.

## Decision
AgentZero v0 scope is restricted to:
1. Single-process CLI runtime.
2. Three commands: `onboard`, `agent`, `status`.
3. One provider protocol: OpenAI-compatible chat endpoint.
4. One memory backend: SQLite.
5. Two tools: `read_file` and `shell`, both allowlisted.

## Non-Goals
- No daemon mode.
- No channel integrations.
- No hardware/peripheral support.
- No plugin runtime.
- No multimodal/RAG.

## Consequences
- Faster feedback cycles and easier debugging.
- Lower risk of architecture drift.
- Straight path for adding advanced modules later behind feature flags.
