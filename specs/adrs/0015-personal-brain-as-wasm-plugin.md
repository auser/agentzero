# ADR 0015: Personal Brain as WASM Plugin

## Status

Proposed

## Context

Andrej Karpathy's "LLM wiki" concept describes a personal knowledge base that compounds over time: raw sources are ingested, synthesized into durable wiki pages, and refined as understanding deepens. The pattern replaces ephemeral RAG (search raw docs every query) with persistent knowledge that improves with each interaction.

[pi-llm-wiki](https://github.com/zosmaai/pi-llm-wiki) by pi.dev implements this as a TypeScript MCP server with 11 tools, four-layer architecture (raw sources, wiki pages, metadata, config), and Obsidian-compatible Markdown output. It validates real demand for the pattern but requires a cloud LLM host (Claude Code, Cursor) to function.

AgentZero is well-positioned to offer a local-first, CLI-native version that works with local models via Ollama. The question is how to build it.

### Options considered

**Option A: Native Rust crate (`crates/agentzero-brain/`).** Tightly integrated, full access to internal APIs. Risk: becomes crate #11 in the workspace, establishes a pattern where interesting features are always built-in, plugin system only runs toy examples.

**Option B: WASM plugin.** Demonstrates the plugin system on a real use case. Independently versioned. Forces the WIT ABI to mature. Risk: depends on Phase 24 host import completion; WASM ABI may need extension.

**Option C: Standalone binary.** Separate from AgentZero entirely. Risk: fragmented install story, duplicated infrastructure.

## Decision

Build the brain as a **WASM plugin** (Option B).

### Rationale

ADR 0012 positions self-improving agents via WASM as AgentZero's core differentiator. The brain plugin is the ideal first real-world test of that system. If the plugin ABI can support a stateful, filesystem-heavy workflow tool, it can support anything.

Building brain as a native crate would undermine the plugin story by keeping all interesting functionality built-in. The WASM plugin path forces the ABI to mature and establishes a pattern for community-contributed plugins.

### ABI requirements

The brain plugin requires five host imports beyond `az:host@0.1.0`:

| Import | Interface | Purpose |
|--------|-----------|---------|
| `list-dir` | filesystem | Search, health checks, vault enumeration |
| `create-dir` | filesystem | Vault initialization |
| `file-exists` | filesystem | Idempotent operations, skip-if-exists |
| `append-file` | filesystem | Capture (append to daily note without read-modify-write) |
| `now` | clock | Timestamps for daily notes and captures |

These are defined in `az:host@0.2.0` (`crates/agentzero-sandbox/wit/az-host.wit`). All are generic — useful for any plugin, not brain-specific.

### Minimal viable plugin (4 commands)

1. `brain init` — create vault directory structure and config file
2. `brain today` — create or print today's daily note from template
3. `brain capture "message"` — append timestamped line to daily note
4. `brain query "term"` — text search across wiki/ files

Additional commands (ingest, ask, review, weekly, health, checkpoint) follow in later phases. Prompt-only mode is the default for agent-assisted commands; `--invoke` requires explicit opt-in.

### Vault structure

```
brain/
  .agentzero-brain.toml
  raw/inbox/  raw/sources/  raw/assets/
  wiki/daily/  wiki/weekly/  wiki/projects/  wiki/areas/
  wiki/decisions/  wiki/people/  wiki/sources/  wiki/reports/
  wiki/index.md  wiki/log.md
  prompts/claude/  prompts/maintenance/  prompts/workflows/
  templates/
```

Invariants: raw/ is immutable, wiki/ is agent-maintained, all files are Markdown with YAML frontmatter, Obsidian-compatible, no proprietary state.

### What we borrow from pi-llm-wiki

- Four-layer separation (raw sources, wiki pages, metadata, config)
- Source provenance with stable IDs and ingest ledger
- Prompt-only mode as default
- Obsidian compatibility without Obsidian dependency

### What we do differently

- CLI-first, not MCP-first (MCP exposure is a future extension)
- Local models via Ollama, not cloud-dependent
- Tool registration in ToolRegistry rather than system-prompt injection
- Markdown + YAML frontmatter only — no JSON sidecar databases
- Policy engine enforces vault boundaries and raw file immutability
- Audit logging for all write operations

## Consequences

### Positive

- Validates the WASM plugin system with a real, complex use case
- Forces WIT ABI to mature with generic filesystem imports useful for all plugins
- Independently versioned — brain can ship on its own release cadence
- Establishes the pattern for community-contributed WASM plugins
- Provides immediate daily-use value for AgentZero users

### Negative

- Blocked on Phase 24 host import completion (runtime wiring for new imports)
- WASM guest has limited access to AgentZero internals (no direct index crate or session API)
- Plugin development requires WASM toolchain (Rust → wasm32-wasi target)
- Performance overhead of WASM sandbox for filesystem-heavy operations (likely negligible)

### Risks

- WASM ABI keeps shifting during Phase 24 development. Mitigate by scoping MVP to operations already defined in the WIT spec.
- Brain scope creeps beyond what the plugin ABI supports. Mitigate by starting with 4 commands and growing incrementally.

## References

- Spec: `specs/prompts/0006-agentzero-brain-production-plugin-prompt.md`
- Plan: `specs/plans/0004-brain-plugin.md`
- ADR 0012: Self-Improving Agent via WASM
- ADR 0013: WIT Adoption for Tool Interfaces
- pi-llm-wiki: https://github.com/zosmaai/pi-llm-wiki
