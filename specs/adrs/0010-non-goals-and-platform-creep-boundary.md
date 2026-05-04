# ADR 0010: Non-Goals and Platform Creep Boundary

## Status

Accepted

## Context

The new project exists to escape platform gravity and build the smallest trustworthy secure local agent harness. Without explicit non-goals, the project will grow back into the older broad AgentZero platform.

## Decision

AgentZero is not a swarm platform, hosted SaaS, workflow DAG engine, Fabric replacement, MCP marketplace, bot platform, SDK platform, or generic automation marketplace. These capabilities may exist later as packages, adapters, or separate projects only after ADR approval.

## Consequences

Implementation must reject v0 scope additions for swarms, hosted dashboards, multi-user teams, billing, Slack/Discord/Telegram bots, full MCP marketplace, all provider integrations, SDKs, long-running daemon orchestration, and workflow DAGs.
