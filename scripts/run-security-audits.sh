#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/run-security-audits.sh [OPTIONS]

Run dependency security checks:
1) cargo audit
2) cargo deny check advisories

Options:
  --help   Show this help text
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if ! command -v cargo-audit >/dev/null 2>&1; then
  echo "cargo-audit is required (install with: cargo install cargo-audit --locked)" >&2
  exit 1
fi

if ! command -v cargo-deny >/dev/null 2>&1; then
  echo "cargo-deny is required (install with: cargo install cargo-deny --locked)" >&2
  exit 1
fi

# Ignore advisories for deps we cannot update yet.
# RUSTSEC-2026-0049: rustls-webpki 0.102.8 via libsql → rustls 0.22
#   (no 0.102.x patch; awaiting libsql upgrade to rustls 0.23)
# RUSTSEC-2026-0098: rustls-webpki name constraints for URI names (transitive)
# RUSTSEC-2026-0099: rustls-webpki wildcard name constraints (transitive)
# RUSTSEC-2026-0002: lru 0.12.5 via ratatui/tantivy (transitive)
# RUSTSEC-2026-0097: rand 0.8.5 (direct, requires 0.8→0.9 migration)
# Note: cargo-deny does not match RUSTSEC-2026-0049 so it is NOT in deny.toml.
cargo audit \
  --ignore RUSTSEC-2026-0049 \
  --ignore RUSTSEC-2026-0098 \
  --ignore RUSTSEC-2026-0099 \
  --ignore RUSTSEC-2026-0002 \
  --ignore RUSTSEC-2026-0097
cargo deny check advisories

echo "PASS: security audits completed"
