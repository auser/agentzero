#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# AgentZero Examples — Run All Smoke Tests
#
# Starts the gateway for each example config, runs the smoke test suite,
# then stops the gateway. No LLM provider needed for smoke tests.
#
# Usage:
#   ./examples/test-all.sh                 # smoke tests only (no API key)
#   ./examples/test-all.sh --with-pipeline # also run the full research pipeline
#
# Prerequisites:
#   - jq installed
#   - agentzero binary built or on PATH
# ---------------------------------------------------------------------------
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WITH_PIPELINE=false
PASS=0
FAIL=0

for arg in "$@"; do
  case "$arg" in
    --with-pipeline) WITH_PIPELINE=true ;;
  esac
done

red()   { printf "\033[31m%s\033[0m\n" "$*"; }
green() { printf "\033[32m%s\033[0m\n" "$*"; }
yellow(){ printf "\033[33m%s\033[0m\n" "$*"; }
bold()  { printf "\033[1m%s\033[0m\n" "$*"; }

# Find agentzero binary
find_binary() {
  if [ -n "${AGENTZERO:-}" ]; then echo "$AGENTZERO"; return; fi
  for candidate in \
    "$ROOT_DIR/target/release/agentzero" \
    "$ROOT_DIR/target/debug/agentzero" \
    "$(which agentzero 2>/dev/null || true)"; do
    if [ -x "$candidate" ]; then echo "$candidate"; return; fi
  done
  echo ""
}

AZ=$(find_binary)
if [ -z "$AZ" ]; then
  red "ERROR: agentzero binary not found"
  echo "Build with: cargo build -p agentzero --release"
  exit 1
fi

# Check jq
if ! command -v jq &> /dev/null; then
  red "ERROR: jq is required but not installed"
  echo "Install: brew install jq  (macOS) or apt-get install jq (Linux)"
  exit 1
fi

echo ""
bold "================================================"
bold " AgentZero Examples — Smoke Test Suite"
bold "================================================"
echo ""
echo "Binary: $AZ"
echo ""

# ── Helper: run test against an example ────────────────────────────────────

test_example() {
  local name="$1"
  local config="$2"
  local test_script="$3"
  local port="${4:-0}"

  # Pick a random port if not specified
  if [ "$port" = "0" ]; then
    port=$((42700 + RANDOM % 100))
  fi

  local gateway="http://127.0.0.1:$port"
  local log="/tmp/agentzero-test-$name.log"
  local pid=""

  echo ""
  bold "── $name ──"
  echo "Config: $config"
  echo "Port:   $port"
  echo ""

  # Start gateway
  "$AZ" gateway \
    --config "$config" \
    --host 127.0.0.1 \
    --port "$port" \
    --new-pairing \
    > "$log" 2>&1 &
  pid=$!

  # Wait for startup
  echo -n "Starting gateway..."
  local started=false
  for i in $(seq 1 20); do
    if curl -sf "$gateway/health" > /dev/null 2>&1; then
      started=true
      break
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      echo ""
      red "Gateway died. Log:"
      tail -20 "$log"
      FAIL=$((FAIL+1))
      return
    fi
    echo -n "."
    sleep 1
  done

  if ! $started; then
    echo ""
    red "Gateway failed to start. Log:"
    tail -20 "$log"
    kill "$pid" 2>/dev/null || true
    FAIL=$((FAIL+1))
    return
  fi
  echo " ready (pid $pid)"

  # Extract pairing code
  sleep 1
  local code
  code=$(grep -oE 'X-Pairing-Code: [A-Za-z0-9_-]+' "$log" \
    | head -1 | awk '{print $NF}') || code=""

  # Run test script
  if GATEWAY="$gateway" PAIRING_CODE="$code" bash "$test_script"; then
    PASS=$((PASS+1))
    green "$name: ALL PASSED"
  else
    FAIL=$((FAIL+1))
    red "$name: SOME TESTS FAILED"
  fi

  # Stop gateway
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}

# ── Run tests ──────────────────────────────────────────────────────────────

# 1. Config validation (cargo test, no gateway needed)
bold "── Config Validation (cargo test) ──"
echo ""
if cargo nextest run -p agentzero-config example_ --no-fail-fast 2>&1 | tail -5; then
  PASS=$((PASS+1))
  green "Config validation: PASSED"
else
  FAIL=$((FAIL+1))
  red "Config validation: FAILED"
fi

# 2. Research Pipeline
test_example "research-pipeline" \
  "$SCRIPT_DIR/research-pipeline/agentzero.toml" \
  "$SCRIPT_DIR/research-pipeline/test-gateway.sh" \
  42701

# 3. Business Office
test_example "business-office" \
  "$SCRIPT_DIR/business-office/agentzero.toml" \
  "$SCRIPT_DIR/business-office/test-gateway.sh" \
  42702

# 4. Full pipeline (optional, needs API key)
if $WITH_PIPELINE; then
  echo ""
  bold "── Full Pipeline Test (requires API key) ──"
  echo ""
  if [ -z "${ANTHROPIC_API_KEY:-}${OPENAI_API_KEY:-}" ]; then
    yellow "Skipping: no API key set"
    echo "Set ANTHROPIC_API_KEY or OPENAI_API_KEY to run"
  else
    cd "$SCRIPT_DIR/research-pipeline"
    if bash test-pipeline.sh; then
      PASS=$((PASS+1))
      green "Full pipeline: PASSED"
    else
      FAIL=$((FAIL+1))
      red "Full pipeline: FAILED"
    fi
    cd "$ROOT_DIR"
  fi
fi

# ── Summary ────────────────────────────────────────────────────────────────

echo ""
bold "================================================"
bold " Summary"
bold "================================================"
echo ""
echo "  $(green "$PASS passed"), $(red "$FAIL failed")"
echo ""

[ "$FAIL" -gt 0 ] && exit 1 || exit 0
