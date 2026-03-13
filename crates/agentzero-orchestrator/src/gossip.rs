//! GossipEventBus — distributed event bus over TCP mesh.
//!
//! Wraps a [`SqliteEventBus`] for local persistence and adds a lightweight TCP
//! gossip layer for multi-instance event propagation. Each node listens on a
//! configurable port and broadcasts new events to known peers via
//! length-prefixed JSON frames. Deduplication via a bounded event ID set
//! prevents infinite re-broadcast.
//!
//! # Wire Protocol
//!
//! Each frame: `[4-byte big-endian length][JSON-encoded Event]`
//!
//! Peers periodically send a ping frame (topic = `"__gossip.ping"`) to detect
//! disconnections. If a peer fails to respond, it is marked offline and
//! reconnection is attempted on the next broadcast cycle.

use agentzero_core::event_bus::Event;
use agentzero_core::{EventBus, EventSubscriber};
use agentzero_storage::SqliteEventBus;
use async_trait::async_trait;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, info, warn};

/// Maximum number of event IDs to remember for deduplication.
const DEDUP_CAPACITY: usize = 10_000;
/// Ping interval for peer health checks.
const PING_INTERVAL: Duration = Duration::from_secs(15);
/// Connection timeout for outbound peers.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Gossip configuration.
#[derive(Debug, Clone)]
pub struct GossipConfig {
    /// Local address to listen on for incoming gossip connections.
    pub listen_addr: SocketAddr,
    /// Addresses of known peers to connect to.
    pub peers: Vec<SocketAddr>,
    /// Path to the SQLite event database (for the underlying SqliteEventBus).
    pub db_path: String,
    /// Broadcast channel capacity.
    pub capacity: usize,
}

/// Distributed event bus: SQLite for local persistence + TCP gossip for
/// multi-instance propagation.
pub struct GossipEventBus {
    local_bus: Arc<SqliteEventBus>,
    /// Seen event IDs for deduplication. Bounded LRU-style (oldest evicted).
    seen: Arc<Mutex<SeenSet>>,
    /// Outbound peer connections.
    peers: Arc<Mutex<Vec<PeerConnection>>>,
    /// Broadcast sender for notifying the gossip layer about new local events.
    local_tx: broadcast::Sender<Event>,
    /// Listen address (stored for test/debug).
    #[allow(dead_code)]
    listen_addr: SocketAddr,
}

struct PeerConnection {
    addr: SocketAddr,
    stream: Option<TcpStream>,
}

/// Bounded set for deduplication — evicts oldest entries when full.
struct SeenSet {
    ids: Vec<String>,
    set: HashSet<String>,
    capacity: usize,
}

impl SeenSet {
    fn new(capacity: usize) -> Self {
        Self {
            ids: Vec::with_capacity(capacity),
            set: HashSet::with_capacity(capacity),
            capacity,
        }
    }

    /// Insert an ID. Returns `true` if it was new (not seen before).
    fn insert(&mut self, id: String) -> bool {
        if self.set.contains(&id) {
            return false;
        }
        if self.ids.len() >= self.capacity {
            // Evict the oldest entry.
            let old = self.ids.remove(0);
            self.set.remove(&old);
        }
        self.set.insert(id.clone());
        self.ids.push(id);
        true
    }

    #[cfg(test)]
    fn contains(&self, id: &str) -> bool {
        self.set.contains(id)
    }
}

