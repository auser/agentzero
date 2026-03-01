#!/usr/bin/env bash
# Self-contained gateway endpoint test script.
#
# Starts the gateway with a known bearer token, tests every HTTP
# endpoint, then shuts it down. No manual pairing code needed.
#
# Usage:
#   ./scripts/test-gateway-manual.sh              # default port 9090
#   ./scripts/test-gateway-manual.sh 8888          # custom port

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

PASS=0
FAIL=0
SKIP=0

pass() { ((PASS++)); echo -e "  ${GREEN}PASS${NC} $1"; }
fail() { ((FAIL++)); echo -e "  ${RED}FAIL${NC} $1 — $2"; }
skip() { ((SKIP++)); echo -e "  ${YELLOW}SKIP${NC} $1 — $2"; }
header() { echo -e "\n${CYAN}=== $1 ===${NC}"; }

PORT="${1:-9090}"
BASE="http://127.0.0.1:${PORT}"
BEARER="az-test-token-$$"
AUTH_HEADER="Authorization: Bearer ${BEARER}"
GW_PID=""

cleanup_gateway() {
    if [[ -n "$GW_PID" ]]; then
        kill "$GW_PID" 2>/dev/null || true
        wait "$GW_PID" 2>/dev/null || true
    fi
    rm -f /tmp/az-gw-*.json /tmp/az-gw-*.html /tmp/az-gw-*.txt
}
trap cleanup_gateway EXIT

# ============================================================
header "Starting gateway on port ${PORT}"
# ============================================================

echo "  Building gateway..."
cargo build --quiet 2>&1

echo "  Launching gateway..."
AGENTZERO_GATEWAY_BEARER_TOKEN="$BEARER" \
    cargo run --quiet -- gateway --port "$PORT" > /dev/null 2>&1 &
GW_PID=$!

# Wait for gateway to be ready (up to 15s)
READY=false
for i in $(seq 1 60); do
    if curl -s -o /dev/null -w '' "${BASE}/health" 2>/dev/null; then
        READY=true
        break
    fi
    sleep 0.25
done

if ! $READY; then
    echo -e "  ${RED}ERROR: Gateway did not start within 15s${NC}"
    exit 1
fi
echo -e "  ${GREEN}Gateway running (PID $GW_PID)${NC}"
echo -e "  Bearer token: ${CYAN}${BEARER}${NC}"

# ============================================================
header "1. Health Check (unauthenticated)"
# ============================================================

HTTP_CODE=$(curl -s -o /tmp/az-gw-health.json -w '%{http_code}' "${BASE}/health")
if [[ "$HTTP_CODE" == "200" ]]; then
    STATUS=$(jq -r '.status' /tmp/az-gw-health.json)
    SERVICE=$(jq -r '.service' /tmp/az-gw-health.json)
    if [[ "$STATUS" == "ok" ]]; then
        pass "GET /health — status=$STATUS service=$SERVICE"
    else
        fail "GET /health" "status='$STATUS', expected 'ok'"
    fi
else
    fail "GET /health" "HTTP $HTTP_CODE"
fi

# ============================================================
header "2. Dashboard (unauthenticated)"
# ============================================================

HTTP_CODE=$(curl -s -o /tmp/az-gw-dash.html -w '%{http_code}' "${BASE}/")
if [[ "$HTTP_CODE" == "200" ]]; then
    if grep -q "agentzero-gateway" /tmp/az-gw-dash.html; then
        pass "GET / — dashboard HTML contains service name"
    else
        fail "GET /" "HTML does not contain 'agentzero-gateway'"
    fi
else
    fail "GET /" "HTTP $HTTP_CODE"
fi

# ============================================================
header "3. Metrics (unauthenticated)"
# ============================================================

