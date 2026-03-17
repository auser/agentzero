# AGENTS.md

## Purpose
Project-level operating rules for all contributors and coding agents working in this repository.

## Project Values

1. **A delight to use** — every interaction should feel like magic. Fast, responsive, beautiful CLI output, helpful errors, zero unnecessary friction. If it can be simpler, make it simpler.
2. **Easy to use** — sensible defaults, zero-config happy path, progressive disclosure of complexity. A new user should be productive in under 60 seconds. Privacy, security, and encryption should "just work" without reading docs.
3. **Safe & secure** — security is non-negotiable; fail-closed by default, encrypted at rest and in transit, sandboxed execution, privacy by default. Local-first AI as the strongest privacy guarantee; encrypted transport when cloud models are needed.
4. **Easy to develop with** — clean APIs, great plugin SDK (WASM), simple extension points. Building a plugin should take minutes, not hours. Documentation that delights developers.
5. **Extensible** — plugin architecture (WASM), provider abstraction, trait-based interfaces. Support local models, cloud models, or both with coordination.
6. **Fast** — minimal allocations, async throughout, no unnecessary overhead.
7. **Slim binaries** — server and CI binaries stay as small as possible; feature-gate optional functionality, avoid bloating the dependency tree.
8. **Idiomatic Rust** — always follow Rust best-practices; prefer generated code, builder patterns, trait-based dispatch, and macros over manual boilerplate.

## Core Principle: Privacy & Security First
Privacy and security are foundational to every decision in this project. They are not optional, not deferrable, and not someone else's concern. Every contributor and agent must consider the privacy and security implications of every change — whether it touches authentication, data handling, network calls, CLI output, logging, error messages, or any other surface. When in doubt, fail closed, encrypt by default, and ask before exposing. This principle applies at all times, in all contexts, without exception.

## Required Workflow Rules

### 1) Comprehensive tests for every functionality change
- Every feature (new or updated) must include **comprehensive** tests in the same PR — not just the minimum, but enough to give real confidence in correctness.
- Every bug fix must include a regression test that fails before and passes after the fix.
- Required minimum per change:
  - Multiple success-path tests covering primary workflows and meaningful input variations.
  - Multiple negative-path tests covering error handling, invalid inputs, and edge cases.
  - Boundary/edge-case tests where applicable (empty inputs, max values, concurrent access, etc.).
- Coverage goal: tests should exercise every significant code path introduced or modified. If a function has three branches, test all three.
- CLI commands: Every CLI command must have unit tests covering at minimum:
  - One success-path test exercising the primary workflow
  - One negative-path test exercising error handling
  - Helper/utility functions must have their own targeted tests
- Agent enforcement:
  - Agents must add/update tests for every code change they make.
  - If a test is truly not feasible, the agent must explicitly state why and propose the nearest practical regression check.

### 2) Keep sprint plan current at all times
- `specs/SPRINT.md` is the source of truth for execution status.
- When starting work:
- Mark task status from `[ ]` to `[-]`.
- When finishing work:
- Mark task status from `[-]` to `[x]`.
- Update acceptance criteria status in the same PR.
- If scope changes, update `specs/SPRINT.md` before implementation.

### 3) Definition of done enforcement
- A task is done only if:
- Code is implemented.
- Tests exist and pass.
- Docs are updated (if behavior changes).
- `specs/SPRINT.md` is updated.

### 4) Quality gates (must pass before merge)
- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- **Clippy is mandatory**: Agents must run clippy after every code change and fix all warnings/errors before committing. Do not leave clippy issues for later — they block all other work. If a pre-commit hook catches clippy failures, fix them immediately and create a new commit (do not amend).

### 5) Architecture and scope discipline
- Follow current scope in `docs/adr/0001-scope.md`.
- Do not add daemon/channels/hardware/plugin/RAG work unless explicitly scheduled in sprint docs.
- Any major scope/module expansion requires ADR update.

### 6) Crate boundary policy (major module per crate)
- When practical, each major functionality module must live in its own crate.
- Examples of major modules:
- config, provider implementation, memory backend, tools by risk domain, observability, runtime orchestration.
- Avoid large “misc infra” crates that accumulate unrelated concerns.
- Allowed exception:
- very small glue logic that would add churn if split; exception must be documented in `specs/SPRINT.md`.

### 7) Security is P0 and blocks feature work
- Security tasks in `Sprint 0` are highest priority and must be completed before non-critical expansion.
- Any new feature that increases attack surface requires:
- threat model update in **both** `docs/security/THREAT_MODEL.md` and `site/src/content/docs/security/threat-model.md`
- security tests (success + abuse/negative paths)
- explicit policy checks (fail-closed behavior)
- No merge for security-sensitive functionality without tests and policy enforcement.

