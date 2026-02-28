# Dependency and CVE Policy

## Scope
Applies to all Rust workspace crates and build/runtime dependencies.

## CI Security Gates
- `cargo audit` runs on every PR and push to detect RustSec advisories.
- `cargo deny check advisories` runs on every PR and push to detect denied advisories and yanked crates.

## Update Cadence
- Routine dependency review: weekly.
- Lockfile refresh cadence: at least bi-weekly, or immediately for security issues.
- Security-critical dependency updates: same day when feasible.

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
