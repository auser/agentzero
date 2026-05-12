# Build a Production-Ready AgentZero Personal Brain Plugin

You are working in the AgentZero repository.

Your task is to design and implement a **production-ready AgentZero plugin** for managing a Karpathy-style personal LLM wiki.

This must be a real plugin, not a toy script, not a one-off helper, and not a loose collection of prompts.

The plugin should provide a safe, boring, local-first command layer around a portable Markdown knowledge base.

The plugin must not own the knowledge. The knowledge remains plain Markdown on disk.

---

## Product Goal

Create a plugin that lets a user manage a personal work brain with commands like:

```bash
agentzero brain init
agentzero brain today
agentzero brain capture "..."
agentzero brain ingest raw/inbox/some-note.md
agentzero brain query "what have I decided about Fabric?"
agentzero brain ask "what should I work on today?"
agentzero brain review
agentzero brain weekly
agentzero brain health
agentzero brain checkpoint
agentzero brain index
```

The plugin should feel like a reliable developer tool:

- predictable
- testable
- local-first
- Markdown-native
- Git-safe
- Obsidian-compatible
- qmd-aware, but not qmd-dependent
- agent-friendly, but not magically autonomous
- clear about what it changes

---

## Core Philosophy

The plugin is a **workflow/router layer**, not a note app.

The vault structure is the source of truth:

```txt
brain/
  AGENTS.md
  CLAUDE.md
  README.md
  Justfile
  .agentzero-brain.toml
  raw/
    inbox/
    sources/
    assets/
  wiki/
    index.md
    log.md
    daily/
    weekly/
    projects/
    areas/
    decisions/
    people/
    sources/
    reports/
    archive/
  prompts/
    claude/
    maintenance/
    workflows/
  templates/
```

The durable knowledge lives in `wiki/`.

Raw input lives in `raw/`.

Agents may compile raw information into wiki notes, but raw files must never be silently mutated.

---

## Production Requirements

This plugin should be implemented as a production-quality AgentZero plugin following the repository's existing conventions.

Before coding, inspect the repo and identify:

1. Plugin architecture.
2. CLI command dispatch.
3. Config loading pattern.
4. Error handling style.
5. Logging/tracing style.
6. Test conventions.
7. Packaging/release conventions.
8. Existing filesystem or tool abstractions.
9. Model/session invocation APIs, if any.
10. How plugins register tools, commands, capabilities, or permissions.

Do not invent a parallel architecture if the repo already has one.

Use the repository's established patterns unless they are clearly unsuitable.

---

## Plugin Name

Prefer:

```txt
agentzero-brain
```

or, if plugins are named by command namespace:

```txt
brain
```

Use the existing plugin naming convention if present.

---

## Configuration

The plugin must support a configuration file at the vault root:

```txt
.agentzero-brain.toml
```

Create it during `brain init` if missing.

Example:

```toml
[vault]
root = "."
raw_dir = "raw"
wiki_dir = "wiki"
prompts_dir = "prompts"
templates_dir = "templates"

[daily]
timezone = "local"
date_format = "%Y-%m-%d"
time_format = "%H:%M"

[search]
backend = "auto" # auto | qmd | ripgrep
qmd_collection_wiki = "brain-wiki"
qmd_collection_raw = "brain-raw"
max_results = 12

[git]
require_checkpoint_for_broad_changes = true
auto_commit = false
commit_message_prefix = "Brain"

[agent]
mode = "prompt" # prompt | invoke
write_answers = false

[safety]
raw_is_immutable = true
allow_destructive = false
max_file_bytes_for_auto_edit = 120000
```

The plugin should also allow command-line overrides where appropriate.

If config is missing, use safe defaults.

---

## Vault Invariants

The plugin must enforce these invariants:

1. `raw/` contains immutable source material.
2. `wiki/` contains agent-maintained compiled knowledge.
3. `prompts/` contains reusable workflow prompts.
4. `templates/` contains note templates.
5. Generated notes use simple Markdown and YAML frontmatter.
6. No command should delete user content by default.
7. No command should overwrite existing files unless `--force` is passed.
8. Raw files must never be modified by ingest/review commands.
9. Broad changes should require or recommend a Git checkpoint.
10. All ingest/review/weekly/health operations append an entry to `wiki/log.md`.
11. All generated knowledge should preserve source references where possible.
12. Uncertainty must be explicit.
13. The plugin must work without Obsidian.
14. The plugin must remain Obsidian-compatible.
15. The plugin must work without qmd.
16. The plugin must work without network access.

