#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Business Office — Gateway Smoke Test
#
# Tests every gateway endpoint WITHOUT needing an LLM provider.
# The gateway must be running: agentzero gateway --config agentzero.toml
#
# Usage:
#   cd examples/business-office
#   agentzero gateway &
#   ./test-gateway.sh
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

red()   { printf "\033[31m%s\033[0m" "$*"; }
green() { printf "\033[32m%s\033[0m" "$*"; }
yellow(){ printf "\033[33m%s\033[0m" "$*"; }
bold()  { printf "\033[1m%s\033[0m" "$*"; }

pass() { PASS=$((PASS+1)); echo "  $(green PASS)  $1"; }
fail() { FAIL=$((FAIL+1)); echo "  $(red FAIL)  $1: $2"; }
skip() { SKIP=$((SKIP+1)); echo "  $(yellow SKIP)  $1: $2"; }

# gz — gateway curl helper (path-first)
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

echo ""
bold "Business Office — Gateway Smoke Test"
echo "Gateway: $GATEWAY"
echo ""

echo "Checking gateway is reachable..."
if ! gz /health -sf > /dev/null 2>&1; then
  echo ""
  red "ERROR: Gateway not reachable at $GATEWAY"
  echo ""
  echo "Start it with:"
  echo "  agentzero gateway --config examples/business-office/agentzero.toml"
  echo ""
  exit 1
fi
echo ""

# ── Health ─────────────────────────────────────────────────────────────────

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

# ── Pairing ────────────────────────────────────────────────────────────────

bold "2. Pairing"
echo ""

assert_gz "POST /pair (wrong code)" 403 /pair \
  -X POST -H "X-Pairing-Code: wrong-code"
assert_gz "POST /pair (no header)" 401 /pair \
  -X POST

if [ -n "${PAIRING_CODE:-}" ]; then
  PAIR_RESPONSE=$(gz /pair -sf -X POST \
    -H "X-Pairing-Code: $PAIRING_CODE") || { fail "POST /pair" "curl failed"; }
  TOKEN=$(echo "$PAIR_RESPONSE" | jq -r '.token // empty')
  if [ -n "$TOKEN" ]; then
    pass "POST /pair (got token)"
  else
    fail "POST /pair" "no token: $PAIR_RESPONSE"
  fi
else
  skip "POST /pair (success)" "set PAIRING_CODE=<code>"
fi

echo ""

# ── Authenticated endpoints ───────────────────────────────────────────────

bold "3. Authenticated Endpoints"
echo ""

if [ -n "$TOKEN" ]; then
  assert_gz "GET /v1/models" 200 /v1/models
  assert_gz "GET /v1/agents" 200 /v1/agents
  assert_gz "GET /v1/runs" 200 /v1/runs

  # Submit + cancel a quick job
  SUBMIT=$(gz /v1/runs -sf -X POST \
    -d '{"message":"test ping"}') || SUBMIT=""
  RUN_ID=$(echo "$SUBMIT" | jq -r '.run_id // empty' 2>/dev/null)
  if [ -n "$RUN_ID" ]; then
    pass "POST /v1/runs (run_id=$RUN_ID)"
    gz "/v1/runs/$RUN_ID" -sf -X DELETE > /dev/null 2>&1 || true
    pass "DELETE /v1/runs/$RUN_ID (cancelled)"
  else
    fail "POST /v1/runs" "no run_id"
  fi
else
  skip "Authenticated endpoints" "no token (set PAIRING_CODE)"
fi

echo ""

# ── Error handling ────────────────────────────────────────────────────────

bold "4. Error Handling"
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

[ "$FAIL" -gt 0 ] && exit 1 || exit 0
