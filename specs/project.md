# AgentZero Project Specification

## Project Name

`AgentZero`

## Purpose

AgentZero is a local-first secure AI agent harness for private developer workflows. It lets an AI agent read, reason over, edit, and automate within a local project while enforcing policy, PII redaction, secret protection, model-routing constraints, runtime isolation, and auditable action provenance.

The project exists to make local AI agents safe enough to use with private code, credentials-adjacent workflows, PII-bearing files, security-sensitive repositories, and local automation.

## Primary User

The primary user is a developer or technical operator who wants to use AI agents on private local projects without leaking secrets, PII, private code, credentials, or sensitive operational context to untrusted tools, packages, models, networks, or editor integrations.

## Core Workflow

The first core workflow is:

```bash
agentzero init --private
agentzero chat --local
```

The user asks AgentZero to inspect a local repository, identify secret and PII exposure risks, safely summarize architecture, propose patches, and produce a redacted audit report. AgentZero defaults to local model usage, supervised tool execution, explicit policy checks, and auditable actions.

## Goals

- [ ] Create a minimal secure local agent session engine.
- [ ] Support project-local configuration under `.agentzero/`.
- [ ] Support `AGENTS.md` as guidance and `.agentzero/policy.yml` as enforceable policy.
- [ ] Provide a CLI-first interface: `init`, `chat`, `run`, `doctor`, `policy`, `audit`, `vault`.
- [ ] Provide local-first model routing with explicit remote-call policy checks.
- [ ] Detect and redact PII before remote model calls.
- [ ] Detect and block secrets before model calls, logs, or tool output exposure.
- [ ] Provide a minimal built-in tool set: read, list, search, edit proposal, shell with approval, local model call, audit, vault.
- [ ] Support first-class skills using `SKILL.md`-style capability bundles.
- [ ] Support package manifests and lockfiles before package execution.
- [ ] Define runtime isolation tiers for host, WASM, and MVM.
- [ ] Expose ACP later as an adapter, not as the core architecture.

## Non-Goals

- [ ] AgentZero is not a swarm platform.
- [ ] AgentZero is not a hosted SaaS.
- [ ] AgentZero is not a workflow DAG engine.
- [ ] AgentZero is not Fabric.
- [ ] AgentZero is not an MCP marketplace.
- [ ] AgentZero is not a generic bot platform.
- [ ] AgentZero will not launch Slack, Discord, Telegram, or browser integrations in v0.
- [ ] AgentZero will not support autonomous multi-agent swarms in v0.
- [ ] AgentZero will not support arbitrary native package execution by default.
- [ ] AgentZero will not make remote model calls with raw PII or secrets.

## Product Principles

- Local-first is enforceable, not just a preference.
- Unknown data is treated as private.
- Unknown permissions are denied.
- Untrusted content never becomes trusted instruction.
- Skills extend behavior, but policy bounds behavior.
- Packages distribute capabilities, but manifests and lockfiles define trust.
- ACP is an adapter, not the core.
- WASM is for low-risk portable tools.
- MVM is for high-risk tool execution.
- Audit logs explain why an action was allowed or blocked.
- The core must remain small enough to reason about.

## Platform Invariants

These must not be violated by implementation work:

- Every meaningful feature must align with an accepted ADR.
- Every public behavior must be documented or tested.
- Every security-sensitive boundary must be explicit.
- Every external integration must have a clear owner, failure model, and test strategy.
- Every change to architecture, data ownership, API behavior, runtime behavior, or security policy requires ADR coverage.
- No tool, skill, package, ACP session, model provider, or runtime adapter may bypass policy evaluation.
- No remote model call may receive raw secrets.
- No remote model call may receive raw PII unless an explicit policy allows it.
- No package may execute install scripts by default.
- No skill may auto-escalate permissions.
- No audit log may contain raw secrets.
- Offline mode must perform zero network calls.
- MVM guests must not receive host home-directory access by default.
- ACP sessions must not receive whole-workspace context by default.

## System Guarantees

- AgentZero fails closed when classification, permissions, runtime trust, or model destination are unknown.
- AgentZero stores audit logs locally and redacts sensitive fields.
- AgentZero can explain why an action was allowed, denied, redacted, or escalated.
- AgentZero can run without network access in offline mode.
- AgentZero can route sensitive work to local models only.
- AgentZero can propose patches without directly editing project files unless policy and approval allow writes.
- AgentZero can load compatible external skills, but execution requires secure-mode policy metadata or explicit user approval.

## Failure Model

AgentZero fails closed for:

- unknown package permissions
- unknown data classification
- unknown runtime safety
- unknown model destination
- failed redaction
- failed secret scanning
- failed policy loading
- failed audit initialization
- denied filesystem path canonicalization
- missing lockfile for package execution
- ACP context requests outside profile scope

AgentZero may retry:

- local model connection detection
- non-mutating file reads
- audit report generation
- package metadata fetches, if network policy allows
- MVM startup, if the user approved MVM execution

AgentZero must not retry automatically:

- shell commands with side effects
- package install scripts
- network calls that may exfiltrate private data
- remote model calls containing private context
- credential-bearing tool calls

## Initial Scope

- [ ] Initialize repository from `auser/project-template`.
- [ ] Replace starter project specification with AgentZero specification.
- [ ] Add ADR pack for secure local agent architecture.
- [ ] Add first implementation plan.
- [ ] Add sprint scope for documentation and architecture bootstrap.
- [ ] Create Rust workspace only after ADRs are accepted.
- [ ] Implement CLI skeleton.
- [ ] Implement policy model.
- [ ] Implement audit event model.
- [ ] Implement redaction and secret scanning interfaces.
- [ ] Implement local model provider abstraction.
- [ ] Implement minimal chat loop.
- [ ] Implement safe repo-security-audit demo.

## Success Criteria

The project is ready for implementation when:

- [ ] `specs/project.md` is complete.
- [ ] ADRs 0001-0010 exist and are internally consistent.
- [ ] `specs/plans/0001-bootstrap-agentzero.md` defines the first build sequence.
- [ ] `specs/SPRINT.md` references active plan and backlog files.
- [ ] `just ci` passes.

The first release is successful when:

- [ ] `agentzero init --private` creates `.agentzero/` config.
- [ ] `agentzero chat --local` starts a local supervised session.
- [ ] AgentZero reads and searches a repo without network access.
- [ ] AgentZero detects representative PII and secrets.
- [ ] AgentZero blocks remote calls with secrets.
- [ ] AgentZero redacts PII before allowed remote calls.
- [ ] AgentZero produces a human-readable audit report.
- [ ] AgentZero supports at least one built-in skill: `repo-security-audit`.
- [ ] AgentZero passes security regression fixtures for malicious skills, prompt injection, and secret exfiltration attempts.