---

## Commands

Implement the following commands.

### `brain init`

Initialize a vault.

Behavior:

- Create canonical directories.
- Create starter files if missing.
- Create `.agentzero-brain.toml` if missing.
- Never overwrite existing files unless `--force`.
- Print a clear summary of created/skipped files.

Required directories:

```txt
raw/inbox/
raw/sources/
raw/assets/
wiki/daily/
wiki/weekly/
wiki/projects/
wiki/areas/
wiki/decisions/
wiki/people/
wiki/sources/
wiki/reports/
wiki/archive/
prompts/claude/
prompts/maintenance/
prompts/workflows/
templates/
```

Required files:

```txt
AGENTS.md
CLAUDE.md
README.md
Justfile
.agentzero-brain.toml
wiki/index.md
wiki/log.md
wiki/projects/index.md
templates/daily.md
templates/project.md
templates/area.md
templates/decision.md
templates/source.md
prompts/claude/07-end-of-day-review.md
prompts/claude/08-weekly-review.md
prompts/claude/12-vault-health-report.md
prompts/claude/13-what-should-i-work-on.md
```

Options:

```bash
agentzero brain init --root ~/brain
agentzero brain init --force
agentzero brain init --dry-run
```

Acceptance criteria:

- Idempotent.
- Safe on an existing vault.
- No accidental overwrites.
- Good test coverage.

---

### `brain today`

Create or print today's daily note path:

```txt
wiki/daily/YYYY-MM-DD.md
```

Behavior:

- Use `templates/daily.md`.
- If the file exists, do not overwrite.
- Print the path.
- Optional: `--open` opens in `$EDITOR` or via Obsidian URI if supported later.

Options:

```bash
agentzero brain today
agentzero brain today --date 2026-05-11
agentzero brain today --open
```

Acceptance criteria:

- Correct local date behavior.
- Configurable date format.
- Does not overwrite existing note.
- Creates parent directories if missing.

---

### `brain capture "message"`

Append a timestamped bullet to today's daily note under `## Capture`.

Example output in the daily note:

```md
- 14:32 — Fabric should stay stateless; Socialite owns durable research results.
```

Behavior:

- Create today's note if missing.
- Insert under `## Capture`.
- If the heading is missing, add it.
- Preserve the rest of the file.
- Print the note path and appended line.

Options:

```bash
agentzero brain capture "message"
agentzero brain capture --date 2026-05-11 "message"
agentzero brain capture --section "Tasks" "message"
```

Acceptance criteria:

- Handles missing heading.
- Handles empty note.
- Handles Unicode.
- Does not corrupt Markdown.

---

### `brain ingest <path>`

Prepare or perform ingest of a raw file.

Behavior:

- Validate that the path exists.
- Warn if the source is outside `raw/`.
- Never modify the source file.
- Generate an ingest prompt using `AGENTS.md`, `CLAUDE.md`, and the selected source path.
- Either:
  - print the prompt to stdout,
  - save the prompt under `wiki/reports/ingest-YYYY-MM-DD-HHMMSS.md`,
  - or invoke the active AgentZero model if the repo has a stable model/session API.

Initial production version may default to prompt generation rather than automatic model writes.

The prompt should instruct the agent to:

- classify the raw file
- create/update cleaned wiki notes
- preserve source references
- update `wiki/log.md`
- avoid invented facts
- keep diffs small

Options:

```bash
agentzero brain ingest raw/inbox/fabric-thoughts.md
agentzero brain ingest raw/inbox/fabric-thoughts.md --save-prompt
agentzero brain ingest raw/inbox/fabric-thoughts.md --invoke
agentzero brain ingest raw/inbox/fabric-thoughts.md --dry-run
```

Acceptance criteria:

- Source file remains byte-identical.
- Prompt is deterministic enough for tests.
- Log entry is written only when appropriate.
- Path traversal is rejected.

---

### `brain query "question"`

Search the vault.

Behavior:

