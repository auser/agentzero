# Monitoring Setup Runbook

## Prometheus Metrics

AgentZero exposes Prometheus metrics at `GET /metrics` on the gateway.

### Scrape Configuration
```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'agentzero'
    scrape_interval: 15s
    static_configs:
      - targets: ['localhost:42617']
    metrics_path: '/metrics'
```

### Key Metrics to Alert On

| Metric | Alert Threshold | Description |
|--------|----------------|-------------|
| `provider_errors_total` | > 10/min | LLM provider errors |
| `rate_limit_429_total` | > 5/min | Rate limit hits |
| `circuit_breaker_open` | == 1 | Provider circuit breaker tripped |
| `request_duration_seconds` | p99 > 30s | Slow responses |
| `active_jobs` | > 50 | Job queue backing up |
| `memory_entries_total` | > 100000 | Memory database growing large |
| `cost_daily_usd` | > budget * 0.8 | Approaching daily cost limit |

### Alert Rules (Prometheus)
```yaml
groups:
  - name: agentzero
    rules:
      - alert: ProviderErrors
        expr: rate(provider_errors_total[5m]) > 0.1
        for: 2m
        labels:
          severity: warning
        annotations:
          summary: "AgentZero provider errors elevated"

      - alert: CircuitBreakerOpen
        expr: circuit_breaker_open == 1
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "AgentZero circuit breaker is open — provider unavailable"

      - alert: CostBudgetWarning
        expr: cost_daily_usd > (cost_daily_limit_usd * 0.8)
        labels:
          severity: warning
        annotations:
          summary: "AgentZero approaching daily cost limit"
```

## Health Checks

```bash
# Basic health (static, always responds)
curl http://localhost:42617/health

# Readiness (checks dependencies)
curl http://localhost:42617/health/ready

# Liveness (verifies runtime is responsive)
curl http://localhost:42617/health/live
```

## Grafana Dashboard

Import the following dashboard JSON as a starting point:

```json
{
  "dashboard": {
    "title": "AgentZero Overview",
    "panels": [
      {
        "title": "Request Rate",
        "type": "graph",
        "targets": [{"expr": "rate(http_requests_total[5m])"}]
      },
      {
        "title": "Provider Errors",
        "type": "graph",
        "targets": [{"expr": "rate(provider_errors_total[5m])"}]
      },
      {
        "title": "Response Latency (p99)",
        "type": "graph",
        "targets": [{"expr": "histogram_quantile(0.99, rate(request_duration_seconds_bucket[5m]))"}]
      },
      {
        "title": "Daily Cost ($)",
        "type": "stat",
        "targets": [{"expr": "cost_daily_usd"}]
      },
      {
        "title": "Active Agents",
        "type": "stat",
        "targets": [{"expr": "active_agents"}]
      }
    ]
  }
}
```

## Log Aggregation

For centralized logging, set `RUST_LOG` and pipe to your log aggregator:

```bash
# JSON-structured logs
RUST_LOG=agentzero=info agentzero gateway 2>&1 | your-log-shipper

# Or use the logging config
[logging]
format = "json"
level = "info"
```
