# Incident Response Runbook

## Emergency Stop (E-Stop)

### Engage E-Stop
```bash
# Stop all agent activity immediately
agentzero estop --level full

# Stop specific domains only
agentzero estop --level domain-block --domain "*.external.com"

# Freeze specific tools
agentzero estop --level tool-freeze --tool shell --tool write_file
```

### Check E-Stop Status
```bash
agentzero estop status
```

### Resume After Investigation
```bash
agentzero estop resume
# If OTP was required on engage:
agentzero estop resume --otp <code>
```

## Provider Failover

### Symptoms
- Repeated 429 (rate limit) or 5xx errors in logs
- Circuit breaker open messages: `circuit breaker open for provider`
- Slow or no responses from agent

### Diagnosis
```bash
# Check provider health
agentzero doctor models

# Check circuit breaker state
agentzero providers-quota

# View recent errors
RUST_LOG=agentzero=debug agentzero agent -m "test" 2>&1 | grep -i error
```

### Resolution
1. Check provider status page (Anthropic, OpenAI, OpenRouter)
2. If provider is down, switch provider in config:
   ```bash
   # Edit config
   agentzero config show
   # Change provider.kind and provider.model
   ```
3. If rate-limited, wait for reset or reduce request volume

## Stuck Jobs

### Symptoms
- Agent not responding
- Gateway health check passes but no output
- Memory shows incomplete conversations

### Diagnosis
```bash
# Check daemon status
agentzero daemon status

# Check active jobs
agentzero coordination status

# Check memory for incomplete conversations
agentzero memory list --limit 5
```

### Resolution
1. Kill stuck daemon: `agentzero daemon stop && agentzero daemon start`
2. Clear stuck job: `agentzero coordination cancel <job-id>`
3. If persistent, check disk space and SQLite integrity

## Log Locations

| Component | Location |
|-----------|----------|
| Agent logs | stderr (use `RUST_LOG=agentzero=debug`) |
| Audit log | `./agentzero-audit.log` (if enabled) |
| Gateway access | stderr with `RUST_LOG=agentzero_gateway=info` |
| Event bus | `RUST_LOG=agentzero_core::event_bus=debug` |

## Escalation Template

```
Incident: [Brief description]
Time: [When it started]
Impact: [What's affected - agents, channels, users]
E-Stop Status: [engaged/not engaged]
Provider: [which provider, any errors]
Steps Taken: [what you've tried]
Logs: [attach relevant log snippets]
```
