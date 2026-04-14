# Strategic Review: What to Pull In vs. Strip Out of AgentZero

## Context

AgentZero is a security-first, privacy-first, self-improving Rust AI agent runtime. 83+ sprints, 20 crates, 2500+ tests, 50+ tools. The project has grown from a CLI agent loop into a full platform with: autopilot (self-running autonomous agents), visual workflow builder, multi-agent orchestration, WASM plugins, MCP server/client, A2A protocol, 8+ channel integrations, on-device inference, and a React SPA control plane.

The core identity pillars are: **security**, **privacy**, **self-improvement**, **single-binary Rust**, **local-first**.

This document evaluates what aligns with those pillars and what doesn't.

---

## CONSIDER PULLING IN

### 1. Formal Verification / Property-Based Testing (HIGH alignment)
**Why:** A security-first project should go beyond unit tests. Property-based testing (proptest/quickcheck) for crypto, redaction, and policy evaluation would be a strong differentiator. The fuzz targets exist but only cover parsing — they don't test invariants.
**Effort:** Medium. Add proptest for: PII redaction never leaks, policy intersection always restricts, encryption round-trips.

### 2. Capability-Based Security Model (HIGH alignment)
**Why:** The current model is boolean flags on `ToolSecurityPolicy` (50+ flat booleans). This doesn't compose well. A capability-based model (like Capsicum, CloudABI, or Deno's `--allow-*`) would let you express "this agent can read files in /data but not /etc" without per-tool flags. The YAML security policy (Sprint 58) is a step toward this but lives outside the core.
**Effort:** High. Would require rethinking the security policy layer.

### 3. Reproducible Builds / Binary Attestation (HIGH alignment)
**Why:** For a security-focused tool, users should be able to verify the binary matches the source. SLSA provenance, reproducible builds, and Sigstore signing would be a strong trust signal. SBOM exists already (CycloneDX) — this is the natural next step.
**Effort:** Medium. CI/CD pipeline work, no code changes.

### 4. Differential Privacy for Memory/Analytics (HIGH alignment)
**Why:** The PII redaction layer catches known patterns (email, SSN, API keys). Differential privacy would protect against statistical inference attacks on stored memory — if someone gains access to the memory store, they can't reconstruct sensitive inputs from embeddings or frequency patterns.
**Effort:** High. Research-heavy. Applies to semantic memory embeddings especially.

### 5. Confidential Computing / TEE Support (HIGH alignment)
**Why:** If the pitch is "keeps private files off the cloud," the logical extension is "even the cloud can't see your data." SGX/TDX/SEV support for the gateway would let enterprise users run AgentZero on untrusted cloud VMs with hardware-level isolation. Pairs with Noise Protocol.
**Effort:** Very high. Hardware-dependent. Could start with documentation/design only.

### 6. WebAuthn / Passkey Authentication (MEDIUM alignment)
**Why:** The gateway currently uses bearer tokens and OTP pairing. WebAuthn/passkeys would eliminate shared secrets entirely for the dashboard. Aligns with zero-trust security posture.
**Effort:** Medium. Add to gateway auth middleware.

### 7. Local RAG Pipeline Improvements (HIGH alignment)
**Why:** Tantivy BM25 + HNSW are already in the codebase. What's missing: hybrid search (BM25 + vector with reciprocal rank fusion), re-ranking, chunk overlap control, and citation/provenance tracking. This is the self-improvement engine's memory backbone.
**Effort:** Medium. Builds on existing infra.

### 8. Agent Behavioral Guardrails / Constitutional AI Patterns (HIGH alignment)
**Why:** The current guardrails detect prompt injection and PII. Missing: output validation (did the agent actually do what was asked?), behavioral constraints (agent shouldn't send money > $X without approval), and self-consistency checks (agent shouldn't contradict its own prior statements). These are distinct from LLM-level alignment — they're runtime policy enforcement.
**Effort:** Medium-High. Ties into autopilot cap gates.

### 9. Structured Output Enforcement (MEDIUM alignment)
**Why:** `outlines-core` is already a dependency but underutilized. Enforcing JSON schema output from local models (Candle, llama.cpp) would make tool calling more reliable without cloud providers. Direct competitive advantage for local-first positioning.
**Effort:** Low-Medium. The dep exists, wire it into the local provider paths.

### 10. Offline Model Management (MEDIUM alignment)
**Why:** For a local-first tool, model management is table stakes. Currently depends on Ollama or manual downloads. An integrated model registry (list available, download, verify integrity, track versions, prune unused) would reduce friction for the offline use case.
**Effort:** Medium. HuggingFace Hub client already exists (`hf-hub`).

