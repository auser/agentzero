#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Research Pipeline — Full Pipeline Test
#
# Starts the gateway, pairs a client, submits a research job via /v1/runs,
# polls until complete, and validates output files.
#
# Requires:
#   - A built agentzero binary (cargo build -p agentzero --release)
#   - A valid API key (ANTHROPIC_API_KEY or OPENAI_API_KEY)
#
# Usage:
#   cd examples/research-pipeline
#   ./test-pipeline.sh
#
# Options (env vars):
#   TOPIC      — research topic (default: "current state of AI regulation in the EU")
#   TIMEOUT    — max seconds to wait for pipeline (default: 300)
#   KEEP       — set to 1 to keep gateway running after test
#   AGENTZERO  — path to agentzero binary (default: auto-detect)
# ---------------------------------------------------------------------------
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

TOPIC="${TOPIC:-current state of AI regulation in the EU}"
TIMEOUT="${TIMEOUT:-300}"
KEEP="${KEEP:-0}"
PORT=42617
GATEWAY="http://127.0.0.1:$PORT"
GATEWAY_PID=""

# ── Helpers ────────────────────────────────────────────────────────────────

red()   { printf "\033[31m%s\033[0m\n" "$*"; }
green() { printf "\033[32m%s\033[0m\n" "$*"; }
yellow(){ printf "\033[33m%s\033[0m\n" "$*"; }
bold()  { printf "\033[1m%s\033[0m\n" "$*"; }

# gz — gateway curl helper (path-first)
# First arg is the gateway path; remaining args go to curl.
# Adds auth header (if TOKEN set) and JSON content-type for mutations.
gz() {
  local path="$1"; shift
  local -a auth=()
  if [ -n "${TOKEN:-}" ]; then
    auth=(-H "Authorization: Bearer $TOKEN")
  fi
  local -a ct=()
  local arg; for arg in "$@"; do
    case "$arg" in POST|PUT|PATCH) ct=(-H "Content-Type: application/json"); break;; esac
  done
  curl ${auth[@]+"${auth[@]}"} ${ct[@]+"${ct[@]}"} "$@" "$GATEWAY$path"
}