### 8) Workspace dependency policy (no per-crate path wiring)
- Internal workspace crates must be declared via workspace dependencies, not direct relative paths.
- Do this:
- `agentzero-auth = { workspace = true }`
- Do not do this:
- `agentzero-auth = { path = "../../crates/agentzero-auth" }`
- Applies to all crate `Cargo.toml` files in this repository.
- Scope clarification:
- This requirement targets subcrate manifests under `crates/*/Cargo.toml` and `bin/*/Cargo.toml`.
- The root workspace `Cargo.toml` remains the single place that declares internal crate paths under `[workspace.dependencies]`.
- If a new internal crate is added:
- add it under `[workspace.dependencies]` in the root `Cargo.toml`
- reference it from subcrates with `{ workspace = true }`

### 9) Persistence policy (use `agentzero-storage`)
- All persisted application state must use `agentzero-storage`.
- Do not add new direct persistence paths in CLI/domain code using ad-hoc `std::fs` JSON/TOML writes for runtime state.
- Persistence implementations must go through storage abstractions provided by `agentzero-storage` (encrypted-at-rest where applicable).
- If migration from legacy direct-file persistence is needed, include:
- a success-path migration test
- a negative-path test for malformed/legacy payload handling

### 10) Rust best-practices (always follow)
All code must follow idiomatic Rust. These are not suggestions — they are mandatory:
- **Common crate first**: Shared types, utilities, and constants belong in `agentzero-common`. Before adding a helper to a domain crate, check whether it already exists in common or belongs there.
- **Builder pattern for complex construction**: Functions or constructors that accept more than 3–4 parameters must use a builder struct (or a dedicated config/options struct with `Default`) instead of long argument lists. Prefer structs over positional arguments for clarity.
- **Prefer autobuilders and codegen**: Use derive macros (`derive_builder`, `typed-builder`, custom derive) and code generation over hand-written boilerplate wherever possible. If a pattern can be generated, it should be generated.
- **Macros for repeated patterns**: Use `macro_rules!` (or derive macros) to eliminate repetitive patterns. Prefer macros over copy-pasting similar impl blocks across crates. If you find yourself writing the same structure more than twice, extract a macro.
- **Traits for shared behavior**: Use traits to define shared interfaces across types. Prefer trait-based dispatch over manual type switching.
- **No large match statements**: Avoid large `match` blocks that dispatch on type or variant. Refactor into trait implementations, lookup tables, or enum dispatch. A `match` with more than 5–6 arms is a code smell — break it up using traits or helper methods.
- **Plugin vs. nested implementation**: When adding new functionality, evaluate whether it belongs as a WASM plugin (isolated, user-installable, sandboxed) or as a native nested implementation (performance-critical, tightly coupled to core). Default to plugin for user-facing extensions; use native only when the plugin boundary adds unacceptable overhead or complexity.
- **Zero clippy warnings**: All code must pass `cargo clippy --workspace --all-targets -- -D warnings` with no exceptions. The pre-commit hook auto-fixes warnings via `cargo clippy --fix --allow-staged` and re-stages the changes; any warnings that cannot be auto-fixed must be resolved manually before committing. Do not `#[allow(clippy::...)]` without a justifying comment.
- **Error handling conventions**: Use `thiserror` for domain-specific error enums; use `anyhow` for ad-hoc error propagation and context. Always add `.context()` or `.with_context()` when propagating errors across crate boundaries.
- **Never use `.unwrap()` in production code**: Always use `.expect("descriptive message")` instead. `.unwrap()` is only acceptable in test code (`#[cfg(test)]`). In production paths, prefer `?` propagation or `.expect()` with a message explaining why the invariant holds.
- **Derive discipline**: Apply standard derives consistently — `Debug, Clone` on most types; add `Serialize, Deserialize` only when the type crosses a serialization boundary; add `Copy, PartialEq, Eq` on small enums.
- **Trait design**: All async traits use `#[async_trait]` and require `Send + Sync` bounds. Trait methods should return `anyhow::Result` unless a domain-specific error type is warranted.
- **Prefer `impl` blocks over free functions**: Attach behavior to the type it operates on. Use free functions only for true module-level utilities with no obvious owning type.
- **`where` clauses for readability**: Use `where` clauses (not inline bounds) when generic constraints span more than one trait bound.

### 11) Screenshots saved to `/tmp`
- When taking screenshots (browser, UI verification, etc.), always save images to the `/tmp` directory.
- Do not save screenshot files in the repository root or any tracked directory.