---

## CONSIDER STRIPPING / DEPRIORITIZING

### 1. Supabase Dependency in Autopilot (RED FLAG)
**Why:** The autopilot system (Sprint 44) has a hard dependency on Supabase PostgREST for state management. This contradicts the "zero external dependencies" and "local-first" pillars. The rest of the project uses SQLite/Turso — autopilot should too.
**Action:** Replace `SupabaseClient` with SQLite (default) or libSQL/Turso (optional sync). Follow the same pattern as `MemoryStore`: `SqliteAutopilotStore` for local, `TursoAutopilotStore` behind `memory-turso` feature for cloud sync. Tables: proposals, missions, mission_steps, events, triggers, content, cap_gate_ledger. Remove `SupabaseClient` entirely — no external database dependency.

### 2. Composio Integration (STRIP)
**Why:** Composio is a third-party SaaS for tool discovery. It requires an external API key and account. Contradicts self-contained philosophy. The AI tool selector (Sprint 40) already solves tool discovery locally.
**Action:** Remove or demote to an optional example/recipe, not a built-in tool.

### 3. Media Generation Tools (DEPRIORITIZE)
**Why:** TTS, image gen, and video gen tools (Sprint 55) call external APIs (OpenAI, ElevenLabs, etc.). They don't run locally, don't align with privacy-first, and add surface area. An AI agent runtime doesn't need to be a media production suite.
**Action:** Keep behind feature flag but don't invest further. Consider removing from default tool set.

### 4. 8+ Channel Integrations (DEPRIORITIZE maintenance)
**Why:** Telegram, Discord, Slack, Matrix, Email, IRC, Nostr, WhatsApp, SMS — each requires ongoing API maintenance as platforms change. The core value is the agent loop, not being a chatbot framework.
**Action:** Keep Telegram + Slack + Webhook as first-class. Move Discord, Matrix, IRC, Nostr, WhatsApp, SMS to a `channels-community` feature gate with clear "best-effort" maintenance commitment. Or better: make channels MCP tools so the community can maintain them independently.

### 5. Multi-Tenancy / `org_id` (EVALUATE)
**Why:** Memory note says "No multi-tenant RBAC; AgentZero is a personal/team tool, not a SaaS platform." But Sprint 39 added `org_id` to JobStore and MemoryStore, and Sprint 43 added per-org API keys. These two directions conflict.
**Action:** Decide: is AgentZero a personal tool or a platform? If personal/team, strip org_id and simplify. If platform, own it fully. The current halfway state adds complexity without commitment.

### 6. Company Templates (Content Agency, Dev Agency, SaaS Product) (EVALUATE)
**Why:** These are aspirational marketing artifacts, not core infrastructure. They risk creating support burden for workflows that don't actually work end-to-end in production.
**Action:** Keep as examples but don't treat as product features. Gate behind explicit "experimental" labeling.

### 7. Canvas/Claude Code/Codex CLI Emulation Tools (STRIP)
**Why:** Tools that simulate other AI products' UX are not core to an agent runtime. They create confusion about what AgentZero is.
**Action:** Remove. Users who want Claude Code should use Claude Code.