HTTP_CODE=$(curl -s -o /tmp/az-gw-metrics.txt -w '%{http_code}' "${BASE}/metrics")
if [[ "$HTTP_CODE" == "200" ]]; then
    if grep -q "agentzero_gateway_requests_total" /tmp/az-gw-metrics.txt; then
        pass "GET /metrics — Prometheus-format metrics returned"
    else
        fail "GET /metrics" "missing expected metric name"
    fi
else
    fail "GET /metrics" "HTTP $HTTP_CODE"
fi

# ============================================================
header "4. Pairing Flow (negative cases)"
# ============================================================

# 4a. Wrong pairing code should fail
HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "${BASE}/pair" \
    -H "Content-Type: application/json" \
    -H "X-Pairing-Code: 000000" \
    -d '{}')
if [[ "$HTTP_CODE" == "401" ]]; then
    pass "POST /pair (wrong code) — 401 Unauthorized"
else
    fail "POST /pair (wrong code)" "HTTP $HTTP_CODE, expected 401"
fi

# 4b. Missing header should fail
HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "${BASE}/pair" \
    -H "Content-Type: application/json" \
    -d '{}')
if [[ "$HTTP_CODE" == "401" ]]; then
    pass "POST /pair (no header) — 401 Unauthorized"
else
    fail "POST /pair (no header)" "HTTP $HTTP_CODE, expected 401"
fi

# ============================================================
header "5. POST /v1/ping"
# ============================================================

HTTP_CODE=$(curl -s -o /tmp/az-gw-ping.json -w '%{http_code}' \
    -X POST "${BASE}/v1/ping" \
    -H "Content-Type: application/json" \
    -H "${AUTH_HEADER}" \
    -d '{"message":"hello from test script"}')
if [[ "$HTTP_CODE" == "200" ]]; then
    OK=$(jq -r '.ok' /tmp/az-gw-ping.json)
    ECHO=$(jq -r '.echo' /tmp/az-gw-ping.json)
    if [[ "$OK" == "true" && "$ECHO" == "hello from test script" ]]; then
        pass "POST /v1/ping — ok=true echo matches"
    else
        fail "POST /v1/ping" "ok=$OK echo=$ECHO"
    fi
else
    fail "POST /v1/ping" "HTTP $HTTP_CODE"
fi

# 5b. Ping without auth should fail
HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "${BASE}/v1/ping" \
    -H "Content-Type: application/json" \
    -d '{"message":"no auth"}')
if [[ "$HTTP_CODE" == "401" ]]; then
    pass "POST /v1/ping (no auth) — 401 Unauthorized"
else
    fail "POST /v1/ping (no auth)" "HTTP $HTTP_CODE, expected 401"
fi

# ============================================================
header "6. POST /api/chat"
# ============================================================

HTTP_CODE=$(curl -s -o /tmp/az-gw-chat.json -w '%{http_code}' \
    -X POST "${BASE}/api/chat" \
    -H "Content-Type: application/json" \
    -H "${AUTH_HEADER}" \
    -d '{"message":"What is 2+2?","context":[]}')
if [[ "$HTTP_CODE" == "200" ]]; then
    MSG=$(jq -r '.message' /tmp/az-gw-chat.json)
    TOKENS=$(jq -r '.tokens_used_estimate' /tmp/az-gw-chat.json)
    if [[ -n "$MSG" && "$TOKENS" -gt 0 ]]; then
        pass "POST /api/chat — message='$MSG' tokens=$TOKENS"
    else
        fail "POST /api/chat" "message='$MSG' tokens=$TOKENS"
    fi
else
    fail "POST /api/chat" "HTTP $HTTP_CODE"
fi

# ============================================================
header "7. POST /webhook (legacy)"
# ============================================================

HTTP_CODE=$(curl -s -o /tmp/az-gw-webhook-legacy.json -w '%{http_code}' \
    -X POST "${BASE}/webhook" \
    -H "Content-Type: application/json" \
    -H "${AUTH_HEADER}" \
    -d '{"message":"webhook test","context":[]}')
