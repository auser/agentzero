#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/verify-dependency-policy.sh [OPTIONS]

Verifies dependency policy guardrails are present:
1) deny.toml exists (cargo-deny advisory config)
2) docs/security/dependency-policy.md exists and references update cadence

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

deny_path="deny.toml"
policy_path="site/src/content/docs/security/dependency-policy.md"

if [[ ! -f "$deny_path" ]]; then
  echo "Missing file: $deny_path" >&2
  exit 1
fi

if [[ ! -f "$policy_path" ]]; then
  echo "Missing file: $policy_path" >&2
  exit 1
fi

if ! grep -qi "Update Cadence" "$policy_path"; then
  echo "dependency policy doc missing Update Cadence section" >&2
  exit 1
fi

echo "PASS: dependency update policy is configured and documented"
