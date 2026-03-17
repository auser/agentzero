# Scaling Runbook

## When to Scale

### Metrics thresholds indicating scale-up need:
- Request queue depth > 20 sustained for > 5 minutes
- Response latency p99 > 30 seconds
- CPU utilization > 80% sustained
- Memory utilization > 85%
- Active concurrent jobs > max_agents * 2

## Horizontal Scaling with Gossip Event Bus

AgentZero supports multi-instance deployment via the gossip event bus.

### Multi-instance setup

1. Configure event bus on each node:
   ```toml
   [swarm]
   event_bus = "sqlite"
   event_db_path = "./events.db"
   event_retention_days = 7

   # Gossip peers (each node lists the others)
   gossip_port = 9000
   gossip_peers = ["node2:9000", "node3:9000"]
   ```

2. Start each node:
   ```bash
   # Node 1
   agentzero gateway --port 8080

   # Node 2
   agentzero gateway --port 8080

   # Node 3
   agentzero gateway --port 8080
   ```

3. Load balance across nodes:
   ```nginx
   upstream agentzero {
       server node1:8080;
       server node2:8080;
       server node3:8080;
   }
   ```

### How gossip works
- Each node broadcasts events to known peers via TCP
- Events are deduplicated by ID (bounded LRU set)
- Peer health monitored via periodic ping
- No leader election — all nodes are equal
- SQLite provides local durability; gossip provides cross-instance awareness

## Lightweight Mode for Edge Nodes

For resource-constrained nodes that participate in the cluster:

```toml
[provider]
kind = "openrouter"
model = "anthropic/claude-haiku-4-5"

[agent]
max_tool_iterations = 5
memory_window_size = 10

[cost]
daily_limit_usd = 1.0
```

Edge nodes use cheap models and limited tools. Heavy computation delegates to full nodes.

## Provider Fallback Chain

Configure multiple providers for resilience:

```toml
[provider]
kind = "anthropic"
model = "claude-sonnet-4-6"

# Fallback providers (tried in order if primary fails)
[[model_routes]]
pattern = "*"
providers = ["anthropic", "openrouter"]
models = ["claude-sonnet-4-6", "anthropic/claude-sonnet-4-6"]
```

### Fallback behavior
1. Primary provider called first
2. On 5xx or timeout, circuit breaker opens
3. Next provider in chain is tried
4. Circuit breaker resets after cooldown period (default 60s)

## Scaling Checklist

- [ ] Multiple gateway instances behind load balancer
- [ ] Gossip event bus configured with peer discovery
- [ ] SQLite event database on each node
- [ ] Provider fallback chain configured
- [ ] Cost limits set per-node and per-agent
- [ ] Monitoring alerts for queue depth and latency
- [ ] Edge nodes using lightweight mode where appropriate
- [ ] Backup schedule configured for each node's database