if [[ "$HTTP_CODE" == "200" ]]; then
    MSG=$(jq -r '.message' /tmp/az-gw-webhook-legacy.json)
    if [[ "$MSG" == echo:* ]]; then
        pass "POST /webhook (legacy) — message='$MSG'"
    else
        fail "POST /webhook (legacy)" "unexpected message='$MSG'"
    fi
else
    fail "POST /webhook (legacy)" "HTTP $HTTP_CODE"
fi

# ============================================================
header "8. POST /v1/chat/completions (OpenAI-compatible)"
# ============================================================

HTTP_CODE=$(curl -s -o /tmp/az-gw-completions.json -w '%{http_code}' \
    -X POST "${BASE}/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -H "${AUTH_HEADER}" \
    -d '{
        "model": "gpt-4o-mini",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Say hello"}
        ]
    }')
if [[ "$HTTP_CODE" == "200" ]]; then
    OBJ=$(jq -r '.object' /tmp/az-gw-completions.json)
    CONTENT=$(jq -r '.choices[0].message.content' /tmp/az-gw-completions.json)
    ROLE=$(jq -r '.choices[0].message.role' /tmp/az-gw-completions.json)
    REASON=$(jq -r '.choices[0].finish_reason' /tmp/az-gw-completions.json)
    if [[ "$OBJ" == "chat.completion" && "$ROLE" == "assistant" && "$REASON" == "stop" ]]; then
        pass "POST /v1/chat/completions — content='$CONTENT'"
    else
        fail "POST /v1/chat/completions" "object=$OBJ role=$ROLE reason=$REASON"
    fi
else
    fail "POST /v1/chat/completions" "HTTP $HTTP_CODE"
fi

