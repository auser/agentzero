---
title: Release Process
description: Versioning and changelog process used for tagged releases.
---

This document defines the versioning and changelog process used for tagged releases.

## Release History

### [0.10.0] — 2026-03-23

#### Added

- **NL goal decomposition** — `GoalPlanner::plan()` decomposes natural language goals into multi-agent DAGs with per-node `tool_hints`. The planner sends the goal + available tool catalog to the LLM and returns a `PlannedWorkflow`.
- **`HintedToolSelector`** — New tool selector combining explicit hints (from goal planner), recipe matches (from catalog learning), and keyword fallback. Selection priority: hints → recipes → TF-IDF.
- **Dynamic tools** — Runtime-created tools with 4 execution strategies: Shell (`{{input}}` substitution), HTTP (endpoint calls), LLM (specialized system prompts), and Composite (tool chaining). Persist encrypted in `.agentzero/dynamic-tools.json`. Export/import for sharing between instances.
- **`tool_create` tool** — LLM-callable tool for creating dynamic tools mid-session from natural language descriptions. Actions: `create`, `list`, `delete`, `export`, `import`. Gated by `enable_dynamic_tools` config.
- **`ToolSource` trait** — Enables mid-session tool registration. `Agent.build_tool_definitions()` merges static tools with dynamically registered tools on each iteration.
- **NL agent definitions** — `agent_manage create_from_description` derives name, system prompt, keywords, allowed tools, and suggested schedule from a plain English description. Includes existing agents in prompt for dedup awareness.
- **Tool catalog learning** — `RecipeStore` records successful tool combos after agent/swarm runs. Jaccard similarity matching on goal keywords boosts previously successful tools for similar future goals. Persists encrypted in `.agentzero/tool-recipes.json`.
- **`build_provider_from_config()`** — Public helper for lightweight LLM callers (goal planner, tool_create) that need a provider without the full agent runtime.
- **Swarm CLI wiring** — `agentzero swarm "goal"` now calls `GoalPlanner::plan()` instead of wrapping the goal in a single agent. Each agent node gets a `HintedToolSelector` with its `tool_hints`.
- **`enable_dynamic_tools`** — New config flag in `[agent]` section (default: `false`). When enabled, loads persisted dynamic tools at startup and registers the `tool_create` tool.

#### Changed

- `PlannedNode` gains `tool_hints: Vec<String>` field (backward-compatible via `#[serde(default)]`).
- `Agent` gains `extra_tool_source: Option<Arc<dyn ToolSource>>` for mid-session tool discovery.
- `RuntimeExecution` gains `dynamic_registry` field for tool persistence lifecycle.
- `AgentManageTool` gains optional `provider` for LLM-based agent derivation.
- `ToolSecurityPolicy` gains `enable_dynamic_tools: bool`.
- Workspace version bumped to `0.10.0`.

---

### [0.4.0] — 2026-03-06

#### Added

- **HTTP registry fetch** — `az plugin install --url <https://...>` and `az plugin refresh --registry-url <https://...>` now accept `https://` and `http://` URLs in addition to `file://`. The registry index loader and refresher both support remote URLs.
- **Plugin dependency resolution** — `PluginManifest` has an optional `dependencies: Vec<PluginDependency>` field (defaults to empty). Each `PluginDependency` carries an `id` and a `version_req` semver string. Running `az plugin install --registry-url <url>` resolves and installs all transitive dependencies before the top-level plugin. Circular dependency chains are detected and reported as errors.
- **Audio input** — User messages may contain `[AUDIO:/path/to/file.wav]` markers. The agent transcribes each marker via a configurable OpenAI-compatible endpoint before forwarding the message to the LLM. Supported formats: `flac`, `mp3`, `mp4`, `m4a`, `ogg`, `opus`, `wav`, `webm` (max 25 MB). If `[audio] api_key` is not set, markers are stripped silently with a warning. Default endpoint: Groq Whisper (`whisper-large-v3`).
- **`[audio]` config section** — New `agentzero.toml` section with `api_url`, `api_key`, `language`, and `model` fields.

#### Changed

- Workspace version bumped to `0.4.0` across all publishable crates.

---

## Versioning Policy

- Use Semantic Versioning: `MAJOR.MINOR.PATCH`.
- During this stage, all publishable crates in this workspace are versioned in lockstep.
- Pre-release tags (for example `1.2.0-rc.1`) are allowed when needed.

## Changelog Policy

- `CHANGELOG.md` is required and must contain:
  - `## [Unreleased]` section for in-flight work.
  - A versioned section for each release in the form:
    - `## [X.Y.Z] - YYYY-MM-DD`
- Every user-visible behavior change should be listed under `Added`, `Changed`, or `Fixed`.

## Pre-Release Checklist

1. Ensure all intended changes are merged.
2. Run quality gates:
   - `cargo fmt --all`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - `scripts/run-coverage.sh --output-dir coverage` (requires `cargo-llvm-cov`)
3. Verify release metadata and changelog entry for the target version:
   - `scripts/verify-release-version.sh --version X.Y.Z`
4. Tag and publish using CI release workflow.

## GitHub Workflows

- `ci.yml`
  - Pull request and push validation (`fmt`, `clippy`, `test`, security checks).
- `cd.yml`
  - Continuous delivery on `main`: runs quality checks and uploads release-build artifacts for Linux/macOS/Windows.
- `release.yml`
  - Triggered by `v*.*.*` tags (or manual dispatch with version input).
  - Verifies changelog/version consistency, builds artifacts on Linux/macOS/Windows, and publishes GitHub Release assets.

## Practical Notes

- If you bump crate versions, keep all workspace crate versions aligned.
- Keep `Unreleased` at the top and move entries into the new release heading at cut time.
