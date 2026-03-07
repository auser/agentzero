# Plan 07: Distributed Event Bus & Horizontal Scaling

## Problem

The multi-agent orchestrator uses `InMemoryBus` (backed by `tokio::sync::broadcast`) — a single-process, in-memory event bus. This means:

- **No horizontal scaling**: All agents, the coordinator, and the gateway must run in one process
- **No persistence**: Events are lost on crash/restart — in-flight agent chains disappear
- **No cross-node communication**: Multiple AgentZero instances cannot cooperate
- **Bus capacity limits**: `broadcast` channel has fixed capacity (256 default); lagged consumers lose events

The `EventBus` trait is already designed for pluggable backends (the code comments mention "a future multi-node implementation"), but only `InMemoryBus` exists.

## Current State

### EventBus trait (`crates/agentzero-core/src/event_bus.rs`)
```rust
#[async_trait]
pub trait EventBus: Send + Sync {
    async fn publish(&self, event: Event) -> Result<()>;
    async fn subscribe(&self, topic_filter: &str) -> Result<Box<dyn EventSubscriber>>;
    fn subscriber_count(&self) -> usize;
}

#[async_trait]
pub trait EventSubscriber: Send + Sync {
    async fn recv(&mut self) -> Result<Event>;
    async fn recv_filtered(&mut self, filter: &str) -> Result<Event>;
}
```

### Event struct
```rust
pub struct Event {
    pub id: String,
    pub topic: String,
    pub payload: String,
    pub source_agent: Option<String>,
    pub correlation_id: Option<String>,
    pub privacy_boundary: Option<String>,
    pub timestamp: SystemTime,
}
```

### InMemoryBus (`crates/agentzero-core/src/event_bus.rs`)
- `tokio::sync::broadcast` with configurable capacity
- Topic matching via `topic_matches()` helper (supports `agent.output.*` patterns)
- Privacy boundary checking via `is_boundary_compatible()`
- Lagged consumer handling: skips to latest on `RecvError::Lagged`

### Orchestrator (`crates/agentzero-orchestrator/`)
- `Coordinator`: 3 concurrent loops (ingestion, routing, response handler)
- `AgentRouter`: LLM-based or keyword-based message classification
- Agent workers: `tokio::spawn` tasks receiving via `mpsc`
- Pipeline executor: sequential steps with error strategies
- All communication through `EventBus` trait (already abstracted)

### Gateway integration
- `build_swarm()` in orchestrator creates `InMemoryBus` + agents + coordinator
- Gateway calls into orchestrator

## Implementation

### Phase 1: Redis-Backed EventBus

Redis Pub/Sub is the natural first distributed backend — low latency, simple protocol, widely deployed.

**New crate: `crates/agentzero-bus-redis/` or feature-gated module in `agentzero-core`**

Decision: Feature-gated in `agentzero-core` to avoid a new crate (the trait is there, the impl should be close).

**Dependencies:**
```toml
[dependencies]
redis = { version = "0.27", features = ["tokio-comp", "aio"], optional = true }
```

**Feature:**
```toml
[features]
bus-redis = ["dep:redis"]
```

**Implementation: `crates/agentzero-core/src/redis_bus.rs`**

```rust
pub struct RedisBus {
    client: redis::Client,
    connection: Arc<Mutex<redis::aio::MultiplexedConnection>>,
    capacity: usize,
}

impl RedisBus {
    pub async fn new(redis_url: &str, capacity: usize) -> Result<Self> { ... }
}

#[async_trait]
impl EventBus for RedisBus {
    async fn publish(&self, event: Event) -> Result<()> {
        // Serialize Event to JSON
        // PUBLISH to Redis channel: "agentzero:{topic}"
        // Also LPUSH to "agentzero:events:{topic}" for persistence (capped list)
    }

    async fn subscribe(&self, topic_filter: &str) -> Result<Box<dyn EventSubscriber>> {
        // PSUBSCRIBE to "agentzero:{pattern}"
        // Return RedisSubscriber wrapping the pubsub connection
    }
}

struct RedisSubscriber {
    pubsub: redis::aio::PubSub,
    filter: String,
}

#[async_trait]
impl EventSubscriber for RedisSubscriber {
    async fn recv(&mut self) -> Result<Event> {
        // Receive from Redis pubsub
        // Deserialize JSON → Event
    }

    async fn recv_filtered(&mut self, filter: &str) -> Result<Event> {
        // Filter on topic pattern (Redis PSUBSCRIBE already does pattern matching)
        // Additional privacy boundary check
    }
}
```

### Phase 2: Event Persistence (Optional)

For crash recovery, persist events to Redis lists:

```rust
// On publish: also store in a capped list
redis_conn.lpush(format!("agentzero:events:{}", event.topic), &serialized)?;
redis_conn.ltrim(format!("agentzero:events:{}", event.topic), 0, 999)?; // keep last 1000
```

On restart, replay recent events to catch up on in-flight chains. The `correlation_id` field enables deduplication.

### Phase 3: Configuration

**Add to config model (`crates/agentzero-config/src/model.rs`):**

```rust
pub struct EventBusConfig {
    /// Backend: "memory" (default) or "redis"
    #[serde(default = "default_bus_backend")]
    pub backend: BusBackend,

    /// Redis URL (required when backend = "redis")
    pub redis_url: Option<String>,

    /// Channel capacity (for both memory and redis backends)
    #[serde(default = "default_bus_capacity")]
    pub capacity: usize,
}

pub enum BusBackend {
    Memory,
    Redis,
}
```

**TOML:**
```toml
[event_bus]
backend = "redis"
redis_url = "redis://localhost:6379"
capacity = 1024
```

