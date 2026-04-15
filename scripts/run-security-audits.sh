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

cargo audit
cargo deny check advisories

echo "PASS: security audits completed"