1. Use qmd if configured and available.
2. Otherwise use ripgrep if available.
3. Otherwise use a simple built-in Markdown text scan.
4. Search `wiki/` first.
5. Search `raw/` second only if requested or if there are too few wiki results.
6. Return paths, snippets, and basic ranking information.

Options:

```bash
agentzero brain query "Fabric stateless orchestration"
agentzero brain query "Fabric" --raw
agentzero brain query "Fabric" --backend ripgrep
agentzero brain query "Fabric" --limit 20
agentzero brain query "Fabric" --json
```

Acceptance criteria:

- Works without qmd.
- Works without ripgrep.
- Does not modify files.
- Returns deterministic JSON with `--json`.
- Handles empty results gracefully.

---

### `brain ask "question"`

Answer a question using retrieved context.

Behavior:

1. Run `brain query`.
2. Build a compact context bundle.
3. Generate or invoke an agent prompt.
4. Require citations to source file paths.
5. Do not modify files unless `--write` is explicitly passed.

If model invocation is unavailable, save or print a ready-to-run prompt.

Options:

```bash
agentzero brain ask "What have I decided about Fabric webhooks?"
agentzero brain ask "What should I work on today?" --save-prompt
agentzero brain ask "What should I work on today?" --invoke
agentzero brain ask "Summarize MVM drift decisions" --write wiki/reports/mvm-drift-answer.md
```

Acceptance criteria:

- Does not hallucinate unsupported facts in generated prompt instructions.
- Always includes source paths in the context bundle.
- Does not mutate the vault by default.

---

### `brain review`

End-of-day review.

Behavior:

- Read today's daily note.
- Generate or invoke a review prompt.
- Extract durable information into the wiki.
- Append to `wiki/log.md`.
- Prefer small, surgical changes.

The prompt should ask the agent to update:

```txt
wiki/projects/
wiki/areas/
wiki/decisions/
wiki/projects/index.md
wiki/index.md
wiki/log.md
```

Safety:

- Never delete daily notes.
- Never delete raw notes.
- Do not rewrite the entire wiki.
- Require/recommend Git checkpoint before broad edits.

Options:

```bash
agentzero brain review
agentzero brain review --date 2026-05-11
agentzero brain review --save-prompt
agentzero brain review --invoke
agentzero brain review --dry-run
```

Acceptance criteria:

- Works in prompt-only mode.
- Validates daily note exists.
- Gives helpful error if no daily note exists.
- No destructive operations by default.

---

### `brain weekly`

Create or update a weekly review note:

```txt
wiki/weekly/YYYY-WW.md
```

Behavior:

- Look at daily notes from the last 7 days.
- Generate or invoke weekly review prompt.
- Include:
  - Summary
  - Progress
  - Decisions Made
  - Active Projects
  - Blocked or Stale Projects
  - Open Questions
  - Next Week Focus
  - Notes Needing Cleanup

Options:

```bash
agentzero brain weekly
agentzero brain weekly --week 2026-W20
agentzero brain weekly --save-prompt
agentzero brain weekly --invoke
```

Acceptance criteria:

- Handles missing daily notes gracefully.
- Does not fabricate progress.
- Produces deterministic target path.

---

### `brain health`

Create a vault health report:

```txt
wiki/reports/vault-health.md
```

Detect:

- orphan notes
- broken wikilinks
- duplicate filenames
- missing frontmatter
- oversized notes
- stale project notes
- raw files not yet ingested
- decisions missing context
- projects missing next actions
- missing index links

Options:

```bash
agentzero brain health
agentzero brain health --json
agentzero brain health --fix
agentzero brain health --dry-run
```

Behavior:

- Default mode reports only.
- `--fix` may perform safe, low-risk fixes.
- Never deletes content.

Acceptance criteria:

- Report-only default.
- JSON output for tests/automation.
- Stable diagnostics.

---

### `brain checkpoint`

Run safe Git checkpointing.

Behavior:

- Check if vault is a Git repo.
- If not, suggest `git init`; do not initialize unless `--init`.
- Show status.
- Commit changes if present.
- Print clean status if no changes.

Options:

```bash
agentzero brain checkpoint
agentzero brain checkpoint --message "Daily brain update"
agentzero brain checkpoint --init
agentzero brain checkpoint --dry-run
```

