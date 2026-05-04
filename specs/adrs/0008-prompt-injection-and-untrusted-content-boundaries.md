# ADR 0008: Prompt Injection and Untrusted Content Boundaries

## Status

Accepted

## Context

AgentZero will process content that may contain malicious instructions. Prompt injection is a first-class threat model, not an edge case. The agent must distinguish user/developer/project instructions from content being analyzed.

## Decision

AgentZero labels content by trust source and never lets untrusted content become trusted instruction. Documents, web pages, PDFs, README files, issue comments, PR comments, terminal output, MCP output, generated files, dependency docs, and package content are untrusted unless explicitly promoted by policy or user approval.

## Consequences

The session engine must track context provenance. Tools must return typed content with trust labels. Skills and packages must not disable security instructions. Model prompts must preserve trust boundaries and instruct the model to treat untrusted content as data.
