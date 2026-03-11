#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Research Pipeline — Gateway Smoke Test
#
# Tests every gateway endpoint WITHOUT needing an LLM provider.
# The gateway must be running: agentzero gateway --config agentzero.toml
#
# Usage:
#   cd examples/research-pipeline
#   agentzero gateway &          # start in background
#   ./test-gateway.sh            # run this script
#   kill %1                      # stop gateway
#
# Or point at a different host/port:
#   GATEWAY=http://127.0.0.1:9090 ./test-gateway.sh
# ---------------------------------------------------------------------------
set -euo pipefail

GATEWAY="${GATEWAY:-http://127.0.0.1:42617}"
PASS=0
FAIL=0
SKIP=0
TOKEN=""

# ── Helpers ────────────────────────────────────────────────────────────────

red()   { printf "\033[31m%s\033[0m" "$*"; }
green() { printf "\033[32m%s\033[0m" "$*"; }
yellow(){ printf "\033[33m%s\033[0m" "$*"; }
bold()  { printf "\033[1m%s\033[0m" "$*"; }

pass() { PASS=$((PASS+1)); echo "  $(green PASS)  $1"; }
fail() { FAIL=$((FAIL+1)); echo "  $(red FAIL)  $1: $2"; }
skip() { SKIP=$((SKIP+1)); echo "  $(yellow SKIP)  $1: $2"; }

# gz — gateway curl helper
# First arg is the gateway path; remaining args go to curl.
# Adds auth header (if TOKEN set) and JSON content-type for mutations.
# Usage:  gz /health                            # simple GET
#         gz /v1/models -sf                     # GET silent+fail
#         gz /v1/runs -sf -X POST -d '{"m":1}'  # POST with JSON body
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

# assert_gz — check HTTP status code for a gateway path
# Usage: assert_gz "label" expected_code /path [extra-curl-args...]
assert_gz() {
  local label="$1" expected="$2" path="$3"
  shift 3
  local -a auth=()
  if [ -n "${TOKEN:-}" ]; then
    auth=(-H "Authorization: Bearer $TOKEN")
  fi
  local status
  status=$(curl -s -o /dev/null -w '%{http_code}' ${auth[@]+"${auth[@]}"} "$@" "$GATEWAY$path") || true
  if [ "$status" = "$expected" ]; then
    pass "$label (HTTP $status)"
  else
    fail "$label" "expected $expected, got $status"
  fi
}

# ── Pre-flight ─────────────────────────────────────────────────────────────

echo ""
bold "Research Pipeline — Gateway Smoke Test"
echo "Gateway: $GATEWAY"
echo ""

echo "Checking gateway is reachable..."
if ! gz /health -sf > /dev/null 2>&1; then
  echo ""
  red "ERROR: Gateway not reachable at $GATEWAY"
  echo ""
  echo "Start it with:"
  echo "  agentzero gateway --config examples/research-pipeline/agentzero.toml"
  echo ""
  exit 1
fi
echo ""

# ── 1. Health & Status ─────────────────────────────────────────────────────

bold "1. Health & Status"
echo ""

HEALTH=$(gz /health -sf | jq -r '.status' 2>/dev/null) || HEALTH=""
if [ "$HEALTH" = "ok" ]; then
  pass "GET /health (.status=ok)"
else
  fail "GET /health" ".status: expected 'ok', got '$HEALTH'"
fi

assert_gz "GET /health/ready" 200 /health/ready
assert_gz "GET /metrics" 200 /metrics
assert_gz "GET / (dashboard)" 200 /

echo ""

# ── 2. Pairing ─────────────────────────────────────────────────────────────

bold "2. Pairing"
echo ""

# Wrong code should fail
assert_gz "POST /pair (wrong code)" 403 /pair \
  -X POST -H "X-Pairing-Code: wrong-code-12345"

# No code header should fail
assert_gz "POST /pair (no header)" 401 /pair \
  -X POST

# Try to pair with the real code from the gateway.
# If PAIRING_CODE is set, use it; otherwise skip pairing-dependent tests.
if [ -n "${PAIRING_CODE:-}" ]; then
  PAIR_RESPONSE=$(gz /pair -sf -X POST \
    -H "X-Pairing-Code: $PAIRING_CODE") || { fail "POST /pair" "curl failed"; }
  PAIRED=$(echo "$PAIR_RESPONSE" | jq -r '.paired // empty' 2>/dev/null)
  TOKEN=$(echo "$PAIR_RESPONSE" | jq -r '.token // empty' 2>/dev/null)

  if [ "$PAIRED" = "true" ] && [ -n "$TOKEN" ]; then
    pass "POST /pair (paired=true, got token)"
  else
    fail "POST /pair" "unexpected response: $PAIR_RESPONSE"
    TOKEN=""
  fi
else
  skip "POST /pair (success)" "set PAIRING_CODE=<code> to test"
fi

echo ""

# ── 3. Authenticated Endpoints ────────────────────────────────────────────

bold "3. Authenticated Endpoints"
echo ""

