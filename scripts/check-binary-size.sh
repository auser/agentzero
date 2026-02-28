#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/check-binary-size.sh [OPTIONS]

Checks that a binary does not exceed a configured size budget.

Options:
  --binary PATH       Binary path (default: target/release/agentzero)
  --max-bytes N       Maximum allowed binary size in bytes (default: 20000000)
  --help              Show this help text
USAGE
}

binary="target/release/agentzero"
max_bytes="20000000"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary)
      binary="${2:-}"
      shift 2
      ;;
    --max-bytes)
      max_bytes="${2:-}"
      shift 2
      ;;
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

if [[ -z "$binary" ]]; then
  echo "--binary must not be empty" >&2
  exit 1
fi

if [[ -z "$max_bytes" || ! "$max_bytes" =~ ^[0-9]+$ ]]; then
  echo "--max-bytes must be a positive integer" >&2
  exit 1
fi

if [[ ! -f "$binary" ]]; then
  echo "Binary not found: $binary" >&2
  exit 1
fi

size_bytes="$(wc -c < "$binary" | tr -d '[:space:]')"
if [[ "$size_bytes" -le "$max_bytes" ]]; then
  echo "PASS: $binary size ${size_bytes}B <= budget ${max_bytes}B"
  exit 0
fi

echo "FAIL: $binary size ${size_bytes}B exceeds budget ${max_bytes}B" >&2
exit 1