### 12) Keep `site/` documentation current
- Any change that affects user-facing behavior must include corresponding updates to the documentation site in `site/src/content/docs/`.
- Code-to-docs mapping — update the matching page(s) when you change:
  - CLI commands or flags → `site/src/content/docs/reference/cli-commands.md`
  - Configuration schema or defaults → `site/src/content/docs/config/`
  - Architecture or crate structure → `site/src/content/docs/architecture/`
  - Security boundaries or policies → `site/src/content/docs/security/`
  - Tool or plugin APIs → `site/src/content/docs/reference/tools.md` and `plugin-api.md`
  - Gateway endpoints → `site/src/content/docs/reference/gateway.md`
- New user-facing features require a new or updated guide in `site/src/content/docs/guides/`.
- When adding a new documentation page, update the sidebar config in `site/astro.config.mjs`.
- Agent enforcement:
  - Agents must identify affected doc pages and update them in the same PR as the code change.
  - If no documentation update is needed, the agent must explicitly state why.

### 13) Holistic problem-solving (no band-aids)
- When encountering a bug, failure, or design problem, **do not patch the immediate symptom**. Step back, identify the root cause, and propose a solution that addresses the underlying issue.
- Think like a principal systems architect:
  - Map the full blast radius of the problem — what else does it affect?
  - Identify why the current design allowed the problem to occur.
  - Propose a fix that eliminates the class of problem, not just the instance.
- Forbidden patterns:
  - Applying a quick fix to silence an error without understanding why it happens.
  - Adding special-case `if` checks or workarounds that mask deeper structural issues.
  - Fixing one symptom only to create a new one elsewhere (whack-a-mole).
  - Suppressing warnings, skipping tests, or loosening constraints to make a problem "go away."
- Required approach:
  - **Diagnose first**: read the relevant code paths, trace the data flow, understand the invariants before proposing any change.
  - **Project-wide view**: consider how the fix interacts with the rest of the system — will it hold up as the codebase evolves?
  - **Production-ready quality**: every fix should be something you'd confidently ship, not a stopgap that needs revisiting later.
  - **Explain the "why"**: when proposing a solution, articulate why this approach solves the root cause and why simpler/narrower fixes are insufficient.
- If the proper fix is large, scope it explicitly and propose a phased plan — but never substitute a band-aid for phase 1.

## Workflow Packs

A **workflow pack** is the primary unit of distribution in the marketplace. Unlike installing a single skill, channel, or plugin in isolation, a workflow pack delivers a **complete coordination graph** — everything needed for an end-to-end workflow, bundled and installable as one unit.

### What a pack contains
A workflow pack may include any combination of:
- **Coordination graph** — the orchestration definition that wires agents, skills, and tools into a directed workflow (the "brain" of the pack).
- **Agents** — one or more agent definitions with roles, system prompts, tool access policies, and delegation rules.
- **Custom skills** — skill implementations (WASM plugins or native) that the workflow depends on.
- **Tool configurations** — pre-configured tool bindings, security policies, and autonomy settings.
- **Channel adapters** — optional integrations (Slack, Discord, Telegram, webhooks) that the workflow can surface through.
- **Cron schedules** — time-triggered entry points into the coordination graph.
- **Assets & prompts** — prompt templates, few-shot examples, configuration defaults, and static assets the workflow needs.

### Design principles for packs
1. **Self-contained** — a pack must declare all of its dependencies explicitly. Installing a pack should not require the user to manually wire up additional components.
2. **Composable** — packs can depend on other packs. A "customer support" pack might compose an "email triage" sub-pack with a "knowledge retrieval" sub-pack.
3. **Overridable** — users can override any component of a pack (swap an agent's model, replace a skill implementation, adjust the coordination graph) without forking the entire pack.
4. **Versioned** — packs follow semver. Breaking changes to the coordination graph or agent contracts require a major version bump.
5. **Sandboxed** — pack execution respects the same security policies as individual tools and plugins. A pack cannot escalate beyond the permissions granted to its constituent components.

### Coordination graph
The coordination graph is the defining artifact of a pack. It specifies:
- The **nodes** (agents, skills, tools, decision points) in the workflow.
- The **edges** (data flow, delegation, conditional branching) connecting them.
- **Entry points** (user-triggered, cron-triggered, webhook-triggered, event-triggered).
- **Termination conditions** and output contracts.

Think of a pack not as "a bag of plugins" but as a **runnable workflow definition** — a first-class, shareable, installable coordination graph with all its dependencies resolved.

## Preferred PR Checklist
- [ ] Functionality implemented
- [ ] Success-path tests added
- [ ] Negative-path tests added
- [ ] `specs/SPRINT.md` task + acceptance updated
- [ ] Docs updated (`docs/COMMANDS.md`/README and `site/` webapp as needed)
- [ ] `fmt`, `clippy`, and `test` all pass