**Environment:**
```
AGENTZERO__EVENT_BUS__BACKEND=redis
AGENTZERO__EVENT_BUS__REDIS_URL=redis://localhost:6379
```

### Phase 4: Orchestrator Changes

Update `build_swarm()` in `crates/agentzero-orchestrator/src/lib.rs`:

```rust
pub async fn build_swarm(config: &SwarmConfig, bus_config: &EventBusConfig) -> Result<...> {
    let bus: Arc<dyn EventBus> = match bus_config.backend {
        BusBackend::Memory => Arc::new(InMemoryBus::new(bus_config.capacity)),
        BusBackend::Redis => {
            let url = bus_config.redis_url.as_deref()
                .ok_or_else(|| anyhow!("redis_url required when backend = redis"))?;
            Arc::new(RedisBus::new(url, bus_config.capacity).await?)
        }
    };
    // Rest unchanged — Coordinator, AgentRouter, workers all use `dyn EventBus`
}
```

Because the orchestrator already uses `Arc<dyn EventBus>`, this is a clean swap with no refactoring needed.

### Phase 5: Horizontal Scaling Support

With Redis as the bus, multiple AgentZero instances can:
1. **Share the event bus**: All instances publish/subscribe to the same Redis
2. **Route to any agent**: Agent workers on any node can pick up events
3. **Distribute load**: Incoming requests hit any instance; responses route back via correlation_id

Additional requirements for full horizontal scaling:
- **Sticky sessions** (or shared state): WebSocket connections need session affinity OR shared session state
- **Distributed rate limiting**: Replace `AtomicU64` counter with Redis-backed sliding window (`INCR` + `EXPIRE`)
- **Shared agent state**: Agent definitions stored in Redis or shared DB, not just in-memory

These are follow-up items, not part of the initial Redis bus implementation.

### Phase 6: NATS Alternative (Future)

NATS is a better fit for high-throughput scenarios (JetStream provides persistence + exactly-once delivery). The `EventBus` trait makes this a clean addition:

```rust
pub struct NatsBus { ... }
impl EventBus for NatsBus { ... }
```

Config: `backend = "nats"`, `nats_url = "nats://localhost:4222"`

This is out of scope for the initial implementation but the architecture supports it.

## Files to Create/Modify

| File | Action |
|------|--------|
| `crates/agentzero-core/Cargo.toml` | Add redis dep (feature-gated) |
| `crates/agentzero-core/src/redis_bus.rs` | New: RedisBus implementation |
| `crates/agentzero-core/src/lib.rs` | Add `#[cfg(feature = "bus-redis")] pub mod redis_bus;` |
| `crates/agentzero-config/src/model.rs` | Add EventBusConfig |
| `crates/agentzero-orchestrator/src/lib.rs` | Bus backend selection in build_swarm() |
| `docker-compose.yml` | Add optional Redis service |
| `Justfile` | Add `build-distributed` recipe |

## Tests (~15 new)

### Unit tests (no Redis required)
1. RedisBus config validation: redis_url required when backend = redis
2. BusBackend serde: "memory" and "redis" round-trip
3. Event JSON serialization/deserialization
4. InMemoryBus still works (regression)

### Integration tests (require Redis, `#[ignore]`)
5. RedisBus publish → subscribe receives event
6. RedisBus topic pattern matching (wildcard subscriptions)
7. RedisBus privacy boundary filtering
8. RedisBus multiple subscribers receive same event
9. RedisBus lagged consumer handling
10. RedisBus correlation_id preserved across publish/subscribe
11. Cross-instance communication: two RedisBus instances, publish on one → receive on other
12. Event persistence: publish, disconnect, reconnect, replay recent events
13. Agent chain via Redis: A→B→C with RedisBus
14. Graceful shutdown with Redis: in-flight events complete
15. Redis connection failure: graceful error, not panic

## Verification

1. `cargo build --features bus-redis` — compiles
2. `cargo build` (no redis feature) — still compiles, no redis dependency
3. Start Redis + AgentZero with `backend = "redis"` → orchestrator works
4. Two AgentZero instances sharing Redis → events route between them
5. Kill one instance → other continues processing
6. All existing `InMemoryBus` tests still pass
7. Integration tests pass with local Redis

## Docker Compose Addition

```yaml
services:
  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis-data:/data

  agentzero:
    environment:
      AGENTZERO__EVENT_BUS__BACKEND: redis
      AGENTZERO__EVENT_BUS__REDIS_URL: redis://redis:6379

volumes:
  redis-data:
```

## Dependencies Added

| Crate | Version | Condition |
|-------|---------|-----------|
| `redis` | 0.27 | `bus-redis` feature |

## Risks

- **Redis availability**: If Redis goes down, the event bus is unavailable. Mitigate: health check Redis in readiness probe (Plan 04), fall back to InMemoryBus with warning.
- **Serialization overhead**: Events serialized to JSON for Redis. For high-throughput, consider MessagePack or protobuf. JSON is fine for initial implementation.
- **Ordering guarantees**: Redis Pub/Sub is fire-and-forget with no ordering guarantees across channels. For strict ordering, use Redis Streams instead of Pub/Sub. Initial implementation uses Pub/Sub (simpler, sufficient for most use cases).
- **Connection management**: Redis connection drops need automatic reconnection. The `redis` crate's `MultiplexedConnection` handles this.

## Dependencies on Other Plans

- **Plan 01 (Containerization)**: Docker Compose gains Redis service
- **Plan 03 (Database)**: Connection pooling complements distributed bus (both reduce contention)
- **Plan 06 (Multi-tenancy)**: Event bus needs tenant isolation — events scoped by `org_id`
