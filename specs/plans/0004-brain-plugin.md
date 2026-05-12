# Plan 0004: Personal Brain Plugin (WASM)

## Context

AgentZero should offer a personal LLM wiki ("brain") — a Karpathy-style knowledge base that compounds over time. Markdown-native, Git-safe, Obsidian-compatible, local-first.

Reference: [pi-llm-wiki](https://github.com/zosmaai/pi-llm-wiki) (TypeScript MCP server by pi.dev).
Detailed spec: `specs/prompts/0006-agentzero-brain-production-plugin-prompt.md`.
Decision record: ADR 0015.

## Decision: WASM Plugin, Not Native Crate

Brain will be built as a WASM plugin, not as workspace crate #11.

**Rationale:** ADR 0012 positions self-improving agents via WASM as AgentZero's core differentiator. Building brain as the first real WASM plugin proves the system works on something meaningful. A native crate would undermine the plugin story — interesting features end up built-in while plugins only run toy examples.

## ABI Requirements

Brain needs these host imports beyond the current `az:host@0.1.0` (read-file, write-file, log):

| Import | Interface | Capability |
|--------|-----------|------------|
| `append-file` | filesystem | FileWrite |
| `list-dir` | filesystem | FileRead |
| `create-dir` | filesystem | FileWrite |
| `file-exists` | filesystem | FileRead |
| `now` | clock | (none) |

These are defined in `az:host@0.2.0` (`crates/agentzero-sandbox/wit/az-host.wit`).

## Minimal Viable Brain (4 commands)

| Command | What it does | Host imports used |
|---------|-------------|-------------------|
| `brain init` | Create vault dirs + config + starter files | create-dir, write-file, file-exists |
| `brain today` | Create/print daily note from template | read-file, write-file, file-exists, now |
| `brain capture "msg"` | Append timestamped line to daily note | read-file, append-file, now |
| `brain query "term"` | Text search across wiki/ files | list-dir, read-file |

## Phasing

### Phase 1: Core Vault Ops (MVP)
- `brain init`, `brain today`, `brain capture`, `brain query`
- Text-only search (no LLM, no RAG)
- Config via `.agentzero-brain.toml`

### Phase 2: Search
- Search trait with qmd/ripgrep/built-in backends
- `brain index` (delegate to agentzero-index when available)

### Phase 3: Agent Workflows
- `brain ingest`, `brain ask`, `brain review`, `brain weekly`
- Prompt-only mode first, `--invoke` when session API is stable

### Phase 4: Health and Git
- `brain health`, `brain checkpoint`
- Wikilink scanning, vault diagnostics

## Blocked On

- Phase 24 host import runtime wiring (Linker + SessionHostCallbacks for new imports)
- WIT Phase 2 adoption (component model) is nice-to-have but not blocking

## What pi-llm-wiki Gets Right

- Four-layer architecture (raw → wiki → metadata → config)
- Source provenance with stable IDs
- Prompt-only mode as default
- Obsidian compatibility

## What We Do Differently

- CLI-first, not MCP-first
- Local models via Ollama, not cloud-dependent
- Tool registration in ToolRegistry (composable) rather than system-prompt injection
- Markdown + YAML frontmatter only — no JSON sidecar databases
- Policy engine protects raw files and vault boundaries