# 8b. Without model field (should default to gpt-4o-mini)
HTTP_CODE=$(curl -s -o /tmp/az-gw-completions-nomodel.json -w '%{http_code}' \
    -X POST "${BASE}/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -H "${AUTH_HEADER}" \
    -d '{"messages": [{"role": "user", "content": "Hello"}]}')
if [[ "$HTTP_CODE" == "200" ]]; then
    CONTENT=$(jq -r '.choices[0].message.content' /tmp/az-gw-completions-nomodel.json)
    if [[ "$CONTENT" == "(gpt-4o-mini)"* ]]; then
        pass "POST /v1/chat/completions (no model) — defaults to gpt-4o-mini"
    else
        fail "POST /v1/chat/completions (no model)" "content='$CONTENT', expected gpt-4o-mini prefix"
    fi
else
    fail "POST /v1/chat/completions (no model)" "HTTP $HTTP_CODE"
fi

# ============================================================
header "9. GET /v1/models"
# ============================================================

HTTP_CODE=$(curl -s -o /tmp/az-gw-models.json -w '%{http_code}' \
    -H "${AUTH_HEADER}" \
    "${BASE}/v1/models")
if [[ "$HTTP_CODE" == "200" ]]; then
    OBJ=$(jq -r '.object' /tmp/az-gw-models.json)
    COUNT=$(jq '.data | length' /tmp/az-gw-models.json)
    FIRST=$(jq -r '.data[0].id' /tmp/az-gw-models.json)
    if [[ "$OBJ" == "list" && "$COUNT" -ge 2 ]]; then
        pass "GET /v1/models — object=$OBJ count=$COUNT first=$FIRST"
    else
        fail "GET /v1/models" "object=$OBJ count=$COUNT"
    fi
else
    fail "GET /v1/models" "HTTP $HTTP_CODE"
fi

# ============================================================
header "10. POST /v1/webhook/:channel"
# ============================================================

HTTP_CODE=$(curl -s -o /tmp/az-gw-webhook-ch.json -w '%{http_code}' \
    -X POST "${BASE}/v1/webhook/telegram" \
    -H "Content-Type: application/json" \
    -H "${AUTH_HEADER}" \
    -d '{"update_id": 12345, "message": {"text": "test"}}')
# Channel may not be configured — 404 is acceptable
if [[ "$HTTP_CODE" == "200" ]]; then
    ACCEPTED=$(jq -r '.accepted' /tmp/az-gw-webhook-ch.json)
    pass "POST /v1/webhook/telegram — accepted=$ACCEPTED"
elif [[ "$HTTP_CODE" == "404" ]]; then
    pass "POST /v1/webhook/telegram — 404 (channel not configured, expected)"
else
    fail "POST /v1/webhook/telegram" "HTTP $HTTP_CODE"
fi

# ============================================================
header "11. GET /api/*path (fallback)"
# ============================================================

HTTP_CODE=$(curl -s -o /tmp/az-gw-fallback.json -w '%{http_code}' \
    -H "${AUTH_HEADER}" \
    "${BASE}/api/some/custom/path")
if [[ "$HTTP_CODE" == "200" ]]; then
    OK=$(jq -r '.ok' /tmp/az-gw-fallback.json)
    PATH_VAL=$(jq -r '.path' /tmp/az-gw-fallback.json)
    if [[ "$OK" == "true" ]]; then
        pass "GET /api/some/custom/path — ok=$OK path=$PATH_VAL"
    else
        fail "GET /api/*path" "ok=$OK"
    fi
else
    fail "GET /api/*path" "HTTP $HTTP_CODE"
fi

# ============================================================
header "12. WebSocket: /ws/chat"
# ============================================================

if command -v websocat &>/dev/null; then
    # Use -H= format to prevent websocat from eating the URL as a header arg
    WS_REPLY=$(echo "hello ws" | timeout 3 websocat -1 \
        -H="${AUTH_HEADER}" \
        "ws://127.0.0.1:${PORT}/ws/chat" 2>/dev/null || true)
    if [[ "$WS_REPLY" == "echo: hello ws" ]]; then
        pass "WS /ws/chat — echo reply matches"
    elif [[ -n "$WS_REPLY" ]]; then
        fail "WS /ws/chat" "reply='$WS_REPLY', expected 'echo: hello ws'"
    else
        fail "WS /ws/chat" "no reply (timeout or connection error)"
    fi
else
    skip "WS /ws/chat" "websocat not installed (brew install websocat)"
fi

# ============================================================
header "13. Auth Rejection Tests"
# ============================================================

# Bad token on authenticated endpoint
HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer bad_token_12345" \
    "${BASE}/v1/models")
if [[ "$HTTP_CODE" == "401" ]]; then
    pass "GET /v1/models (bad token) — 401 Unauthorized"
else
    fail "GET /v1/models (bad token)" "HTTP $HTTP_CODE, expected 401"
fi

# No auth on models
HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' "${BASE}/v1/models")
if [[ "$HTTP_CODE" == "401" ]]; then
    pass "GET /v1/models (no auth) — 401 Unauthorized"
else
    fail "GET /v1/models (no auth)" "HTTP $HTTP_CODE, expected 401"
fi

# No auth on chat completions
HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "${BASE}/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -d '{"messages":[{"role":"user","content":"hi"}]}')
if [[ "$HTTP_CODE" == "401" ]]; then
    pass "POST /v1/chat/completions (no auth) — 401 Unauthorized"
else
    fail "POST /v1/chat/completions (no auth)" "HTTP $HTTP_CODE, expected 401"
fi

# No auth on api fallback (always_require_pairing=true)
HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' "${BASE}/api/test")
if [[ "$HTTP_CODE" == "401" ]]; then
    pass "GET /api/test (no auth) — 401 Unauthorized"
else
    fail "GET /api/test (no auth)" "HTTP $HTTP_CODE, expected 401"
fi

# ============================================================
# Summary
# ============================================================

echo ""
echo -e "${CYAN}========================================${NC}"
echo -e "  Results: ${GREEN}${PASS} passed${NC}, ${RED}${FAIL} failed${NC}, ${YELLOW}${SKIP} skipped${NC}"
echo -e "${CYAN}========================================${NC}"

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
