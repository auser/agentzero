## Summary
- 

## Verification
- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Security Review Checklist
- [ ] Secrets are not introduced in code, logs, fixtures, or docs.
- [ ] New/changed network endpoints use secure transport (TLS) or are explicitly local-only.
- [ ] New/changed tool, plugin, or memory behavior is fail-closed by default.
- [ ] Threat model updated for any attack-surface change (`docs/security/THREAT_MODEL.md`).
- [ ] Success-path and abuse/negative-path tests are included.
- [ ] Supply-chain risk reviewed for new dependencies (advisory/license/source impact).

## Sprint Tracking
- [ ] `specs/SPRINT.md` updated (task status + acceptance criteria)