Acceptance criteria:

- Handles non-Git repo safely.
- Handles no changes.
- Handles Git failure with clear errors.
- Does not swallow command failures.

---

### `brain index`

Update qmd or search index.

Behavior:

- Detect qmd.
- If qmd exists, update configured collections.
- If qmd is missing, print installation/setup guidance and fall back gracefully.
- Do not require qmd.

Options:

```bash
agentzero brain index
agentzero brain index --backend qmd
agentzero brain index --dry-run
```

Acceptance criteria:

- qmd optional.
- No hard failure if qmd unavailable unless explicitly requested.
- Clear diagnostics.

---

## qmd Integration

qmd is optional.

The plugin should:

- detect whether `qmd` is on PATH
- allow `search.backend = "auto" | "qmd" | "ripgrep"`
- use qmd when available and configured
- fall back to ripgrep or built-in search
- not hardcode qmd behavior without inspecting/validating the CLI

Do not make qmd a required dependency.

The plugin may create helper instructions for users to configure qmd collections:

```txt
brain-wiki = compiled durable knowledge
brain-raw = raw source material
```

---

## Obsidian Compatibility

The vault must remain usable directly in Obsidian.

Rules:

- Use Markdown files.
- Use wikilinks where useful.
- Use simple YAML frontmatter.
- Avoid hidden proprietary state.
- Do not require an Obsidian plugin for core operation.
- Do not create non-portable database state in the vault.

Suggested frontmatter:

```yaml
---
type:
status:
created:
updated:
tags: []
---
```

Useful `type` values:

```txt
daily
weekly
project
area
decision
source
report
prompt
```

Useful `status` values:

```txt
draft
active
stable
stale
archived
```

---

## Security and Safety Requirements

This plugin operates on a user's personal knowledge base. Treat data loss and silent corruption as serious bugs.

Required protections:

1. No destructive default behavior.
2. No silent overwrites.
3. Validate paths stay inside the configured vault root unless explicitly allowed.
4. Prevent path traversal.
5. Do not follow symlinks for write operations unless explicitly configured.
6. Never mutate files under `raw/` during ingest/review.
7. Never send data to external services unless the user invokes a model/tool path that clearly does so.
8. Make prompt-only mode available.
9. Show changed file paths after any write operation.
10. Prefer atomic writes where possible.
11. Use temp files and rename for file writes where appropriate.
12. Preserve existing newline style when reasonable.
13. Log operations to `wiki/log.md`.

---

## Error Model

Use clear typed errors or the repo's standard error system.

At minimum distinguish:

- config error
- path error
- vault not initialized
- command dependency missing
- git error
- template error
- search backend error
- agent invocation unavailable
- permission error
- validation error

Errors should be actionable.

Bad:

```txt
failed
```

Good:

```txt
Vault is not initialized at /path. Run `agentzero brain init --root /path` first.
```

---

## Observability

Use the repository's logging/tracing conventions.

Commands should support:

```bash
--verbose
--quiet
--json
--dry-run
```

where appropriate.

All write operations should print a concise summary:

```txt
Created:
  wiki/daily/2026-05-11.md

Updated:
  wiki/log.md

Skipped:
  AGENTS.md already exists
```

---

## Testing Requirements

Add tests following the repo's conventions.

Required test areas:

### Init

- creates canonical directories
- creates starter files
- does not overwrite existing files
- respects custom root
- supports dry-run

### Today

- creates correct daily path
- does not overwrite existing daily note
- respects explicit date
- uses template

### Capture

- creates today's note if missing
- appends under `## Capture`
- creates heading if missing
- handles Unicode
- preserves file content

### Ingest

- validates source path
- refuses path traversal
- does not modify raw source
- generates expected prompt
- handles outside-raw warning

### Query

- uses qmd when mocked available
- falls back to ripgrep
- falls back to built-in search
- supports JSON output
- handles empty results

### Health

- detects broken wikilinks
- detects raw files not ingested
- reports missing frontmatter
- does not fix by default

### Checkpoint

- handles non-Git repo
- handles clean repo
- handles changed repo
- surfaces Git errors

### Safety

- raw immutability
- no overwrite without force
- path traversal rejected
- dry-run does not write

