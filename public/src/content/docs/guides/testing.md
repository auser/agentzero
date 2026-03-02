---
title: Manual Test Procedures
description: Manual test procedures for verifying live provider flows, gateway operations, and daemon lifecycle.
---

These procedures verify behavior that requires live infrastructure (API keys, running services) and cannot be fully covered by unit tests.

## Prerequisites

- A working `agentzero` binary on your PATH
- An `agentzero.toml` config (run `agentzero onboard` if needed)
- At least one provider API key configured

---

## P1. Provider Authentication

### P1.1 Token-based auth (OpenAI, Anthropic, OpenRouter)

```bash
# Store a token
agentzero auth setup-token --provider openrouter --token sk-or-v1-...

# Verify it's stored
agentzero auth list
# Expected: shows openrouter profile with "active" status

# Verify it resolves
agentzero auth status
# Expected: shows active provider and profile

# Send a test message using the stored token
agentzero agent -m "Say hello in exactly 3 words"
# Expected: response from the provider
```

**Pass criteria:** Message returns a valid response. No "missing API key" error.

### P1.2 Environment variable auth

```bash
# Unset any stored tokens first
agentzero auth logout --provider openrouter

# Set via env var
export OPENROUTER_API_KEY="sk-or-v1-..."

# Verify it works
agentzero agent -m "Say hello in exactly 3 words"
# Expected: response from the provider
```

**Pass criteria:** Provider resolves the key from the environment variable.

### P1.3 Local provider (no auth)

```bash
# Ensure Ollama is running
ollama serve &
ollama pull llama3.1:8b

# Configure
agentzero onboard --provider ollama --model llama3.1:8b --yes

# Test
agentzero agent -m "What is 2+2?"
# Expected: response without any API key
```

**Pass criteria:** Response received with no authentication configured.

---

## P2. Provider Streaming

### P2.1 Verify streaming output

```bash
# Enable verbose output to see streaming events
agentzero -vvv agent -m "Write a haiku about rust programming"
```

**Pass criteria:** Output appears incrementally (not all at once after a delay). Debug logs show `StreamChunk` events.

---

## P3. Gateway Operations

### P3.1 Start and health check

```bash
# Start gateway in foreground
agentzero gateway --host 127.0.0.1 --port 18080 &
GATEWAY_PID=$!

# Wait for startup
sleep 2

# Health check
curl -s http://127.0.0.1:18080/health
# Expected: {"status":"ok"}

# Clean up
kill $GATEWAY_PID
```

**Pass criteria:** `/health` returns 200 with `{"status":"ok"}`.

### P3.2 Pairing flow

```bash
# Start gateway (note the pairing code in output)
agentzero gateway --host 127.0.0.1 --port 18080 --new-pairing &
GATEWAY_PID=$!
sleep 2

# Exchange pairing code for bearer token
# (replace PAIRING_CODE with the code from gateway output)
TOKEN=$(curl -s -X POST http://127.0.0.1:18080/pair \
  -H "Content-Type: application/json" \
  -d '{"code":"PAIRING_CODE"}' | jq -r '.token')

echo "Token: $TOKEN"

# Use the token for an authenticated request
curl -s -X POST http://127.0.0.1:18080/v1/ping \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json"
# Expected: 200 response

# Verify unauthenticated request is rejected
curl -s -o /dev/null -w "%{http_code}" \
  -X POST http://127.0.0.1:18080/v1/ping
# Expected: 401

kill $GATEWAY_PID
```

**Pass criteria:** Pairing returns a token. Token grants access. Missing token returns 401.

### P3.3 OpenAI-compatible completions

```bash
# With a running gateway and valid bearer token:
curl -s -X POST http://127.0.0.1:18080/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "anthropic/claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Say hi"}]
  }'
# Expected: OpenAI-format response with choices[0].message.content
```

**Pass criteria:** Response follows OpenAI chat completions format.

### P3.4 WebSocket chat

```bash
# Using websocat (install: cargo install websocat)
echo '{"message":"Hello"}' | websocat ws://127.0.0.1:18080/ws/chat \
  -H "Authorization: Bearer $TOKEN"
# Expected: streaming response over WebSocket
```

**Pass criteria:** WebSocket connection established. Response messages received.

---

## P4. Daemon Lifecycle

### P4.1 Start, status, stop

```bash
# Start daemon
agentzero daemon start --port 18080
# Expected: "daemon started" message with PID

# Check status
agentzero daemon status
# Expected: shows running=true, PID, host, port, uptime

# JSON status
agentzero daemon status --json
# Expected: JSON with running, pid, host, port, started_at_epoch_seconds

# Verify gateway is accessible
curl -s http://127.0.0.1:18080/health
# Expected: {"status":"ok"}

# Stop daemon
agentzero daemon stop
# Expected: "daemon stopped" message

# Verify it's stopped
agentzero daemon status
# Expected: shows running=false
```

**Pass criteria:** Full lifecycle completes without errors.

### P4.2 Stale state recovery

```bash
# Start daemon
agentzero daemon start --port 18080

# Force-kill without clean shutdown
kill -9 $(agentzero daemon status --json | jq '.pid')

# Status should auto-correct
agentzero daemon status
# Expected: running=false (auto-corrected from stale state)

# Should be able to restart
agentzero daemon start --port 18080
agentzero daemon stop
```

**Pass criteria:** Stale state is detected and corrected. Restart succeeds after crash.

---

## P5. Model Discovery

### P5.1 Model listing and refresh

```bash
# Refresh model catalog from provider
agentzero models refresh

# List available models
agentzero models list
# Expected: table of models with provider, ID, capabilities

# Check specific provider
agentzero doctor models
# Expected: shows reachability and model availability per provider
```

**Pass criteria:** Models list is populated. Doctor shows provider health status.

---

## P6. Multi-Provider Switching

### P6.1 Profile switching

```bash
# Set up two providers
agentzero auth setup-token --provider openai --token sk-...
agentzero auth setup-token --provider anthropic --token sk-ant-...

# Use each provider explicitly
agentzero agent -m "Who are you?" --provider openai --model gpt-4o-mini
agentzero agent -m "Who are you?" --provider anthropic --model claude-haiku-4-5-20251001

# Switch active profile
agentzero auth use --provider anthropic --profile default
agentzero auth status
# Expected: anthropic is now active
```

**Pass criteria:** Both providers respond. Profile switching changes the default.

---

## Test Results Template

Record results using this format:

| Test | Date | Result | Notes |
|---|---|---|---|
| P1.1 Token auth | | PASS/FAIL | |
| P1.2 Env var auth | | PASS/FAIL | |
| P1.3 Local provider | | PASS/FAIL | |
| P2.1 Streaming | | PASS/FAIL | |
| P3.1 Gateway health | | PASS/FAIL | |
| P3.2 Pairing flow | | PASS/FAIL | |
| P3.3 Completions API | | PASS/FAIL | |
| P3.4 WebSocket chat | | PASS/FAIL | |
| P4.1 Daemon lifecycle | | PASS/FAIL | |
| P4.2 Stale recovery | | PASS/FAIL | |
| P5.1 Model discovery | | PASS/FAIL | |
| P6.1 Profile switching | | PASS/FAIL | |