### 8. llama.cpp Provider (PER MEMORY: deprioritize)
**Why:** Memory says "Always use Candle for local inference; don't invest in the builtin llama.cpp provider." But Sprint 60 added `BuiltinProvider` using `llama-cpp-2` for the floating chat bubble. This contradicts the stated direction.
**Action:** Reconcile. Either commit to llama.cpp as the fast path for chat (it's more mature than Candle for inference) or remove it. Two local inference backends doubles the surface area.

### 9. Firecracker/Fleet Mode (DEPRIORITIZE — handled by `mvm`)
**Why:** Ambition to become an infrastructure platform (microVM isolation, fleet management) pulls focus from core agent reliability. Docker sandbox (Sprint 59) already provides container-level isolation. The user's separate `mvm` project (gomicrovm.com) handles Firecracker microVM isolation independently.
**Action:** Keep deprioritized. Reference `mvm` for microVM use cases. Don't duplicate that work inside AgentZero.

### 10. Hardware Tools (CPAL audio, sysinfo) (ALREADY GATED — OK)
**Why:** Already behind feature flags. Minimal impact. Leave as-is.

---

## STRATEGIC TENSIONS TO RESOLVE

### Tension 1: Local-First vs. Platform
AgentZero markets as "single binary, zero dependencies" but has grown into a platform (gateway, dashboard, workflow builder, multi-agent orchestration, Supabase integration). These aren't necessarily contradictory, but the Supabase dependency, multi-tenancy, and Agent-as-a-Service features suggest platform ambitions that conflict with the "runs on a Raspberry Pi" positioning.

**Recommendation:** Double down on local-first as the default experience. Platform features (multi-tenant, Supabase, fleet) should be explicitly optional layers that never compromise the single-binary story.

### Tension 2: Security Theater vs. Security Substance
The project has an impressive list of security features (18+ layers), but some are application-level checks that a determined attacker can bypass. The YAML security policy + iptables sandbox (Sprint 59) is the right direction — enforcement at the OS/network level. The application-level checks (PII regex, shell tokenizer) are defense-in-depth, not security boundaries.

**Recommendation:** Be explicit in docs about what's a security boundary (WASM sandbox, iptables, encryption) vs. what's defense-in-depth (PII regex, prompt injection detection). Don't oversell the regex-based protections.

### Tension 3: Self-Improvement Loop Complexity
The autopilot system (proposals, missions, cap gates, trigger engine, reaction matrix, stale recovery) is architecturally complex. It's the most ambitious part of the project and the least tested in production. The 14 reaction matrix tests and 9 autopilot loop tests are a start, but this system has emergent behavior that unit tests can't cover.

**Recommendation:** Invest in simulation/chaos testing for the autopilot loop before expanding it. A self-improving agent that goes wrong is worse than one that doesn't self-improve.

### Tension 4: Two Local Inference Stacks
Both Candle and llama.cpp are in the codebase. Candle is the stated preference (per memory) but llama.cpp is what powers the floating chat. Maintaining two is expensive.

**Recommendation:** Pick one. llama.cpp is more mature for inference. Candle is more Rust-native but GPU support is blocked (per memory: "candle-metal-kernels alpha on crates.io"). Pragmatically: llama.cpp for inference, Candle for embeddings only.

---

## PRIORITY RANKING

### Pull In (by impact/effort ratio):
1. Property-based testing for security invariants (low effort, high trust signal)
2. Replace Supabase with local SQLite in autopilot (medium effort, removes contradiction)
3. Structured output enforcement via outlines-core (low effort, high utility)
4. Reproducible builds + binary attestation (medium effort, high trust signal)
5. Local RAG improvements (medium effort, core value)

### Strip/Simplify (by complexity reduction):
1. Remove Composio integration
2. Remove Canvas/Claude Code/Codex CLI emulation tools
3. Resolve llama.cpp vs Candle (pick one for inference)
4. Replace Supabase with SQLite in autopilot
5. Demote 5+ channels to community-maintained

---

---

## DECISIONS (from user input)

### Identity: Platform with Local Default
- Keep org_id and platform features but make them **optional layers**
- Local-first is the default experience, platform is opt-in
- Supabase should become optional (SQLite default, Supabase as sync target)
- agentzero-lite remains the pure local-first story

### Inference: llama.cpp for inference, Candle for embeddings
- **llama.cpp** (`llama-cpp-2`): primary inference backend — more mature, better model support, GPU works today
- **Candle**: embeddings only — pure Rust, good for vector generation, no C++ deps for this path
- Floating chat bubble stays on llama.cpp `BuiltinProvider`
- Don't invest in Candle for chat/completion — wait for GPU crate stabilization
- Update the `candle_only` memory — now "Candle for embeddings, llama.cpp for inference"

### Pull In (all four selected):
1. **Property-based testing** — proptest for crypto, redaction, policy invariants
2. **Capability-based security** — replace 50+ boolean flags with composable capabilities
3. **Reproducible builds + attestation** — SLSA provenance, Sigstore signing
4. **Structured output (outlines-core)** — wire into Candle provider for reliable JSON

### Strip (all three):
1. **Composio integration** — remove entirely, contradicts self-contained philosophy
2. **Canvas/Claude Code/Codex CLI emulation tools** — remove, not core to agent runtime
3. **Media generation tools** — remove (TTS, image gen, video gen), external API dependency

---

## IMPLEMENTATION ROADMAP

### Phase 1: Strip (reduce surface area first)
1. Remove Composio tool + deps
2. Remove Canvas/Claude Code/Codex CLI emulation tools
3. Remove media gen tools (TTS, image gen, video gen) + `cpal`/`hound` deps
4. Keep `llama-cpp-2` as the inference backend; remove Candle completion/chat paths
5. Candle remains for embeddings only (`EmbeddingProvider` impl)
6. `cargo clippy --workspace && cargo test --workspace` — verify clean

### Phase 2: Replace Supabase with SQLite/libSQL in Autopilot
1. Create `SqliteAutopilotStore` — tables: proposals, missions, mission_steps, events, triggers, content, cap_gate_ledger
2. Implement `AutopilotStore` trait with SQLite backend (follow `SqliteMemoryStore` pattern)
3. Optional `TursoAutopilotStore` behind `memory-turso` feature for cloud sync
4. Remove `SupabaseClient` entirely — no external database dependency
5. Autopilot works fully offline by default
6. `cargo test --workspace` — verify autopilot tests pass without Supabase

### Phase 3: Pull In — Low-Effort Wins
1. Wire `outlines-core` into llama.cpp provider for structured JSON output
2. Add `proptest` to workspace dev-deps
3. Property tests for: PII redaction completeness, policy intersection monotonicity, encryption round-trip, sealed envelope integrity
4. CI: add reproducible build verification step
5. CI: add Sigstore signing to release pipeline

### Phase 4: Pull In — Capability-Based Security (larger effort)
1. Design capability model (replace `ToolSecurityPolicy` booleans)
2. Define capability types: `FileRead(path_glob)`, `FileWrite(path_glob)`, `Network(domain_glob)`, `Shell(command_list)`, `Memory(scope)`, etc.
3. Capability composition: agent capabilities = intersection of (config, policy file, parent agent)
4. Migration path: map existing boolean flags to capability grants
5. Update YAML security policy to use capabilities instead of per-tool rules

### Phase 5: Channel Simplification
1. First-class: Telegram, Slack, Webhook (maintained by core team)
2. `channels-community` feature: Discord, Matrix, IRC, Nostr, WhatsApp, SMS
3. Document community maintenance expectations

---

---

## GAPS WE WEREN'T CONSIDERING

### 1. Config Backward Compatibility (MUST ADDRESS)
When someone has `[composio]`, `[media_gen]`, `enable_claude_code = true`, or `enable_canvas = true` in their `agentzero.toml`, removing these features means their config will **fail to parse** (`serde` unknown field error). We need a graceful degradation strategy:
- Option A: `#[serde(deny_unknown_fields)]` is probably NOT set (check), so unknown keys are silently ignored — then we're fine
- Option B: If strict parsing, add a deprecation pass that warns and strips unknown sections before parsing
- **Must verify before stripping anything**

### 2. The `no_multitenant` Memory vs. "Platform with Local Default" Tension
The standing memory note says "No multi-tenant RBAC; AgentZero is a personal/team tool." But the user chose "platform with local default," which keeps `org_id`. These conflict. Need to update the `no_multitenant` memory note to reflect the new decision: **org_id stays as optional platform feature, but don't build RBAC/permissions around it**.

### 3. Autopilot Chaos/Simulation Testing
The autopilot loop (proposals → missions → cap gates → triggers → reactions) has emergent behavior that unit tests can't cover. Before expanding it or decoupling Supabase, we should add:
- Simulation harness: run N agents for M cycles, assert no runaway state
- Cap gate boundary testing: what happens when limits are hit concurrently?
- Stale recovery under load: does the 5-min scan handle 1000 missions?
This is especially important since the autopilot can self-modify through proposals.

### 4. Threat Model Staleness
The threat model doc (190 lines, written early) is probably stale after 40+ sprints. Every security change (Noise protocol, sealed envelopes, YAML policy, sandbox, A2A, MCP server mode) added attack surface that may not be documented. **Update threat model before capability-based security redesign** — otherwise we're designing capabilities against an incomplete picture.

### 5. Agent Memory Poisoning
No integrity checks on memory content. If an agent writes bad/malicious data to memory (via `memory_store` tool or semantic recall), it poisons future reasoning for all agents that query that memory. Potential mitigations:
- Content signing (agent_id + timestamp + hash)
- Anomaly detection on memory writes (perplexity scoring already exists for prompts — extend to memory)
- Memory rollback/versioning

### 6. MCP Server Mode Attack Surface
Sprint 49 exposed all 48+ tools via MCP server (stdio + HTTP). If someone connects an untrusted MCP client, they have access to every enabled tool. The capability-based security redesign should cover MCP connections — each MCP session should get its own capability set, not inherit the full server's.

### 7. A2A Protocol Trust Model
Sprint 50 added A2A (Agent-to-Agent) with `POST /a2a` accepting tasks from external agents. The current trust model is: if you can reach the endpoint, you can submit tasks. No capability negotiation, no mutual attestation. External agents could:
- Submit tasks that consume all cap gate budget
- Send payloads designed to trigger prompt injection
- Enumerate internal agent names via `tasks/get`
**Should be included in capability-based security scope.**

### 8. Binary Size Measurement (Before/After)
We should measure binary size before stripping and after each phase. This gives concrete data for the "lightweight" story and helps validate the effort. Use `cargo bloat` or `bloaty`.

### 9. The UI Has Pages for Stripped Features
The React SPA has:
- Canvas page (`ui/src/routes/canvas/`)
- Tools page shows all tools including Composio, media gen, claude_code, canvas
- Config page has sections for `[composio]` and `[media_gen]`

These need cleanup or the UI will show broken/empty sections. Not just a Rust-side change.

### 10. Supply Chain Security Beyond Sigstore
Reproducible builds + Sigstore is good, but we should also consider:
- `cargo-vet` for dependency auditing (Mozilla's approach)
- Dependency vendoring option for air-gapped deployments (aligns with local-first)
- SBOM already exists — should reference it from Sigstore attestation

### 11. Rollback/Undo for Agent Actions
Sprint 47 added regression detection (file conflict warnings). But detection without remediation is incomplete. An undo log that records agent file modifications with before/after state would let users (or the autopilot) roll back bad changes. Git handles this for code, but agents can modify non-git files.

---

## UPDATED IMPLEMENTATION ROADMAP

### Phase 0: Pre-Work (before any stripping)
1. Verify serde config parsing tolerates unknown fields (backward compat)
2. Update `no_multitenant` memory note → "org_id stays as optional platform feature"
3. Measure baseline binary sizes (`agentzero`, `agentzero-lite`)
4. Update threat model document with post-Sprint-58 attack surface

### Phase 1: Strip (reduce surface area)
1. Remove Composio tool + WASM plugin + config + UI references
2. Remove Canvas tool + gateway routes + core CanvasStore + UI page
3. Remove Claude Code, Codex CLI, Gemini CLI, OpenCode CLI tools
4. Remove media gen tools (TTS, image gen, video gen) + config
5. Clean up UI: remove canvas page, update tools page, update config page
6. Measure binary size delta
7. `cargo clippy --workspace && cargo test --workspace`

### Phase 2: Replace Supabase with SQLite/libSQL in Autopilot
1. Create `SqliteAutopilotStore` — tables: proposals, missions, mission_steps, events, triggers, content, cap_gate_ledger
2. Implement `AutopilotStore` trait with SQLite backend (follow `SqliteMemoryStore` pattern)
3. Optional `TursoAutopilotStore` behind `memory-turso` feature for cloud sync
4. Remove `SupabaseClient` entirely — no external database dependency
5. Autopilot works fully offline by default
6. `cargo test --workspace`

### Phase 3: Pull In — Low-Effort Wins
1. Wire `outlines-core` into llama.cpp provider for structured JSON output
2. Add `proptest` to workspace dev-deps
3. Property tests for: PII redaction completeness, policy intersection monotonicity, encryption round-trip, sealed envelope integrity
4. CI: add reproducible build verification step + Sigstore signing
5. Consider `cargo-vet` for dependency auditing

### Phase 4: Pull In — Capability-Based Security (larger effort)
1. **Update threat model first** — document MCP server, A2A, autopilot attack surfaces
2. Design capability model (replace `ToolSecurityPolicy` booleans)
3. Define capability types: `FileRead(path_glob)`, `FileWrite(path_glob)`, `Network(domain_glob)`, `Shell(command_list)`, `Memory(scope)`, etc.
4. **Per-MCP-session capabilities** — each MCP client gets scoped access
5. **A2A capability negotiation** — external agents declare required capabilities
6. Capability composition: agent capabilities = intersection of (config, policy file, parent agent)
7. Migration path: map existing boolean flags to capability grants
8. Update YAML security policy to use capabilities

### Phase 5: Channel Simplification
1. First-class: Telegram, Slack, Webhook
2. `channels-community` feature: Discord, Matrix, IRC, Nostr, WhatsApp, SMS
3. Document community maintenance expectations

### Phase 6: Hardening
1. Autopilot simulation harness (N agents, M cycles, assert no runaway)
2. Memory integrity checks (content signing, anomaly detection)
3. Agent action undo log for non-git file modifications

---

## Verification

After any changes:
- `cargo clippy --workspace` — 0 warnings
- `cargo test --workspace` — all pass
- `cargo build --release -p agentzero` — binary builds
- `cargo build --release -p agentzero-lite` — lite binary builds
- Feature flag combinations: `--features default`, `--features all-channels`, `--features wasm-runtime`
