# ADR 0014: MCP Deprecation to Optional Feature Flag

## Status

Accepted

## Context

AgentZero currently ships two protocol adapters:

- **ACP** (`agentzero-acp`): AgentZero owns the agentic loop. The editor sends chat messages; AgentZero runs LLM inference, decides tool calls, manages context, and streams progress.
- **MCP** (`agentzero-mcp`): AgentZero is a passive tool server. The external host (Claude Code, Cursor, Zed) owns the LLM and decides what tools to call.

With self-improving agents (ADR 0012) as the core feature, AgentZero must own the loop to detect missing capabilities, generate tools, and register them dynamically. This is impossible in MCP mode where the host drives.

Pi (pi.dev) explicitly positions itself as "not an MCP" for the same architectural reason: the agent must control the loop to be an agent.

Both adapters are thin (~450–500 lines each) over the same core (ADR 0007), so the binary cost is negligible. The issue is **identity**: shipping MCP as a default suggests AgentZero is a tool server, which contradicts its mission.

## Decision

Move the `agentzero-mcp` crate behind a Cargo feature flag `mcp`. ACP remains the default and only protocol in standard builds.

### Changes

1. In root `Cargo.toml`, add `mcp = ["agentzero-mcp"]` to the workspace feature list.
2. In `agentzero-cli`, gate the `mcp` subcommand behind `#[cfg(feature = "mcp")]`.
3. The `az mcp` command is only available when built with `--features mcp`.
4. The `az serve` command (ACP) remains always available.
5. Documentation updated to describe ACP as the native protocol.

### Who still needs MCP

Users integrating AgentZero into existing MCP clients (Claude Code, Cursor, Zed) as a tool provider. They can build with `--features mcp` or use a pre-built binary with the feature enabled.

## Consequences

### Positive

- Clarifies identity: AgentZero is an agent, not a tool server.
- Default binary is slightly smaller.
- Removes cognitive overhead for new users ("which protocol do I use?").
- Aligns with the self-improving agent direction (requires ACP).

### Negative

- Users who relied on `az mcp` need to rebuild with `--features mcp`.
- Breaks existing MCP integration setups on upgrade (mitigated by documentation).

### Migration

- Announce in release notes for the version that makes this change.
- Provide `--features mcp` build instructions.
- Consider publishing two binary variants (standard and mcp-enabled).
