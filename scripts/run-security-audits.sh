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

# cargo-deny handles advisory checking with the ignore list in deny.toml.
# cargo-audit runs without ignore support, so we pass --ignore for each
# advisory that is tracked in deny.toml as an upstream transitive dep.
cargo audit \
  --ignore RUSTSEC-2024-0436 \
  --ignore RUSTSEC-2025-0057 \
  --ignore RUSTSEC-2025-0134 \
  --ignore RUSTSEC-2025-0141 \
  --ignore RUSTSEC-2026-0002 \
  --ignore RUSTSEC-2026-0049 \

cargo deny check advisories

echo "PASS: security audits completed"
