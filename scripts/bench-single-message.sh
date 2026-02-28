#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/bench-single-message.sh [--iterations N] [--binary PATH] [--message TEXT] [--command "..."]

Benchmarks a single-message run repeatedly. By default this benchmarks:
  <binary> agent -m "<message>"

Options:
  --iterations N     Number of benchmark runs (default: 10)
  --binary PATH      Binary path (default: target/release/agentzero)
  --message TEXT     Message for default agent command (default: hello benchmark)
  --command "..."    Override command args passed to binary
  -h, --help         Show this help
EOF
}

iterations=10
binary="target/release/agentzero"
message="hello benchmark"
override_cmd=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --iterations)
      iterations="${2:-}"
      shift 2
      ;;
    --binary)
      binary="${2:-}"
      shift 2
      ;;
    --message)
      message="${2:-}"
      shift 2
      ;;
    --command)
      override_cmd="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ! [[ "$iterations" =~ ^[0-9]+$ ]] || [[ "$iterations" -le 0 ]]; then
  echo "iterations must be a positive integer" >&2
  exit 2
fi

if [[ ! -x "$binary" ]]; then
  cargo build -p agentzero --release >/dev/null
fi

if [[ -z "$override_cmd" ]]; then
  if [[ -z "${OPENAI_API_KEY:-}" ]]; then
    echo "OPENAI_API_KEY must be set for default single-message benchmark" >&2
    exit 2
  fi
  cmd_args="agent -m \"$message\""
else
  cmd_args="$override_cmd"
fi

sum_ms=0
min_ms=""
max_ms=0

for ((i=1; i<=iterations; i++)); do
  start_s="$(perl -MTime::HiRes=time -e 'printf "%.6f\n", time')"
  eval "$binary $cmd_args" >/dev/null 2>&1
  end_s="$(perl -MTime::HiRes=time -e 'printf "%.6f\n", time')"
  elapsed_ms="$(awk -v s="$start_s" -v e="$end_s" 'BEGIN { printf "%.3f", (e-s)*1000 }')"

  sum_ms="$(awk -v a="$sum_ms" -v b="$elapsed_ms" 'BEGIN { printf "%.3f", a+b }')"
  if [[ -z "$min_ms" ]] || awk -v a="$elapsed_ms" -v b="$min_ms" 'BEGIN { exit !(a < b) }'; then
    min_ms="$elapsed_ms"
  fi
  if awk -v a="$elapsed_ms" -v b="$max_ms" 'BEGIN { exit !(a > b) }'; then
    max_ms="$elapsed_ms"
  fi
done

avg_ms="$(awk -v total="$sum_ms" -v n="$iterations" 'BEGIN { printf "%.3f", total / n }')"

echo "benchmark=single_message"
echo "iterations=$iterations"
echo "min_ms=$min_ms"
echo "avg_ms=$avg_ms"
echo "max_ms=$max_ms"
