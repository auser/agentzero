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

# Check if already running
if gz /health -sf > /dev/null 2>&1; then
  yellow "Gateway already running at $GATEWAY"
  GATEWAY_PID=""
else
  "$AZ" gateway \
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
fi
echo ""

# ── 3. Pair client ─────────────────────────────────────────────────────────

bold "3. Pairing client"

# Extract pairing code from log when we started the gateway ourselves.
# If the gateway was already running, use PAIRING_CODE from the environment.
PAIRING_CODE=""
if [ -n "$GATEWAY_PID" ]; then
  # We started the gateway — extract its pairing code from the log
  sleep 1
  PAIRING_CODE=$(grep -oE 'X-Pairing-Code: [A-Za-z0-9_-]+' /tmp/agentzero-test-gateway.log \
    | head -1 | awk '{print $NF}') || true
elif [ -n "${PAIRING_CODE_OVERRIDE:-}" ]; then
  PAIRING_CODE="$PAIRING_CODE_OVERRIDE"
fi

if [ -z "$PAIRING_CODE" ]; then
  yellow "Could not extract pairing code from log."
  echo "Looking for bearer token in environment..."
  if [ -n "${AGENTZERO_GATEWAY_BEARER_TOKEN:-}" ]; then
    TOKEN="$AGENTZERO_GATEWAY_BEARER_TOKEN"
    green "Using AGENTZERO_GATEWAY_BEARER_TOKEN"
  else
    red "Cannot authenticate. Set AGENTZERO_GATEWAY_BEARER_TOKEN or restart with --new-pairing"
    echo ""
    echo "Gateway log:"
    cat /tmp/agentzero-test-gateway.log
    exit 1
  fi
else
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
fi
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

# ── 5. Submit research job ─────────────────────────────────────────────────

bold "5. Submitting research job"
echo "   Topic: $TOPIC"

SUBMIT_RESPONSE=$(gz /v1/runs -sf -X POST \
  -d "$(jq -n --arg msg "Research: $TOPIC" '{message: $msg}')") || {
  red "Job submission failed"
  exit 1
}

RUN_ID=$(echo "$SUBMIT_RESPONSE" | jq -r '.run_id // empty')
if [ -z "$RUN_ID" ]; then
  red "No run_id in response: $SUBMIT_RESPONSE"
  exit 1
fi
green "  Submitted: $RUN_ID"
echo ""

# ── 6. Poll for completion ─────────────────────────────────────────────────

bold "6. Waiting for pipeline to complete (timeout: ${TIMEOUT}s)"

START_TIME=$(date +%s)
LAST_STATUS=""

while true; do
  ELAPSED=$(( $(date +%s) - START_TIME ))
  if [ "$ELAPSED" -ge "$TIMEOUT" ]; then
    echo ""
    red "Pipeline timed out after ${TIMEOUT}s (last status: $LAST_STATUS)"
    echo ""
    echo "Check gateway log: cat /tmp/agentzero-test-gateway.log"
    echo "Poll manually:     gz /v1/runs/$RUN_ID -sf"
    exit 1
  fi

  STATUS_JSON=$(gz "/v1/runs/$RUN_ID" -sf) || {
    echo -n "?"
    sleep 2
    continue
  }
  STATUS=$(echo "$STATUS_JSON" | jq -r '.status // "unknown"')

  if [ "$STATUS" != "$LAST_STATUS" ]; then
    [ -n "$LAST_STATUS" ] && echo ""
    echo -n "  [$STATUS] ${ELAPSED}s"
    LAST_STATUS="$STATUS"
  else
    echo -n "."
  fi

  case "$STATUS" in
    completed)
      echo ""
      green "  Pipeline completed in ${ELAPSED}s"
      break
      ;;
    failed)
      echo ""
      ERROR=$(echo "$STATUS_JSON" | jq -r '.error // "unknown error"')
      red "  Pipeline failed: $ERROR"
      if echo "$ERROR" | grep -qi "timeout"; then
        echo ""
        yellow "  Hint: the LLM provider request timed out."
        echo "  Check /tmp/agentzero-test-gateway.log for 429 (rate limit) errors."
        echo "  If rate-limited, wait a minute and try again."
      fi
      exit 1
      ;;
    cancelled)
      echo ""
      red "  Pipeline was cancelled"
      exit 1
      ;;
  esac

  sleep 3
done
echo ""

# ── 7. Retrieve result ────────────────────────────────────────────────────

bold "7. Retrieving result"

RESULT_JSON=$(gz "/v1/runs/$RUN_ID/result" -sf) || {
  red "Failed to fetch result"
  exit 1
}
RESULT=$(echo "$RESULT_JSON" | jq -r '.result // empty')

if [ -n "$RESULT" ]; then
  RESULT_LEN=${#RESULT}
  green "  Result: $RESULT_LEN chars"
  echo ""
  echo "  --- First 500 chars ---"
  echo "$RESULT" | head -c 500
  echo ""
  echo "  --- End preview ---"
else
  yellow "  Result body is empty (output may be in files)"
fi
echo ""

# ── 8. Check output files ─────────────────────────────────────────────────

bold "8. Checking output files"

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
check_file "output/brief.md" "Final brief"
check_file "research/events.jsonl" "Event log"

if [ -f "research/events.jsonl" ]; then
  EVENT_COUNT=$(wc -l < "research/events.jsonl" | tr -d ' ')
  green "  Event log: $EVENT_COUNT events"
fi

echo ""

# ── 9. Fetch event log via API ─────────────────────────────────────────────

bold "9. API event log"

EVENTS_JSON=$(gz "/v1/runs/$RUN_ID/events" -sf) || EVENTS_JSON="{}"
API_EVENT_COUNT=$(echo "$EVENTS_JSON" | jq '.events | length // 0' 2>/dev/null) || API_EVENT_COUNT=0
green "  /v1/runs/$RUN_ID/events: $API_EVENT_COUNT events"

echo ""

# ── Summary ────────────────────────────────────────────────────────────────

bold "Pipeline test complete!"
echo ""
echo "  Run ID:    $RUN_ID"
echo "  Duration:  ${ELAPSED}s"
echo "  Result:    ${RESULT_LEN:-0} chars"

if [ -f "output/brief.md" ]; then
  echo ""
  bold "Final brief:"
  echo ""
  cat "output/brief.md"
fi
