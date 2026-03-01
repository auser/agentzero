#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/verify-dependency-policy.sh [OPTIONS]

Verifies dependency policy guardrails are present:
1) .github/dependabot.yml exists and includes cargo + github-actions ecosystems
2) docs/security/DEPENDENCY_POLICY.md exists and references update cadence

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

dependabot_path=".github/dependabot.yml"
policy_path="public/src/content/docs/security/dependency-policy.md"

if [[ ! -f "$dependabot_path" ]]; then
  echo "Missing file: $dependabot_path" >&2
  exit 1
fi

if [[ ! -f "$policy_path" ]]; then
  echo "Missing file: $policy_path" >&2
  exit 1
fi

if ! grep -qE 'package-ecosystem:\s*"cargo"' "$dependabot_path"; then
  echo "dependabot config missing cargo ecosystem entry" >&2
  exit 1
fi

if ! grep -qE 'package-ecosystem:\s*"github-actions"' "$dependabot_path"; then
  echo "dependabot config missing github-actions ecosystem entry" >&2
  exit 1
fi

if ! grep -qi "Update Cadence" "$policy_path"; then
  echo "dependency policy doc missing Update Cadence section" >&2
  exit 1
fi

echo "PASS: dependency update policy is configured and documented"
