---
title: Privacy Configuration
description: Complete reference for all privacy-related TOML configuration options in AgentZero.
---

All privacy settings live under the `[privacy]` section of `agentzero.toml`. The `privacy` Cargo feature flag must be enabled at compile time.

## `[privacy]`

```toml
[privacy]
mode = "off"                          # off | private | local_only | encrypted | full
block_cloud_providers = false          # Legacy flag (prefer mode = "local_only")
enforce_local_provider = false         # Require local provider regardless of mode
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `mode` | string | `"off"` | Privacy mode preset. See [Privacy Guide](/guides/privacy/) for details. |
| `block_cloud_providers` | bool | `false` | Block cloud provider kinds (legacy; use `mode = "local_only"` instead). |
| `enforce_local_provider` | bool | `false` | Require a local provider even when mode isn't `local_only`. |

## `[privacy.noise]`

Noise Protocol configuration for encrypted transport.

```toml
[privacy.noise]
enabled = false
handshake_pattern = "XX"              # XX (mutual auth) or IK (known server key)
session_timeout_secs = 3600           # Session TTL in seconds
max_sessions = 1000                   # Max concurrent active sessions
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Enable Noise Protocol encryption. Auto-enabled by `mode = "private"`, `"encrypted"`, or `"full"`. |
| `handshake_pattern` | string | `"XX"` | Noise handshake pattern. `XX` = mutual auth (recommended). `IK` = client knows server key. |
| `session_timeout_secs` | u64 | `3600` | Seconds before a Noise session expires. Must be > 0. |
| `max_sessions` | usize | `1000` | Maximum number of concurrent active sessions. Must be > 0. |

## `[privacy.sealed_envelopes]`

Sealed envelope configuration for zero-knowledge routing.

```toml
[privacy.sealed_envelopes]
enabled = false
max_envelope_bytes = 65536            # Maximum size of a single sealed envelope
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Enable sealed envelope support. Auto-enabled by `mode = "full"`. |
| `max_envelope_bytes` | usize | `65536` | Maximum sealed envelope payload size in bytes. Must be > 0. |

## `[privacy.key_rotation]`

Automatic key rotation for Noise Protocol static keys.

```toml
[privacy.key_rotation]
enabled = false
interval_hours = 24                    # Hours between automatic rotations
persist_path = "keys/"                 # Directory for persisting keys across restarts
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Enable automatic key rotation. Auto-enabled by `mode = "private"`, `"encrypted"`, or `"full"`. |
| `interval_hours` | u64 | `24` | Hours between automatic key rotations. |
| `persist_path` | string | `"keys/"` | Directory path for storing rotated keys. Relative to data directory. |

## Per-Component Boundaries

### Agent Boundaries

Set on individual agents in the `[agents.<name>]` section:

```toml
[agents.research]
provider = "anthropic"
model = "claude-sonnet-4-6"
privacy_boundary = "encrypted_only"    # inherit | local_only | encrypted_only | any
allowed_providers = ["anthropic"]       # Restrict to specific provider kinds
blocked_providers = ["openai"]          # Block specific provider kinds
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `privacy_boundary` | string | `""` (inherit) | Privacy boundary for this agent. Can't be more permissive than global mode. |
| `allowed_providers` | list | `[]` | If non-empty, only these provider kinds are allowed. |
| `blocked_providers` | list | `[]` | These provider kinds are always blocked. |

### Tool Boundaries

Set per-tool in the `[security.tool_boundaries]` section:

```toml
[security.tool_boundaries]
shell = "local_only"                   # Shell can't access network
web_search = "any"                     # Web search has no restrictions
http_request = "encrypted_only"        # HTTP requires encrypted transport
```

Keys are tool names. Values are boundary strings: `inherit`, `local_only`, `encrypted_only`, `any`.

Tool boundaries are resolved against the agent's boundary — the result is always at least as strict as the agent's.

## Validation Rules

Config validation enforces:

1. **Mode values** — Must be one of: `off`, `local_only`, `encrypted`, `full`
2. **Boundary values** — Must be one of: `inherit`, `local_only`, `encrypted_only`, `any` (or empty string for inherit)
3. **Boundary hierarchy** — Agent boundaries can't be more permissive than global mode
4. **Provider compatibility** — `local_only` mode requires a local provider kind and localhost base URL
5. **Noise parameters** — `session_timeout_secs` and `max_sessions` must be > 0
6. **Envelope size** — `max_envelope_bytes` must be > 0
7. **Handshake pattern** — Must be `XX` or `IK`

## Mode-as-Preset

Each mode is a preset that auto-configures multiple settings:

| Setting | `off` | `local_only` | `encrypted` | `full` |
|---------|-------|-------------|-------------|--------|
| Cloud providers | Yes | No | Yes | Yes |
| Noise Protocol | No | No | Yes | Yes |
| Sealed envelopes | No | No | No | Yes |
| Key rotation | No | No | Yes | Yes |
| Network tools | Yes | No | Yes | Yes |
| WASM plugin network | Yes | No | Yes | Yes |
