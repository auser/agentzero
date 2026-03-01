# AgentZero Backlog — Deferred Items

Items moved here from active sprint plans. These are not currently planned and should only be picked up when there's a concrete need.

---

## Multi-Agent Stack (formerly Phase E)

Deferred: adds significant distributed-systems complexity with no immediate use case. The single-process architecture handles current requirements. Can be revisited if horizontal scaling becomes necessary.

### Coordination Enhancement
- [ ] Extend `CoordinationStatus` with task queue, worker registry, task lifecycle
- [ ] Add `CoordinationManager` with task distribution logic
- [ ] Add worker heartbeat tracking and stale-worker detection

### IPC Layer Enhancement
- [ ] Add structured message types (task assignment, result, status query, capability advertisement)
- [ ] Add message routing by agent name/role
- [ ] Add message TTL and expiration
- [ ] Evaluate transport upgrade (Unix domain socket or shared-memory for same-host)

### Gateway node_control Endpoint
- [ ] Add `[gateway.node_control]` config section (enabled, auth_token, allowed_node_ids)
- [ ] Implement `POST /api/node-control` endpoint
- [ ] Support operations: node.describe, node.invoke
- [ ] Add auth via X-Node-Control-Token header

### Multi-Node Orchestration
- [ ] Create `crates/agentzero-orchestrator/`
- [ ] Implement node registry (capabilities, health status)
- [ ] Implement task routing to best-fit node
- [ ] Implement result aggregation from distributed sub-agents
- [ ] Add failure handling (timeout → reassign)

### Remote Delegation
- [ ] Extend delegate tool with `node` parameter
- [ ] Route remote delegation through node_control endpoint
- [ ] Fallback to local delegation if remote node unavailable

### Acceptance (when picked up)
- [ ] Task queue distributes work to registered workers
- [ ] IPC messages route correctly by agent name
- [ ] node_control endpoint accepts describe/invoke operations
- [ ] Remote delegation falls back to local gracefully

### New Crates (if implemented)
| Crate | Purpose |
|---|---|
| `crates/agentzero-orchestrator/` | Multi-node orchestration, node registry, distributed task routing |
