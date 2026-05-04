# ADR 0002: Local-First Model Routing

## Status

Accepted

## Context

AgentZero's product promise depends on private-data safety. Local-first must be enforceable rather than a marketing phrase. Provider choice must be governed by data sensitivity and policy, not only user preference.

## Decision

AgentZero defaults to local models. Remote model calls require policy evaluation, data classification, redaction checks, and audit logging. Secrets and credentials are never sent to remote models. PII is denied or redacted before remote model calls unless explicit policy allows otherwise.

## Consequences

Model routing must classify content before provider calls. Offline mode must make zero network calls. The provider abstraction must support local Ollama, LM Studio, vLLM, llama.cpp-compatible servers, and remote OpenAI-compatible APIs later through the same policy path.
