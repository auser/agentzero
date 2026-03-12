---
title: Environment Variables
description: All environment variables recognized by the AgentZero runtime.
---

AgentZero reads several environment variables for API keys, backend selection, and runtime overrides.

## `.env` File Support

All environment variables can be set in `.env` files instead of (or in addition to) the shell environment. AgentZero loads `.env` files from two locations:

1. **Config directory** (`~/.agentzero/.env`) — global defaults
2. **Current working directory** (`./.env`) — project-local overrides

Within each directory, files are loaded in this order (later overrides earlier):

- `.env` — base values
- `.env.local` — local overrides (typically gitignored)
- `.env.{AGENTZERO_ENV}` — environment-specific (e.g. `.env.production`), selected via `AGENTZERO_ENV`, `APP_ENV`, or `NODE_ENV`

CWD files take priority over config-dir files. Process environment variables (`export`) take priority over all `.env` files.

## Core Variables

| Variable | Description | Default |
|---|---|---|
| `OPENAI_API_KEY` | API key for OpenAI-compatible providers | — |
| `AGENTZERO_ENV` | Environment mode (`development` or `production`). When `production`, startup validates TLS and auth are configured | `development` |
| `AGENTZERO_DATA_DIR` | Override data/config directory | `~/.agentzero` |
| `AGENTZERO_CONFIG` | Override config file path | `$DATA_DIR/agentzero.toml` |
| `AGENTZERO_MEMORY_BACKEND` | Memory backend (`sqlite` or `turso`) | `sqlite` |
| `AGENTZERO_DATA_KEY` | Encryption key for storage (base64 or 64-char hex) | auto-generated key file |
| `AGENTZERO_GATEWAY_BEARER_TOKEN` | Static bearer token for gateway auth | — |
| `RUST_LOG` | Logging level (also set via `-v` flags) | — |

## Provider Keys

| Variable | Description |
|---|---|
| `OPENAI_API_KEY` | OpenAI, OpenRouter, or any OpenAI-compatible provider |
| `ANTHROPIC_API_KEY` | Anthropic direct API access |
| `BRAVE_API_KEY` | Brave Search API (for `web_search` tool) |
| `PERPLEXITY_API_KEY` | Perplexity Search API |
| `EXA_API_KEY` | Exa Search API |
| `JINA_API_KEY` | Jina Reader/Search API |
| `COMPOSIO_API_KEY` | Composio integration API |
| `GROQ_API_KEY` | Groq API (used for audio transcription via `[audio]` config) |

## Memory Backend

| Variable | Description |
|---|---|
| `AGENTZERO_MEMORY_BACKEND` | `sqlite` (default) or `turso` |
| `TURSO_AUTH_TOKEN` | Auth token for Turso/libsql remote backend |
| `TURSO_DATABASE_URL` | Turso database URL |

## Verbosity via CLI

The `-v` flag maps to `RUST_LOG` levels:

```bash
agentzero -v status        # RUST_LOG=error
agentzero -vv status       # RUST_LOG=info
agentzero -vvv status      # RUST_LOG=debug
agentzero -vvvv status     # RUST_LOG=trace
```

## JSON Output

Any command supports `--json` for machine-readable output:

```bash
agentzero --json status
```

Output format:

```json
{
  "ok": true,
  "command": "status",
  "result": { ... },
  "error": null
}
```