impl GossipEventBus {
    /// Create a new gossip event bus. Starts the listener and peer connections.
    pub async fn start(config: GossipConfig) -> anyhow::Result<Arc<Self>> {
        let local_bus = Arc::new(SqliteEventBus::open(&config.db_path, config.capacity)?);
        let seen = Arc::new(Mutex::new(SeenSet::new(DEDUP_CAPACITY)));
        let (local_tx, _) = broadcast::channel(config.capacity);

        let peers: Vec<PeerConnection> = config
            .peers
            .iter()
            .map(|addr| PeerConnection {
                addr: *addr,
                stream: None,
            })
            .collect();

        let bus = Arc::new(Self {
            local_bus,
            seen,
            peers: Arc::new(Mutex::new(peers)),
            local_tx: local_tx.clone(),
            listen_addr: config.listen_addr,
        });

        // Spawn the TCP listener for incoming gossip connections.
        let bus_clone = bus.clone();
        let listen_addr = config.listen_addr;
        tokio::spawn(async move {
            if let Err(e) = bus_clone.run_listener(listen_addr).await {
                warn!(error = %e, "gossip listener exited");
            }
        });

        // Spawn the outbound gossip broadcaster.
        let bus_clone = bus.clone();
        let mut local_rx = local_tx.subscribe();
        tokio::spawn(async move {
            loop {
                match local_rx.recv().await {
                    Ok(event) => {
                        bus_clone.broadcast_to_peers(&event).await;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "gossip broadcaster lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        // Spawn periodic ping.
        let bus_clone = bus.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(PING_INTERVAL).await;
                let ping = Event::new("__gossip.ping", "gossip", "");
                bus_clone.broadcast_to_peers(&ping).await;
            }
        });

        info!(addr = %config.listen_addr, peers = config.peers.len(), "gossip event bus started");
        Ok(bus)
    }

    /// Accept inbound connections and handle gossip frames.
    async fn run_listener(self: &Arc<Self>, addr: SocketAddr) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        info!(addr = %addr, "gossip listener started");
        loop {
            let (stream, peer_addr) = listener.accept().await?;
            debug!(peer = %peer_addr, "accepted gossip connection");
            let bus = self.clone();
            tokio::spawn(async move {
                if let Err(e) = bus.handle_inbound(stream).await {
                    debug!(peer = %peer_addr, error = %e, "gossip inbound handler exited");
                }
            });
        }
    }

    /// Handle an inbound TCP connection — read frames and ingest events.
    async fn handle_inbound(self: &Arc<Self>, mut stream: TcpStream) -> anyhow::Result<()> {
        loop {
            let event = read_frame(&mut stream).await?;
            // Skip ping frames.
            if event.topic == "__gossip.ping" {
                continue;
            }
            self.ingest_remote_event(event).await?;
        }
    }

    /// Ingest an event received from a remote peer. Dedup, persist, and
    /// broadcast to local subscribers (but NOT back to gossip peers).
    async fn ingest_remote_event(&self, event: Event) -> anyhow::Result<()> {
        let is_new = {
            let mut seen = self.seen.lock().await;
            seen.insert(event.id.clone())
        };
        if !is_new {
            return Ok(());
        }
        // Persist to local SQLite and broadcast to local subscribers only.
        self.local_bus.publish(event).await?;
        Ok(())
    }

    /// Send an event to all connected peers.
    async fn broadcast_to_peers(&self, event: &Event) {
        let mut peers = self.peers.lock().await;
        for peer in peers.iter_mut() {
            if let Err(e) = Self::send_to_peer(peer, event).await {
                debug!(peer = %peer.addr, error = %e, "failed to send to peer");
                peer.stream = None; // Mark as disconnected for reconnect.
            }
        }
    }

    /// Send a single event to a peer, reconnecting if necessary.
    async fn send_to_peer(peer: &mut PeerConnection, event: &Event) -> anyhow::Result<()> {
        if peer.stream.is_none() {
            let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(peer.addr))
                .await
                .map_err(|_| anyhow::anyhow!("connection timeout"))??;
            peer.stream = Some(stream);
            debug!(peer = %peer.addr, "reconnected to gossip peer");
        }
        let stream = peer.stream.as_mut().expect("stream just set");
        write_frame(stream, event).await
    }
}

#[async_trait]
impl EventBus for GossipEventBus {
    async fn publish(&self, event: Event) -> anyhow::Result<()> {
        // Mark as seen to prevent re-ingestion from peers.
        {
            let mut seen = self.seen.lock().await;
            seen.insert(event.id.clone());
        }
        // Persist to local SQLite + broadcast to local subscribers.
        self.local_bus.publish(event.clone()).await?;
        // Notify the gossip broadcaster to send to peers.
        let _ = self.local_tx.send(event);
        Ok(())
    }

    fn subscribe(&self) -> Box<dyn EventSubscriber> {
        self.local_bus.subscribe()
    }

    fn subscriber_count(&self) -> usize {
        self.local_bus.subscriber_count()
    }

    async fn replay_since(
        &self,
        topic: Option<&str>,
        since_id: Option<&str>,
    ) -> anyhow::Result<Vec<Event>> {
        self.local_bus.replay_since(topic, since_id).await
    }

    async fn gc_older_than(&self, max_age: Duration) -> anyhow::Result<usize> {
        self.local_bus.gc_older_than(max_age).await
    }
}

// ---------------------------------------------------------------------------
// Wire protocol: length-prefixed JSON frames
// ---------------------------------------------------------------------------

/// Write a single event as a length-prefixed JSON frame.
async fn write_frame(stream: &mut TcpStream, event: &Event) -> anyhow::Result<()> {
    let json = serde_json::to_vec(event)?;
    let len = json.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&json).await?;
    stream.flush().await?;
    Ok(())
}

