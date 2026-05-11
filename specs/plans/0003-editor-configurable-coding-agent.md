# 0003 — AgentZero as an Editor-Configurable Coding Agent (Pi Model)

## Objective

Make `agentzero serve` a fully functional coding agent that editors can spawn and talk to — like Pi (pi.dev), not like an MCP server. Pi IS the coding agent; editors configure it via `models.json` and communicate via JSON-over-stdio RPC. AgentZero already has most infrastructure but the pieces aren't wired together.

## Context

Pi (pi.dev) explicitly says **"No MCP"**. Instead of being a passive tool server, Pi is a "minimal terminal coding harness" — editors spawn it, configure it, and communicate via a JSON RPC protocol over stdin/stdout. A [blog post](https://patloeber.com/gemma-4-pi-agent/) demonstrates Pi configured to use LM Studio serving Gemma 4 locally.

AgentZero's current state:
- **ACP server** (`agentzero serve`) has the right protocol shape but `Chat` is a stub
- **Session engine** has the full agentic loop (LLM → tool calls → loop) but it's embedded in `cmd_chat` CLI code
- **ProviderRouter** exists but is hardcoded per-invocation, not loaded from `models.json`
- **Tool system** has read/list/search/write/shell but no `edit` (search-and-replace)

## Phases

### Phase 1: Extract AgentLoop from CLI into agentzero-session

Extract the agentic loop (send to LLM, execute tool calls, loop up to N rounds, handle approvals) from `commands.rs:872-998` into a reusable `AgentLoop` struct in `agentzero-session`.

- New: `crates/agentzero-session/src/agent_loop.rs`
- Modify: `crates/agentzero-session/src/lib.rs`, `crates/agentzero-cli/src/commands.rs`

### Phase 2: Wire ACP Chat to AgentLoop

Connect `agentzero serve`'s Chat handler to the real `AgentLoop`. Add streaming notifications and model management methods to the ACP protocol.

- Modify: `crates/agentzero-acp/src/protocol.rs`, `crates/agentzero-acp/src/server.rs`, `crates/agentzero-acp/Cargo.toml`

### Phase 3: Dynamic Provider Loading from models.json

Add `ProviderRouter::from_config()` so both CLI and ACP load providers from the same `models.json`. Support mid-session model switching.

- New: `crates/agentzero-session/src/models_config.rs`
- Modify: `crates/agentzero-session/src/router.rs`, `crates/agentzero-cli/src/commands.rs`

### Phase 4: Print/JSON Mode for CLI

Add `az chat -p "question" --mode json` for single-shot queries. Any editor can shell out to this without a protocol.

- Modify: `crates/agentzero-cli/src/commands.rs`

### Phase 5: Editor Configuration Generators

`az init --editor vscode|cursor|zed` generates editor-native config files for immediate integration.

- Modify: `crates/agentzero-cli/src/commands.rs`

### Phase 6: Edit Tool (search-and-replace)

Add `edit` tool with `{path, old_text, new_text}` for surgical file edits. Returns unified diff.

- Modify: `crates/agentzero-session/src/tool_exec.rs`, `crates/agentzero-session/src/ollama.rs`, `crates/agentzero-session/src/openai_compat.rs`

## Implementation Order

```
Phase 1 → Phase 2 → Phase 3  (critical path: ACP becomes a real agent)
Phase 4                        (independent, can parallel with Phase 3)
Phase 6                        (independent, can parallel with anything)
Phase 5                        (after Phase 2, needs working ACP)
```

Suggested sequence: **1 → 2 → 3 → 4 → 6 → 5**

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Protocol | Evolve ACP (not new) | ADR 0007: ACP is the adapter layer |
| Streaming | Server-initiated notifications (no `id`) | Matches LSP pattern |
| Tool approval | Bidirectional ACP messages | Security-first; can't auto-approve writes/shell |
| Provider config | `models.json` single source | Already exists, matches Pi pattern |
| MCP | Keep as-is (passive tool provider) | MCP and ACP serve different purposes |

## Verification

1. `cargo test -p agentzero-session` — AgentLoop unit tests
2. `agentzero serve` + piped JSON Chat — real LLM response
3. `models.json` → LM Studio — both CLI and ACP use it
4. `az chat -p "what is 2+2" --mode json` — valid JSON output
5. `az init --editor vscode` — creates `.vscode/tasks.json`
6. Model uses `edit` tool to modify a file
7. End-to-end: VS Code spawns `agentzero serve`, sends Chat, gets response
