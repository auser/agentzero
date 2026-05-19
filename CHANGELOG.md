# Changelog

All notable changes to AgentZero are documented here.

## [0.15.0] - 2026-05-19

### Bug Fixes

- Checkpoint resume now restores session approvals

### Documentation

- Add plugin system and brain wiki guides

### Features

- Wire plugin http_request to network policy + PII scan
- Wire http_request + real AgentLoop gateway handler
- Gateways, hibernation, provider extensibility
- Machine-derived encryption key — no passphrase prompt
- Read chat passphrase from AZ_PASSPHRASE env or settings.toml

### Miscellaneous

- Add deploy-docs Justfile target and release reminder

### Security

- Harden gateway approval + enforce URL allowlists
## [0.14.0] - 2026-05-12

### Bug Fixes

- Cargo fmt formatting for WAT test lines
- CI clippy and formatting issues
- Security hardening — path blocklist, TOCTOU, redaction, WASM verification

### Documentation

- Update landing page binary size stat to 8.9 MB
- Update all website docs for P0-P4 features
- Update SPRINT.md with Phase 26 (Marketplace) progress
- Update SPRINT.md with Phase 25 (Provider & Onboarding) complete
- Add ADRs 0012-0014 and az:host WIT interface spec

### Features

- WASM plugin system + brain plugin (ADR 0015)
- Extend WASM host imports with filesystem, clock, and alloc protocol
- Add richer wasm-encoder templates replacing Javy dependency
- P4 polish — random redaction, audit summary, vault import, multi-model config
- Add catalog search, trust tiers, and cross-project tool linking
- Add az bootstrap for platform-aware LLM backend setup
- Move MCP to optional --features mcp flag (ADR 0014)
- Add Anthropic Claude provider (Messages API)
- Add generate_tool as LLM-callable built-in and wire host callbacks
- Wire WasmHostCallbacks to ToolExecutor with policy enforcement
- Integrate tool generation into agent loop
- Add dynamic per-project tool registration with versioning
- Add wasm-encoder codegen for template-based WASM tool generation
- Add WASM host imports via wasmtime Linker (az::log, az::read_file, az::write_file)
- Implement approval scope tracking (Once/Session)
- Add editor-configurable coding agent (AgentLoop, ACP Chat, edit tool, print mode, models.json)

### Miscellaneous

- Release v0.14.0
- Consolidate CI from 5 jobs to 2 (#22)
- Remove .playwright-mcp from git tracking
- Update Cargo.lock for v0.3.0 version bump

### Performance

- Reduce release binary from 8.9 MB to 5.0 MB
- Reduce release binary from 13 MB to 8.9 MB

### Refactoring

- Extract redaction scanning into agentzero-core as shared utility
## [0.3.0] - 2026-05-09

### Documentation

- Reflect default-on WASM and shipped HostSupervised tier

### Features

- Add dependency-audit, license-check, and secrets-scan skills
- Add shell completions, ISO timestamps, and docs for RAG + index
- Add agentzero-index crate for semantic document querying (RAG)
- Add central skill index for short-name resolution

### Miscellaneous

- Release v0.3.0
- Rename binary from agentzero to az
- Add just install — build release and symlink to ~/.bin
## [0.2.0] - 2026-05-05

### Documentation

- Update website and sprint for registry, publish, host-supervised

### Features

- Add remote skill registry and publish via GitHub Releases
- Default wasm on + implement HostSupervised skill runtime

### Miscellaneous

- Release v0.2.0
- Add release commands and git-cliff changelog config

### Style

- Apply cargo fmt to all new modules
## [0.1.1] - 2026-05-05

### Bug Fixes

- Darken light mode text for better readability
- Landing page now properly serves at root, not Starlight docs

### Documentation

- Update SPRINT.md with Phase 19 (WASM integration) and tag v0.1.0
- Add WASM sandbox documentation across all site pages
- Add 5 new guide pages — encryption, ACP, sessions, audit, ADRs
- Update binary size to 6 MB
- Bump font sizes across landing page
- Redesign landing page — bigger type, more sections, binary size stat
- Redesign landing page with light/dark mode and glassmorphism
- Add landing page with hero and feature cards
- Add Astro Starlight documentation site

### Features

- Complete WASM production gaps — init, doctor, integration tests
- Wire WASM sandbox into skill execution pipeline
## [0.1.0] - 2026-05-05

### Features

- Add skill registry, lockfile, GitHub CI, README rewrite (Phase 18 — v0.1.0)
- Add multi-provider routing and retry logic (Phase 17)
- Wire ACP to session, add context compaction, prompts, git install (Phase 16)
- Add MCP server — AgentZero as tool provider for any MCP client (Phase 15)
- Add ACP serve command, settings loading, improved doctor (Phase 14)
- Add secret vault, content provenance, skill discovery, ACP adapter (Phase 13)
- Wire redaction pipeline, comprehensive audit, complete init (Phase 12)
- Add encrypted persistence, session resume, and skill install (Phase 11)
- Add OpenAI-compatible provider and AES-256-GCM encryption (Phase 10)
- Add persistence, history, and WASM sandbox skeleton (Phase 9)
- Add file write tool, streaming, model selection, cargo run (Phase 8)
- Add tool calling, streaming, and shell approval in chat (Phase 7)
- Add policy loader, Ollama provider, and interactive chat (Phase 6)
- Add repo-security-audit skill with external patterns (Phase 5)
- Add session engine, tool executor, and tracing (Phase 4)
- Bootstrap AgentZero workspace through Phase 3