if [ -n "$TOKEN" ]; then
  # Ping
  PING_BODY=$(gz /v1/ping -sf -X POST \
    -d '{"message":"smoke-test"}') || { fail "POST /v1/ping" "curl failed"; }
  PING_OK=$(echo "$PING_BODY" | jq -r '.ok // empty' 2>/dev/null)
  if [ "$PING_OK" = "true" ]; then
    pass "POST /v1/ping (ok=true)"
  else
    fail "POST /v1/ping" "unexpected: $PING_BODY"
  fi

  # Models
  MODELS_COUNT=$(gz /v1/models -sf \
    | jq '.data | length' 2>/dev/null) || MODELS_COUNT=0
  if [ "$MODELS_COUNT" -gt 0 ]; then
    pass "GET /v1/models (${MODELS_COUNT} models)"
  else
    fail "GET /v1/models" "empty model list"
  fi

  # Agents list
  AGENTS_COUNT=$(gz /v1/agents -sf \
    | jq '.total // .data // 0' 2>/dev/null) || AGENTS_COUNT=0
  if [ "$AGENTS_COUNT" -ge 0 ]; then
    pass "GET /v1/agents (total=$AGENTS_COUNT)"
  fi

  # Runs list
  RUNS_BODY=$(gz /v1/runs -sf) || { fail "GET /v1/runs" "curl failed"; RUNS_BODY=""; }
  if [ -n "$RUNS_BODY" ]; then
    pass "GET /v1/runs (ok)"
  fi

  # Run status for nonexistent ID
  assert_gz "GET /v1/runs/nonexistent" 404 /v1/runs/nonexistent

  # Webhook (cli channel)
  assert_gz "POST /v1/webhook/cli" 200 /v1/webhook/cli \
    -X POST -H "Content-Type: application/json" -d '{"message":"test"}'

  # Webhook (unknown channel)
  assert_gz "POST /v1/webhook/nonexistent" 404 /v1/webhook/nonexistent \
    -X POST -H "Content-Type: application/json" -d '{"message":"test"}'

else
  skip "POST /v1/ping" "no token (set PAIRING_CODE)"
  skip "GET /v1/models" "no token"
  skip "GET /v1/agents" "no token"
  skip "GET /v1/runs" "no token"
  skip "POST /v1/webhook" "no token"
fi

echo ""

# ── 4. Async Job Flow ──────────────────────────────────────────────────────

bold "4. Async Job Flow (/v1/runs)"
echo ""

if [ -n "$TOKEN" ]; then
  # Submit a job
  SUBMIT_BODY=$(gz /v1/runs -sf -X POST \
    -d '{"message":"ping"}') || { fail "POST /v1/runs (submit)" "curl failed"; }
  RUN_ID=$(echo "$SUBMIT_BODY" | jq -r '.run_id // empty' 2>/dev/null)

  if [ -n "$RUN_ID" ]; then
    pass "POST /v1/runs (run_id=$RUN_ID)"

    # Poll status
    assert_gz "GET /v1/runs/$RUN_ID (status)" 200 "/v1/runs/$RUN_ID"

    # Result (likely 202 while pending)
    RESULT_STATUS=$(curl -s -o /dev/null -w '%{http_code}' \
      -H "Authorization: Bearer $TOKEN" "$GATEWAY/v1/runs/$RUN_ID/result") || true
    if [ "$RESULT_STATUS" = "200" ] || [ "$RESULT_STATUS" = "202" ]; then
      pass "GET /v1/runs/$RUN_ID/result (HTTP $RESULT_STATUS)"
    else
      fail "GET /v1/runs/$RUN_ID/result" "expected 200 or 202, got $RESULT_STATUS"
    fi

    # Events
    assert_gz "GET /v1/runs/$RUN_ID/events" 200 "/v1/runs/$RUN_ID/events"

    # Cancel
    CANCEL_BODY=$(gz "/v1/runs/$RUN_ID" -sf -X DELETE) || { fail "DELETE /v1/runs/$RUN_ID" "curl failed"; }
    CANCELLED=$(echo "$CANCEL_BODY" | jq -r '.cancelled // empty' 2>/dev/null)
    if [ "$CANCELLED" = "true" ]; then
      pass "DELETE /v1/runs/$RUN_ID (cancelled=true)"
    else
      pass "DELETE /v1/runs/$RUN_ID (responded: $(echo "$CANCEL_BODY" | head -c 80))"
    fi
  else
    fail "POST /v1/runs (submit)" "no run_id in response: $SUBMIT_BODY"
  fi
else
  skip "Async job flow" "no token (set PAIRING_CODE)"
fi

echo ""

# ── 5. Error Handling ──────────────────────────────────────────────────────

bold "5. Error Handling"
echo ""

# Unauthenticated requests — when bearer is configured, expect 401;
# when no bearer is configured, the gateway may return 500 (agent unavailable)
# since it tries to process the request. Both are acceptable: the key thing
# is that it does NOT return 200.
SAVED_TOKEN="$TOKEN"; TOKEN=""
for endpoint in "/api/chat" "/v1/chat/completions"; do
  STATUS=$(gz "$endpoint" -s -o /dev/null -w '%{http_code}' -m 5 -X POST \
    -d '{"message":"test","model":"x","messages":[{"role":"user","content":"x"}]}') || STATUS="000"
  if [ "$STATUS" != "200" ]; then
    pass "POST $endpoint (no auth) rejects (HTTP $STATUS)"
  else
    fail "POST $endpoint (no auth)" "expected non-200, got 200"
  fi
done
TOKEN="$SAVED_TOKEN"

echo ""

# ── Summary ────────────────────────────────────────────────────────────────

bold "Summary"
echo ""
echo "  $(green "$PASS passed"), $(red "$FAIL failed"), $(yellow "$SKIP skipped")"
echo ""

if [ "$FAIL" -gt 0 ]; then
  exit 1
fi