/// Read a single length-prefixed JSON frame from the stream.
async fn read_frame(stream: &mut TcpStream) -> anyhow::Result<Event> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 16 * 1024 * 1024 {
        anyhow::bail!("gossip frame too large: {len} bytes");
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let event: Event = serde_json::from_slice(&buf)?;
    Ok(event)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path() -> String {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        format!("/tmp/gossip_test_{pid}_{nanos}.db")
    }

    #[tokio::test]
    async fn two_node_gossip_relay() {
        // Start two gossip nodes that peer with each other.
        let db1 = temp_db_path();
        let db2 = temp_db_path();

        let bus1 = GossipEventBus::start(GossipConfig {
            listen_addr: "127.0.0.1:0".parse().expect("addr"),
            peers: vec![],
            db_path: db1.clone(),
            capacity: 64,
        })
        .await
        .expect("bus1 start");

        // Get the actual bound address of bus1's listener.
        // Since we bind to port 0, we need to use a fixed port approach for testing.
        // Instead, use known ports.
        let addr1: SocketAddr = "127.0.0.1:19871".parse().expect("addr");
        let addr2: SocketAddr = "127.0.0.1:19872".parse().expect("addr");

        // Clean up and restart with fixed ports.
        drop(bus1);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let bus1 = GossipEventBus::start(GossipConfig {
            listen_addr: addr1,
            peers: vec![addr2],
            db_path: db1.clone(),
            capacity: 64,
        })
        .await
        .expect("bus1 start");

        let bus2 = GossipEventBus::start(GossipConfig {
            listen_addr: addr2,
            peers: vec![addr1],
            db_path: db2.clone(),
            capacity: 64,
        })
        .await
        .expect("bus2 start");

        // Give the gossip layer time to connect.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Subscribe on bus2 to receive events from bus1.
        let mut sub2 = bus2.subscribe();

        // Publish an event on bus1.
        let event = Event::new("test.relay", "node1", "hello from node 1");
        bus1.publish(event.clone()).await.expect("publish");

        // bus2 should receive it via gossip.
        let received = tokio::time::timeout(Duration::from_secs(2), sub2.recv())
            .await
            .expect("timeout waiting for gossip relay")
            .expect("recv");

        assert_eq!(received.topic, "test.relay");
        assert_eq!(received.payload, "hello from node 1");

        // Cleanup.
        let _ = std::fs::remove_file(&db1);
        let _ = std::fs::remove_file(&db2);
    }

    #[tokio::test]
    async fn dedup_prevents_rebroadcast() {
        let mut seen = SeenSet::new(5);
        assert!(seen.insert("a".to_string()));
        assert!(seen.insert("b".to_string()));
        assert!(!seen.insert("a".to_string())); // duplicate
        assert!(seen.contains("a"));
        assert!(seen.contains("b"));
    }

    #[tokio::test]
    async fn dedup_evicts_oldest() {
        let mut seen = SeenSet::new(3);
        seen.insert("a".to_string());
        seen.insert("b".to_string());
        seen.insert("c".to_string());
        // Capacity full — inserting "d" should evict "a".
        assert!(seen.insert("d".to_string()));
        assert!(!seen.contains("a"));
        assert!(seen.contains("b"));
        assert!(seen.contains("d"));
    }

    #[tokio::test]
    async fn local_publish_persists_and_subscribes() {
        let db = temp_db_path();
        let addr: SocketAddr = "127.0.0.1:19873".parse().expect("addr");

        let bus = GossipEventBus::start(GossipConfig {
            listen_addr: addr,
            peers: vec![],
            db_path: db.clone(),
            capacity: 64,
        })
        .await
        .expect("start");

        let mut sub = bus.subscribe();

        let event = Event::new("test.local", "self", "local payload");
        bus.publish(event).await.expect("publish");

        let received = tokio::time::timeout(Duration::from_secs(1), sub.recv())
            .await
            .expect("timeout")
            .expect("recv");

        assert_eq!(received.topic, "test.local");

        // Verify persistence via replay.
        let replayed = bus.replay_since(None, None).await.expect("replay");
        assert!(!replayed.is_empty());
        assert_eq!(replayed.last().expect("has events").topic, "test.local");

        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn wire_protocol_round_trip() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");

        let event = Event::new("test.wire", "sender", "wire payload");
        let event_clone = event.clone();

        let writer = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.expect("connect");
            write_frame(&mut stream, &event_clone).await.expect("write");
        });

        let (mut stream, _) = listener.accept().await.expect("accept");
        let received = read_frame(&mut stream).await.expect("read");

        assert_eq!(received.topic, "test.wire");
        assert_eq!(received.payload, "wire payload");

        writer.await.expect("writer task");
    }
}
