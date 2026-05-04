# ADR 0009: Capability-Based Secret Handles

## Status

Accepted

## Context

Local agents need credentials to perform useful tasks, but placing secrets in prompts, environment variables, logs, or model context creates unacceptable leakage risk.

## Decision

AgentZero represents secrets as capability handles whenever possible, such as `handle://vault/github/default`. The model sees handles and metadata, not raw secret values. Tools receive raw secret material only at execution time if policy allows the specific action.

## Consequences

The vault subsystem must expose handles, not values, to model-facing context. Audit events record handle usage without raw secret values. Shell commands do not inherit ambient secrets by default. Credential-bearing tools require explicit policy.
