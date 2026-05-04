# AgentZero Security Model

## Security Goal

AgentZero must let a local AI agent work with private developer data without leaking secrets, PII, private code, credentials, or sensitive operational context to untrusted tools, models, packages, runtimes, or networks.

## Trust Boundaries

AgentZero separates context and actions into explicit trust zones:

1. Trusted user instructions
2. Trusted project policy
3. Trusted accepted ADRs
4. Trusted AgentZero core code
5. Compatible but untrusted skill instructions
6. Untrusted document content
7. Untrusted tool output
8. Untrusted network content
9. Untrusted package code
10. Untrusted runtime guest output

Untrusted content must never become trusted instruction.

## Data Classifications

AgentZero classifies content as:

- `public`
- `internal`
- `private`
- `pii`
- `secret`
- `credential`
- `regulated`
- `unknown`

Unknown content is treated as `private`.

## Model Routing

Model routing is governed by data classification:

- `public`: remote models may be allowed by policy.
- `internal`: remote models require explicit policy.
- `private`: remote models require redaction or explicit policy.
- `pii`: remote models are denied unless policy allows redacted form.
- `secret`: remote models are denied.
- `credential`: remote models are denied.
- `unknown`: remote models are denied until classified or redacted.

## Permission Model

Every action is evaluated against policy before execution:

- file read
- file write
- shell command
- network request
- model call
- secret handle usage
- package install
- package execution
- skill load
- runtime launch
- ACP context request
- MVM mount
- WASM host call

No execution path may bypass policy.

## Approval Scopes

User approvals are scoped:

- `once`: exact action only
- `session`: matching action shape for current session
- `project`: matching action shape for current project
- `package`: requested permission for package version
- `never`: deny and remember

## Egress Control

All network access must pass through policy, audit, and classification checks. Egress includes:

- HTTP requests
- DNS requests
- git remotes
- package downloads
- MCP servers
- browser automation
- telemetry
- crash reporting
- model provider calls

Offline mode denies all egress.

## Secrets Model

Secrets are represented as handles where possible:

```text
handle://vault/github/default
```

The model must not see raw secret values. Tools receive secret material only at execution time if policy allows.

## Runtime Isolation

Runtime options are:

- `none`: instruction-only skills and prompt templates
- `host-readonly`: safe host tools that cannot mutate
- `host-supervised`: host execution requiring user approval
- `wasm-sandbox`: low-risk portable tools with explicit host calls
- `mvm-microvm`: high-risk tools, package installs, Python/Node/native execution, MCP servers, browser automation
- `deny`: action not allowed

## Audit Requirements

Every meaningful action produces an audit event containing:

- action kind
- requested capability
- data classification
- policy decision
- reason
- runtime
- skill/package involved
- model provider, if any
- redactions applied
- approval scope, if any
- output summary

Raw secrets must never appear in audit logs.

## Safe Failure

AgentZero fails closed when uncertain.

- Unknown data classification: treat as private.
- Unknown permission: deny.
- Unknown runtime safety: deny or require MVM.
- Unknown model destination: deny.
- Failed redaction: deny remote call.
- Failed audit initialization: deny session start.
- Failed policy load: deny privileged actions.
