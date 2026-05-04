# Prompt: Selectively Port From Existing AgentZero

You are working in AgentZero. The older AgentZero codebase is at:

```text
/Users/auser/work/rust/mine/agentzero
```

## Task

Inspect the old repo and propose a porting plan before copying code.

## Required Analysis

For each candidate module, answer:

1. What does it do?
2. Which AgentZero ADR authorizes it?
3. Does it belong in core, adapter, package, runtime, or not at all?
4. What policy boundary wraps it?
5. What audit event does it emit?
6. Can it leak secrets, PII, or private code?
7. Does it work offline?
8. What tests are needed?

## Port First

Only consider:

- policy evaluation primitives
- PII redaction utilities
- secret detection utilities
- audit logging structures
- encrypted state or vault pieces
- provider abstraction pieces
- local model detection/routing pieces
- tool permission-checking patterns
- useful CLI ergonomics

## Do Not Port Initially

- swarms
- channels
- broad provider catalog as core UX
- A2A
- MCP
- SDKs
- marketplace
- gateway
- natural-language permanent tool creation

## Output

Create or update:

```text
specs/plans/0002-port-from-existing-agentzero.md
```

Do not copy code until the plan is accepted.