---

## Documentation Requirements

Add production-quality docs.

Required docs:

1. README section or plugin doc page.
2. Command reference.
3. Vault structure explanation.
4. qmd integration guide.
5. Obsidian setup guide.
6. Safety model.
7. Examples.
8. Troubleshooting.

Example usage section:

```bash
agentzero brain init --root ~/brain
cd ~/brain
agentzero brain today
agentzero brain capture "Need to decide whether MVM drift belongs in mvm or mvmd."
agentzero brain query "MVM drift"
agentzero brain review --save-prompt
agentzero brain checkpoint --message "Daily brain update"
```

---

## Packaging and Release Quality

The plugin should be ready to ship.

Ensure:

- formatting passes
- linting passes
- tests pass
- docs are included
- no TODO stubs in production code
- no panics for user input
- no hardcoded absolute paths
- no hidden network calls
- no broad filesystem writes outside the vault
- no unbounded scans without reasonable limits
- no dependency on a specific user's vault
- no agent/model dependency for basic commands

If the repo has CI commands, run them.

If there is a standard `just ci`, use it.

---

## Implementation Strategy

Implement in vertical slices.

### Slice 0: Repo Inspection

Before coding, report:

1. Where plugin code should live.
2. Existing CLI command architecture.
3. Existing plugin registration architecture.
4. Config conventions.
5. Test conventions.
6. Proposed files to create/modify.
7. Risks or unknowns.

### Slice 1: Core Vault Operations

Implement:

- config loading
- path validation
- safe writes
- `brain init`
- `brain today`
- `brain capture`

Tests required before moving on.

### Slice 2: Search

Implement:

- search trait/interface
- qmd detection
- ripgrep fallback
- built-in fallback
- `brain query`
- `brain index`

Tests required before moving on.

### Slice 3: Prompt/Agent Workflows

Implement:

- prompt rendering
- `brain ingest`
- `brain ask`
- `brain review`
- `brain weekly`

Use prompt-only mode first if model invocation is not stable.

Tests required before moving on.

### Slice 4: Health and Git

Implement:

- wikilink scanning
- health diagnostics
- `brain health`
- Git helper
- `brain checkpoint`

Tests required before moving on.

### Slice 5: Docs and Polish

Implement:

- docs
- examples
- troubleshooting
- final CI pass

---

## Non-Goals for First Production Version

Do not build:

- a vector database
- a web UI
- an Obsidian plugin UI
- an autonomous background daemon
- automatic scheduled rewriting
- cloud sync
- multi-user collaboration
- complex ontology/tagging system
- custom storage format
- mandatory qmd dependency
- mandatory LLM provider dependency

---

## Future Extensions

Document but do not implement unless trivial:

- MCP server for the brain plugin
- Obsidian URI integration
- `brain doctor`
- `brain diff`
- `brain undo`
- `brain sources pending`
- `brain decisions list`
- `brain projects stale`
- `brain ask --write`
- plugin marketplace packaging
- scheduled reviews
- privacy modes
- redaction rules
- encrypted raw sources
- multi-vault profiles

---

## Design Tone

This should feel like a reliable systems tool, not a productivity toy.

Prioritize:

- boring correctness
- small diffs
- local-first behavior
- explicit paths
- explicit logs
- Markdown portability
- Git safety
- clean tests
- clean errors
- composability
- minimal hidden state

---

## First Response Required

Do not start coding immediately.

First inspect the repo.

Then respond with:

1. Where this plugin should live.
2. Which files need to be created or modified.
3. How command dispatch should work.
4. How plugin registration should work.
5. How config should be loaded.
6. How search backends should be abstracted.
7. How prompt-only mode should work.
8. How tests should be structured.
9. Any missing repo context.
10. A minimal production implementation plan.

After that, implement Slice 1 only.

Do not proceed to Slice 2 until Slice 1 tests pass.

## Additional Required Production Commands

Also implement or stub with clear errors:

- `brain status`
- `brain diff`
- `brain sources pending`
- `brain sources list`
- `brain doctor`

These are required for production usability.

Do not implement destructive `brain undo` yet unless it is only advisory or requires explicit confirmation.

Add source hashing and an ingest ledger before implementing automatic ingest writes.