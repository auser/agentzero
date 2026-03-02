---
title: Dependency Policy
description: Dependency and CVE policy for all Rust workspace crates.
---

## Scope
Applies to all Rust workspace crates and build/runtime dependencies.

## CI Security Gates
- Security audit gates are enforced through `scripts/run-security-audits.sh`.
- Enforced workflows:
  - `.github/workflows/ci.yml` (PR + push)
  - `.github/workflows/cd.yml` (push to `main`)
  - `.github/workflows/release.yml` (tag/manual release)
- Checks executed:
  - `cargo audit`
  - `cargo deny check advisories`
- Merge/release blocking:
  - Any failing security audit job blocks the workflow and must be resolved or explicitly excepted in policy.

## Update Cadence
- Routine dependency review: weekly.
- Lockfile refresh cadence: at least bi-weekly, or immediately for security issues.
- Security-critical dependency updates: same day when feasible.

## Automated Dependency Updates
- Dependabot configuration is required at `.github/dependabot.yml`.
- Required ecosystems:
  - `cargo` (workspace dependencies)
  - `github-actions` (workflow/action pin updates)
- Dependabot PRs should carry dependency labels and follow normal CI/security gates.

## CVE / Advisory Response Policy
- Critical / High: patch or mitigate within 24 hours; release fix as soon as validation passes.
- Medium: patch or mitigate within 7 days.
- Low: patch in normal maintenance cycle (<=30 days).
- If no patch exists: document temporary mitigation and monitor upstream daily.

## Triage and Ownership
- PR author performs initial advisory impact review.
- Maintainer on duty approves final severity and mitigation timeline.
- Every security dependency change must reference this policy in PR notes.

## Exceptions
- Any temporary exception must include:
- advisory ID(s)
- reason and compensating control
- explicit expiration date
- PR reference updating this policy and `deny.toml` when applicable.
