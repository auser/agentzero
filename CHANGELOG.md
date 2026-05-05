# Changelog

All notable changes to AgentZero are documented here.

## [0.2.0] - 2026-05-05

### Documentation

- Update website and sprint for registry, publish, host-supervised

### Features

- Add remote skill registry and publish via GitHub Releases
- Default wasm on + implement HostSupervised skill runtime

### Miscellaneous

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

