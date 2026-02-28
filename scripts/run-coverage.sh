#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/run-coverage.sh [OPTIONS]

Generate workspace coverage reports using cargo-llvm-cov.

Options:
  --output-dir PATH   Output directory (default: coverage)
  --lcov PATH         LCOV output path (default: coverage/lcov.info)
  --help              Show this help text
USAGE
}

output_dir="coverage"
lcov_path=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    --lcov)
      lcov_path="${2:-}"
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

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo "cargo-llvm-cov is required (install with: cargo install cargo-llvm-cov --locked)" >&2
  exit 1
fi

mkdir -p "$output_dir"
if [[ -z "$lcov_path" ]]; then
  lcov_path="$output_dir/lcov.info"
fi

summary_path="$output_dir/summary.txt"

cargo llvm-cov --workspace --lcov --output-path "$lcov_path" | tee "$summary_path"

echo "PASS: coverage artifacts generated"
echo "  summary: $summary_path"
echo "  lcov:    $lcov_path"
