# Plan 40: Event Bus Improvements — Multi-Axis Filtering, Publish Metrics, Arc Payloads

**Sprint:** 81
**Goal:** Production-harden the event bus with three improvements inspired by the omnibus crate, plus consolidation of the duplicate event bus hierarchy.

## Context

AgentZero had two event bus hierarchies:
1. **Core** (`agentzero-core::EventBus`) — `InMemoryBus`, `FileBackedBus`, `SqliteEventBus`, `GossipEventBus`
2. **Orchestrator** (`agentzero-orchestrator::event_bus::EventBus`) — `InMemoryEventBus`, `FileBackedEventBus`

**Resolution:** The orchestrator's event bus was dead code — all real orchestrator modules already used the core `EventBus`. Deleted `crates/agentzero-orchestrator/src/event_bus.rs` and its re-exports. One unified event bus hierarchy now.

## Phase A: Multi-Axis Subscriber Filtering

### Core hierarchy (`agentzero-core/src/event_bus.rs`)

1. Add `EventFilter` struct:
   ```rust
   pub struct EventFilter {
       pub source: Option<String>,
       pub topic_prefix: Option<String>,
   }
   ```
   with `matches(&self, event: &Event) -> bool`

2. Add `recv_with_filter()` default method on `EventSubscriber` trait — loops `recv()` applying `EventFilter::matches()`. Keeps `recv_filtered()` for backward compat.

3. Update `TypedSubscriber` to optionally accept an `EventFilter` for source filtering.

### Storage (`agentzero-storage/src/event_bus.rs`)

4. Add `replay_with_filter()` on `SqliteEventBus` that uses SQL `WHERE source = ? AND topic LIKE ?%` for efficient catch-up.

5. Add `idx_events_source` index to the events table.

### Orchestrator (`agentzero-orchestrator/src/event_bus.rs`)

6. Update `EventReceiver::recv()` to optionally filter by source in addition to channel.

### Regression bus (`agentzero-core/src/regression_bus.rs`)

7. Update `spawn_regression_monitor()` to use `recv_with_filter()`.

## Phase B: Publish Result Feedback

### Core hierarchy

1. Add `PublishResult` struct:
   ```rust
   #[derive(Debug, Clone, Copy)]
   pub struct PublishResult {
       pub delivered: usize,
   }
   ```

2. Change `EventBus::publish()` → `Result<PublishResult>`.

3. Update `InMemoryBus`: `tx.send()` returns receiver count on `Ok`, 0 on `Err`.

4. Update `FileBackedBus`: propagate inner result.

5. Update `TypedTopic::publish()` and `publish_with_boundary()` to return `PublishResult`.

### Storage

6. Update `SqliteEventBus::publish()` to return `PublishResult` from broadcast count.

### Orchestrator

7. Update orchestrator `EventBus::publish()` → `Result<PublishResult>`.
8. Update `InMemoryEventBus` and `FileBackedEventBus`.

### Gossip

9. Update `GossipEventBus::publish()` — return local delivery count only.

### Call sites

10. All callers currently do `.publish(...).await?` — the `?` still works, callers just ignore `PublishResult` unless they opt in. Update call sites to compile.

## Phase C: Arc-Wrapped Event Payloads

### Core

1. Change `Event.payload: String` → `Arc<str>`.
2. Update `Event::new()` to convert via `Arc::from(s.into())`.
3. Serde: `Arc<str>` has built-in Serialize/Deserialize support.

### Storage

4. Update `SqliteEventBus` insert/read — `Arc<str>` derefs to `&str` for SQL params, construct via `Arc::from(s)` on read.

### Orchestrator

5. Update `BusEvent.payload: String` → `Arc<str>`.
6. Update `PersistedEvent.payload` similarly.

### Gossip

7. Wire protocol already serializes to JSON — `Arc<str>` serializes identically to `String`.

## Files to modify

- `crates/agentzero-core/src/event_bus.rs` — EventFilter, PublishResult, Arc payload, trait changes
- `crates/agentzero-core/src/lib.rs` — re-export new types
- `crates/agentzero-core/src/regression_bus.rs` — use recv_with_filter
- `crates/agentzero-storage/src/event_bus.rs` — SqliteEventBus updates + source index
- `crates/agentzero-orchestrator/src/event_bus.rs` — orchestrator bus updates
- `crates/agentzero-orchestrator/src/gossip.rs` — GossipEventBus publish return
- All publish call sites (agents_ipc, trigger_fire, proposal_vote, proposal_create, job_store, gateway tests)

## Verification

1. `cargo clippy --all-targets` — 0 warnings
2. `cargo test -p agentzero-core` — all existing + new tests pass
3. `cargo test -p agentzero-storage` — all existing + new tests pass
4. `cargo test -p agentzero-orchestrator` — all existing + new tests pass
5. Full workspace build succeeds
