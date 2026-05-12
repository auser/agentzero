# ADR 0015: WASM Plugin System

## Status

Accepted

## Context

AgentZero's self-improving agent architecture (ADR 0012) established WASM as the sandbox for dynamic tool generation. ADR 0013 defined the `az:host` WIT interface for host-guest communication. But the WASM runtime was limited to generated tools — there was no mechanism for installing, discovering, or running standalone WASM applications as first-class plugins.

Meanwhile, the brain plugin (personal LLM wiki) needed a home. Building it as workspace crate #12 would establish a pattern where all interesting features are built-in, undermining the plugin story. Building it as a WASM plugin forces the plugin system to mature.

### Options considered

**Option A: Skills-only.** Extend the existing skill system (SKILL.md, `az run <name>`) to support input/output. Risk: skills are designed for fire-and-forget execution, not interactive command dispatch.

**Option B: WASM plugin system.** New plugin concept with PLUGIN.toml manifests, a plugin registry, and generic CLI dispatch via `execute_with_input`. Risk: parallel system alongside skills.

**Option C: Merge skills and plugins.** Unify under one concept. Risk: significant refactor of the existing skill system for uncertain benefit.

## Decision

Adopt **Option B**: a dedicated WASM plugin system that coexists with skills.

### Plugin contract

A plugin is a directory containing:

```
.agentzero/plugins/<name>/
  PLUGIN.toml      # manifest
  <name>.wasm      # compiled WASM module
```

### Manifest format (PLUGIN.toml)

```toml
[plugin]
name = "brain"
version = "0.1.0"
description = "Personal LLM wiki"
runtime = "wasm"
wasm_path = "brain.wasm"

[[commands]]
name = "init"
description = "Initialize a brain vault"

[[commands]]
name = "today"
description = "Create today's daily note"
```

### WASM guest contract

Plugins export:
- `run(ptr: i32, len: i32) -> i64` — receives JSON input, returns packed `(ptr, len)` output
- `alloc(size: i32) -> i32` — bump allocator for host string passing

Plugins import from the `az` module:
- `read_file`, `write_file`, `append_file` — filesystem I/O
- `list_dir`, `create_dir`, `file_exists` — directory operations
- `now` — clock access (ISO 8601)
- `log` — audit-logged messages

All imports go through `PluginHostCallbacks` which validates paths via `PathValidator` (agentzero-core) before performing I/O.

### JSON protocol

Input (host → guest):
```json
{"action": "capture", "root": "/path/to/vault", "message": "thought"}
```

Output (guest → host):
```json
{"success": true, "output": "wiki/daily/2026-05-11.md\n- 14:32 -- thought"}
```

### Plugin registry

`PluginRegistry` scans `.agentzero/plugins/*/PLUGIN.toml`:
- `list()` — enumerate installed plugins
- `get(name)` — load a specific plugin manifest
- `find_wasm(name)` — load the WASM module bytes
- `install(source)` — copy a plugin from a local directory

### CLI integration

- `az plugin list` — show installed plugins
- `az plugin install <path>` — install from local directory
- `az plugin info <name>` — show manifest and commands
- Named subcommands (e.g., `az brain init`) dispatch through the plugin system with native fallback

### Security model

`PluginHostCallbacks` wraps a `PathValidator` anchored at the vault root extracted from the JSON input. All filesystem operations are validated:
- Path traversal blocked (canonicalization + root bounds check)
- Sensitive paths blocked (.ssh, .gnupg, .aws/credentials, .env)
- Symlink writes blocked (TOCTOU mitigation)
- Each plugin declares its required permissions in PLUGIN.toml

### Distribution

- `just build-plugin <name>` — compile Rust to wasm32-unknown-unknown
- `just install-plugin <name>` — copy to .agentzero/plugins/
- GitHub Actions workflow publishes `.wasm` + `.sha256` as release assets on tag push

### Relationship to skills

Skills and plugins serve different purposes:

| | Skills | Plugins |
|---|--------|---------|
| **Manifest** | SKILL.md (frontmatter) | PLUGIN.toml |
| **Dispatch** | `az run <name>` | Named subcommands or `az plugin run` |
| **Input** | None (fire-and-forget) | JSON via `run(input)` |
| **Output** | Exit code | JSON response |
| **Use case** | Scanners, audits, one-shot tools | Interactive workflows, command suites |
| **Storage** | `skills/` or `.agentzero/skills/` | `.agentzero/plugins/` |

Future unification is possible but not required. The plugin system is additive.

## Consequences

### Positive

- Brain ships as a WASM plugin — validates the plugin system with a real use case
- Any Rust crate can become a plugin by compiling to wasm32-unknown-unknown
- PathValidator provides defense-in-depth for plugin filesystem access
- Plugin manifests make capabilities discoverable (`az plugin info`)
- Distribution via GitHub releases with checksum verification

### Negative

- Two parallel systems (skills and plugins) with similar but distinct concepts
- Plugin development requires wasm32 target (`rustup target add wasm32-unknown-unknown`)
- JSON serialization overhead for every plugin command invocation
- WASM fuel limits may need tuning for complex plugin operations

### Future work

- `az plugin install owner/repo` — download from GitHub releases
- External subcommand dispatch — `az <plugin-name>` for any installed plugin
- `run_command` host import for plugins that need shell access (git, ripgrep)
- Plugin capability permissions enforced by policy engine
- Plugin marketplace / catalog

## References

- ADR 0012: Self-Improving Agent via WASM
- ADR 0013: WIT Adoption for Tool Interfaces
- Spec: `specs/prompts/0006-agentzero-brain-production-plugin-prompt.md`
- Plan: `specs/plans/0004-brain-plugin.md`
- pi-llm-wiki: https://github.com/zosmaai/pi-llm-wiki