cleanup() {
  if [ -n "$GATEWAY_PID" ] && [ "$KEEP" != "1" ]; then
    echo ""
    echo "Stopping gateway (pid $GATEWAY_PID)..."
    kill "$GATEWAY_PID" 2>/dev/null || true
    wait "$GATEWAY_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# Find agentzero binary
find_binary() {
  if [ -n "${AGENTZERO:-}" ]; then
    echo "$AGENTZERO"
    return
  fi
  # Check common locations
  for candidate in \
    "../../target/release/agentzero" \
    "../../target/debug/agentzero" \
    "$(which agentzero 2>/dev/null || true)"; do
    if [ -x "$candidate" ]; then
      echo "$candidate"
      return
    fi
  done
  echo ""
}

AZ=$(find_binary)
if [ -z "$AZ" ]; then
  red "ERROR: agentzero binary not found"
  echo "Build with: cargo build -p agentzero --release"
  echo "Or set AGENTZERO=/path/to/binary"
  exit 1
fi

echo ""
bold "Research Pipeline — Full Pipeline Test"
echo "Binary:  $AZ"
echo "Config:  $SCRIPT_DIR/agentzero.toml"
echo "Topic:   $TOPIC"
echo "Timeout: ${TIMEOUT}s"
echo ""

# Clean up stale data from previous runs
rm -f research.db
rm -rf research/
mkdir -p research

# ── 1. Check API key ──────────────────────────────────────────────────────

bold "1. Checking API credentials"

if [ -z "${ANTHROPIC_API_KEY:-}${OPENAI_API_KEY:-}" ]; then
  # Check if auth store has a profile
  if ! "$AZ" auth status > /dev/null 2>&1; then
    red "ERROR: No API key found"
    echo ""
    echo "Set one of:"
    echo "  export ANTHROPIC_API_KEY=sk-ant-..."
    echo "  export OPENAI_API_KEY=sk-..."
    echo "  agentzero auth setup-token --provider anthropic"
    exit 1
  fi
  green "Using credentials from auth store"
else
  green "API key found in environment"
fi
echo ""

# ── 2. Start gateway ──────────────────────────────────────────────────────

bold "2. Starting gateway"

# Check if already running — kill and restart so we get a fresh pairing code
if gz /health -sf > /dev/null 2>&1; then
  yellow "Gateway already running at $GATEWAY — restarting for fresh pairing..."
  # Try to find and kill the existing process
  pkill -f 'agentzero.*gateway' 2>/dev/null || true
  sleep 2
  if gz /health -sf > /dev/null 2>&1; then
    red "Could not stop existing gateway. Kill it manually and retry."
    exit 1
  fi
  green "Old gateway stopped."
fi

RUST_LOG="${RUST_LOG:-info}" "$AZ" gateway \
  --config "$SCRIPT_DIR/agentzero.toml" \
  --host 127.0.0.1 \
  --port "$PORT" \
  --new-pairing \
  > /tmp/agentzero-test-gateway.log 2>&1 &
GATEWAY_PID=$!

# Wait for gateway to start
echo -n "Waiting for gateway..."
for i in $(seq 1 30); do
  if gz /health -sf > /dev/null 2>&1; then
    echo ""
    green "Gateway started (pid $GATEWAY_PID)"
    break
  fi
  if ! kill -0 "$GATEWAY_PID" 2>/dev/null; then
    echo ""
    red "Gateway process died. Log:"
    cat /tmp/agentzero-test-gateway.log
    exit 1
  fi
  echo -n "."
  sleep 1
done
if ! gz /health -sf > /dev/null 2>&1; then
  echo ""
  red "Gateway failed to start within 30s"
  cat /tmp/agentzero-test-gateway.log
  exit 1
fi
echo ""

# ── 3. Pair client ─────────────────────────────────────────────────────────

bold "3. Pairing client"

# Extract pairing code from log (we always start a fresh gateway above).
sleep 1
PAIRING_CODE=$(grep -oE 'X-Pairing-Code: [A-Za-z0-9_-]+' /tmp/agentzero-test-gateway.log \
  | head -1 | awk '{print $NF}') || true

if [ -z "$PAIRING_CODE" ]; then
  # Fallback: try to read the 6-digit code from the box in the log
  PAIRING_CODE=$(grep -oE '[0-9]{6}' /tmp/agentzero-test-gateway.log \
    | head -1) || true
fi

if [ -z "$PAIRING_CODE" ]; then
  red "Could not extract pairing code from gateway log."
  echo ""
  echo "Gateway log:"
  cat /tmp/agentzero-test-gateway.log
  exit 1
fi

PAIR_RESPONSE=$(gz /pair -sf -X POST \
  -H "X-Pairing-Code: $PAIRING_CODE") || {
  red "Pairing failed. Code: $PAIRING_CODE"
  exit 1
}
TOKEN=$(echo "$PAIR_RESPONSE" | jq -r '.token // empty')
if [ -z "$TOKEN" ]; then
  red "Pairing succeeded but no token returned: $PAIR_RESPONSE"
  exit 1
fi
green "Paired (token: ${TOKEN:0:16}...)"
echo ""

# ── 4. Quick health checks ────────────────────────────────────────────────

bold "4. Pre-flight checks"

# Health
HEALTH=$(gz /health -sf | jq -r '.status') || HEALTH=""
if [ "$HEALTH" = "ok" ]; then
  green "  /health: ok"
else
  red "  /health: $HEALTH"
  exit 1
fi

# Models
MODELS=$(gz /v1/models -sf | jq '.data | length') || MODELS=0
green "  /v1/models: $MODELS models"

# Agents
AGENTS_JSON=$(gz /v1/agents -sf) || AGENTS_JSON="{}"
AGENTS_TOTAL=$(echo "$AGENTS_JSON" | jq '.total // 0') || AGENTS_TOTAL=0
green "  /v1/agents: $AGENTS_TOTAL registered"

echo ""

# ── 5. Submit research job via /api/chat (routes through swarm pipeline) ───

bold "5. Submitting research job via /api/chat (swarm pipeline)"
echo "   Topic: $TOPIC"
echo "   This is synchronous — it waits for the full pipeline to complete."
echo "   Timeout: ${TIMEOUT}s"
echo ""

START_TIME=$(date +%s)

RESULT=$(gz /api/chat -sf -X POST \
  --max-time "$TIMEOUT" \
  -d "$(jq -n --arg msg "Research: $TOPIC" '{message: $msg}')") || {
  ELAPSED=$(( $(date +%s) - START_TIME ))
  red "Pipeline failed or timed out after ${ELAPSED}s"
  echo ""
  echo "Check gateway log:"
  echo "  cat /tmp/agentzero-test-gateway.log"
  exit 1
}

ELAPSED=$(( $(date +%s) - START_TIME ))
RESULT_TEXT=$(echo "$RESULT" | jq -r '.message // empty')

if [ -n "$RESULT_TEXT" ]; then
  RESULT_LEN=${#RESULT_TEXT}
  green "  Pipeline completed in ${ELAPSED}s ($RESULT_LEN chars)"
  echo ""
  echo "  --- First 500 chars ---"
  echo "$RESULT_TEXT" | head -c 500
  echo ""
  echo "  --- End preview ---"
else
  yellow "  Pipeline returned empty response (check gateway log)"
fi
echo ""

# ── 6. Check output files ─────────────────────────────────────────────────

bold "6. Checking output files"

check_file() {
  local path="$1" label="$2"
  if [ -f "$path" ]; then
    local size
    size=$(wc -c < "$path" | tr -d ' ')
    green "  $label: $path ($size bytes)"
  else
    yellow "  $label: $path (not found)"
  fi
}

check_file "research/raw-findings.md" "Raw findings"
check_file "research/detailed-data.md" "Detailed data"
check_file "research/analysis.md" "Analysis"
check_file "research/brief.md" "Final brief"
check_file "research/events.jsonl" "Event log"

if [ -f "research/events.jsonl" ]; then
  EVENT_COUNT=$(wc -l < "research/events.jsonl" | tr -d ' ')
  green "  Event log: $EVENT_COUNT events"
fi

echo ""

# ── Summary ────────────────────────────────────────────────────────────────

bold "Pipeline test complete!"
echo ""
echo "  Duration:  ${ELAPSED}s"
echo "  Result:    ${RESULT_LEN:-0} chars"

if [ -f "research/brief.md" ]; then
  echo ""
  bold "Final brief:"
  echo ""
  cat "research/brief.md"
fi
