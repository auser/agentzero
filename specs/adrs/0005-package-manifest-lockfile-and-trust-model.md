# ADR 0005: Package Manifest, Lockfile, and Trust Model

## Status

Accepted

## Context

Pi-like package extensibility is valuable, but arbitrary package trust conflicts with AgentZero's security promise. AgentZero packages must be inspectable, reproducible, permissioned, and auditable.

## Decision

AgentZero packages distribute skills, prompts, tools, references, policies, and tests. Packages require a manifest declaring permissions, runtime requirements, package contents, source, and security metadata. Executable packages require a lockfile with resolved versions, content hashes, and permission snapshots before execution.

## Consequences

No package install script runs by default. Native binaries are denied by default. Post-install network fetches are denied by default. Package trust decisions are scoped to package version and manifest permissions. Package signing can be added later without changing the manifest model.
