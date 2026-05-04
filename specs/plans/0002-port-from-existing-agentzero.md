# Port From Existing AgentZero

## Objective

Selectively port only the existing AgentZero pieces that serve AgentZero's local-first secure harness direction.

## Port First

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
- multi-channel bots
- 37+ provider catalog as marketing or core UX
- 58+ built-in tools
- A2A
- MCP as a first-class pitch
- SDKs
- marketplace
- gateway
- natural-language permanent tool creation

## Porting Rule

Every ported module must answer:

1. Which ADR authorizes this?
2. Which policy boundary wraps it?
3. What audit event does it emit?
4. What data classifications can it process?
5. Can it run offline?
6. Can it leak secrets or PII?
7. Does it belong in core or an adapter/package?

## First Porting Milestone

Port only enough to support:

```bash
agentzero init --private
agentzero chat --local
agentzero audit report
```
