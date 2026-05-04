# ADR 0004: Skills as First-Class Capability Bundles

## Status

Accepted

## Context

Skills provide Pi-like extensibility without bloating the core. They also let AgentZero interoperate with the wider agent skill ecosystem. However, skills can influence model behavior and may contain executable helpers, so AgentZero must not treat them as inherently trusted.

## Decision

AgentZero supports `SKILL.md`-style skills as first-class capability bundles. Skills may contain instructions, references, assets, prompts, helper scripts, and policy metadata. AgentZero supports compatibility mode for existing skills, but secure execution requires AgentZero policy metadata or explicit user approval.

## Consequences

Skill loading must use progressive disclosure. Skill execution must be permissioned. Skills may be instruction-only, WASM-backed, MVM-backed, or denied. Unknown or compatible-only skills are loaded as untrusted content and cannot escalate permissions silently.
