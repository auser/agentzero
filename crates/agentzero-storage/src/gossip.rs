//! TCP mesh gossip layer for multi-instance event broadcasting.
//!
//! `GossipEventBus` wraps any local `EventBus` implementation (typically
//! `SqliteEventBus`) and adds peer-to-peer event propagation over TCP.
//! Each node listens for incoming events, broadcasts locally-published
//! events to all known peers, and deduplicates by event ID so that
//! messages don't loop forever in the mesh.
//!
//! Wire protocol: 4-byte big-endian length prefix followed by JSON bytes.

use agentzero_core::{Event, EventBus, EventSubscriber};
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Configuration for the gossip mesh network.
#[derive(Debug, Clone)]
pub struct GossipConfig {
    /// TCP port to listen on for incoming peer connections.
    pub listen_port: u16,
    /// List of peer addresses in `host:port` format.
    pub peers: Vec<String>,
    /// Maximum number of event IDs to remember for deduplication.
    /// When this limit is reached, the oldest entries are discarded
    /// (approximated by clearing the entire set and starting fresh).
    pub max_seen_ids: usize,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            listen_port: 0,
            peers: Vec::new(),
            max_seen_ids: 10_000,
        }
    }
}

/// GossipEventBus wraps a local EventBus (typically SqliteEventBus) with
/// TCP mesh networking. Each node:
/// 1. Listens on a TCP port for incoming events from peers
/// 2. Broadcasts locally-published events to all known peers
/// 3. Deduplicates received events by ID (bounded set)
/// 4. Periodically pings peers to detect failures
pub struct GossipEventBus {
    local: Arc<dyn EventBus>,
    config: GossipConfig,
    seen_ids: Arc<Mutex<HashSet<String>>>,
}

impl GossipEventBus {
    /// Create a new gossip event bus wrapping the given local bus.
    pub fn new(local: Arc<dyn EventBus>, config: GossipConfig) -> Self {
        Self {
            local,
            config,
            seen_ids: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Start the background listener and peer-ping tasks.
    ///
    /// Returns a `tokio::task::JoinHandle` for the listener task and one
    /// for the ping task. The caller should hold these handles (or abort
    /// them on shutdown).
    pub async fn start(
        &self,
    ) -> anyhow::Result<(tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)> {
        let listener = TcpListener::bind(("0.0.0.0", self.config.listen_port)).await?;
        let local_port = listener.local_addr()?.port();
        agentzero_core::tracing::info!(port = local_port, "gossip event bus listening");

        let local = Arc::clone(&self.local);
        let seen_ids = Arc::clone(&self.seen_ids);
        let max_seen = self.config.max_seen_ids;

        let listen_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        agentzero_core::tracing::debug!(%addr, "gossip: accepted peer connection");
                        let local = Arc::clone(&local);
                        let seen_ids = Arc::clone(&seen_ids);
                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_peer_connection(stream, &local, &seen_ids, max_seen).await
                            {
                                agentzero_core::tracing::debug!(
                                    error = %e,
                                    %addr,
                                    "gossip: peer connection error"
                                );
                            }
                        });
                    }
                    Err(e) => {
                        agentzero_core::tracing::warn!(error = %e, "gossip: accept error");
                    }
                }
            }
        });

        let peers = self.config.peers.clone();
        let ping_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                for peer in &peers {
                    match TcpStream::connect(peer).await {
                        Ok(_) => {
                            agentzero_core::tracing::debug!(peer = %peer, "gossip: peer ping OK");
                        }
                        Err(e) => {
                            agentzero_core::tracing::warn!(
                                peer = %peer,
                                error = %e,
                                "gossip: peer ping failed"
                            );
                        }
                    }
                }
            }
        });

        Ok((listen_handle, ping_handle))
    }

    /// Record an event ID in the dedup set. Returns `true` if the ID was
    /// newly inserted (i.e., not a duplicate).
    async fn mark_seen(&self, event_id: &str) -> bool {
        let mut seen = self.seen_ids.lock().await;
        if seen.len() >= self.config.max_seen_ids {
            // Simple bounded eviction: clear and start fresh.
            // A proper LRU would be more precise but this is simple and
            // sufficient for distributed dedup where we mostly care about
            // recent events.
            seen.clear();
        }
        seen.insert(event_id.to_string())
    }

    /// Broadcast an event to all configured peers (best-effort).
    async fn broadcast_to_peers(&self, event: &Event) {
        let payload = match serde_json::to_vec(event) {
            Ok(p) => p,
            Err(e) => {
                agentzero_core::tracing::warn!(error = %e, "gossip: failed to serialize event");
                return;
            }
        };

        for peer in &self.config.peers {
            let payload = payload.clone();
            let peer = peer.clone();
            tokio::spawn(async move {
                if let Err(e) = send_to_peer(&peer, &payload).await {
                    agentzero_core::tracing::debug!(
                        peer = %peer,
                        error = %e,
                        "gossip: failed to send event to peer"
                    );
                }
            });
        }
    }
}

#[async_trait]
impl EventBus for GossipEventBus {
    async fn publish(&self, event: Event) -> anyhow::Result<()> {
        // Mark as seen so we don't re-process it when a peer echoes it back.
        self.mark_seen(&event.id).await;

        // Publish locally first.
        self.local.publish(event.clone()).await?;

        // Then broadcast to peers (fire-and-forget).
        self.broadcast_to_peers(&event).await;

        Ok(())
    }

    fn subscribe(&self) -> Box<dyn EventSubscriber> {
        // Subscribers get events from the local bus, which includes both
        // locally-published and remotely-received events.
        self.local.subscribe()
    }

    fn subscriber_count(&self) -> usize {
        self.local.subscriber_count()
    }

    async fn replay_since(
        &self,
        topic: Option<&str>,
        since_id: Option<&str>,
    ) -> anyhow::Result<Vec<Event>> {
        self.local.replay_since(topic, since_id).await
    }

    async fn gc_older_than(&self, max_age: std::time::Duration) -> anyhow::Result<usize> {
        self.local.gc_older_than(max_age).await
    }
}

// ---------------------------------------------------------------------------
// Wire protocol helpers
// ---------------------------------------------------------------------------

/// Send a length-prefixed JSON payload to a peer.
async fn send_to_peer(addr: &str, payload: &[u8]) -> anyhow::Result<()> {
    let mut stream = TcpStream::connect(addr).await?;
    write_frame(&mut stream, payload).await?;
    stream.shutdown().await?;
    Ok(())
}

/// Write a length-prefixed frame: 4-byte big-endian length + payload.
async fn write_frame(stream: &mut TcpStream, payload: &[u8]) -> anyhow::Result<()> {
    let len = u32::try_from(payload.len())
        .map_err(|_| anyhow::anyhow!("event payload too large for gossip frame"))?;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(payload).await?;
    stream.flush().await?;
    Ok(())
}

/// Read a length-prefixed frame. Returns `None` on clean EOF.
async fn read_frame(stream: &mut TcpStream) -> anyhow::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match stream.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;

    // Guard against absurdly large frames (16 MiB limit).
    if len > 16 * 1024 * 1024 {
        anyhow::bail!("gossip frame too large: {len} bytes");
    }

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

/// Handle a single incoming peer connection: read frames, dedup, publish locally.
async fn handle_peer_connection(
    mut stream: TcpStream,
    local: &Arc<dyn EventBus>,
    seen_ids: &Mutex<HashSet<String>>,
    max_seen: usize,
) -> anyhow::Result<()> {
    while let Some(frame) = read_frame(&mut stream).await? {
        let event: Event = serde_json::from_slice(&frame)?;

        // Dedup check.
        let is_new = {
            let mut seen = seen_ids.lock().await;
            if seen.len() >= max_seen {
                seen.clear();
            }
            seen.insert(event.id.clone())
        };

        if is_new {
            // Publish to the local bus so local subscribers receive it.
            if let Err(e) = local.publish(event).await {
                agentzero_core::tracing::warn!(error = %e, "gossip: failed to publish received event locally");
            }
        } else {
            agentzero_core::tracing::debug!(event_id = %event.id, "gossip: dedup — skipping already-seen event");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::event_bus::InMemoryBus;

    /// Helper to create a gossip bus with an in-memory local bus.
    fn make_gossip(config: GossipConfig) -> GossipEventBus {
        let local: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));
        GossipEventBus::new(local, config)
    }

    #[tokio::test]
    async fn local_publish_reaches_subscriber() {
        let bus = make_gossip(GossipConfig::default());
        let mut sub = bus.subscribe();

        bus.publish(Event::new("test.topic", "node-1", "hello"))
            .await
            .expect("publish");

        let event = sub.recv().await.expect("recv");
        assert_eq!(event.topic, "test.topic");
        assert_eq!(event.payload, "hello");
        assert_eq!(event.source, "node-1");
    }

    #[tokio::test]
    async fn dedup_prevents_duplicate_processing() {
        let bus = make_gossip(GossipConfig::default());

        // First insertion should return true (newly seen).
        assert!(bus.mark_seen("evt-123").await);
        // Second insertion of the same ID should return false.
        assert!(!bus.mark_seen("evt-123").await);
    }

    #[tokio::test]
    async fn seen_ids_bounded() {
        let config = GossipConfig {
            max_seen_ids: 5,
            ..Default::default()
        };
        let bus = make_gossip(config);

        // Fill up to the limit.
        for i in 0..5 {
            bus.mark_seen(&format!("evt-{i}")).await;
        }

        // The next insertion should trigger a clear, so the set resets.
        assert!(bus.mark_seen("evt-overflow").await);

        // After the clear + insert of "evt-overflow", the set has only 1 entry.
        let seen = bus.seen_ids.lock().await;
        assert_eq!(seen.len(), 1);
        assert!(seen.contains("evt-overflow"));
    }

    #[test]
    fn gossip_config_defaults_are_sensible() {
        let config = GossipConfig::default();
        assert_eq!(config.listen_port, 0); // 0 means OS-assigned
        assert!(config.peers.is_empty());
        assert_eq!(config.max_seen_ids, 10_000);
    }

    #[tokio::test]
    async fn tcp_round_trip_frame() {
        // Spin up a local TCP listener, send a frame, read it back.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");

        let payload = b"hello gossip";

        let send_handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.expect("connect");
            write_frame(&mut stream, payload).await.expect("write");
            stream.shutdown().await.expect("shutdown");
        });

        let (mut stream, _) = listener.accept().await.expect("accept");
        let frame = read_frame(&mut stream).await.expect("read").expect("frame");
        assert_eq!(frame, b"hello gossip");

        send_handle.await.expect("send task");
    }

    #[tokio::test]
    async fn peer_to_peer_event_delivery() {
        // Set up two gossip buses that can talk to each other.
        let local_a: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));
        let local_b: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));

        // Start bus B first so we know its port.
        let bus_b = Arc::new(GossipEventBus::new(
            Arc::clone(&local_b),
            GossipConfig {
                listen_port: 0,
                peers: vec![],
                max_seen_ids: 100,
            },
        ));
        let (listen_b, ping_b) = bus_b.start().await.expect("start B");

        // Find out what port B is listening on — we snoop via the listener task's
        // side effect. Instead, we bind B to port 0 and read from the listener.
        // Since start() logged the port, but we need it programmatically, let's
        // just start a fresh listener to find the port, then configure A.
        // Actually, a simpler approach: start B's listener manually.
        listen_b.abort();
        ping_b.abort();

        // Manual approach: start a listener for B, get the port, configure A.
        let listener_b = TcpListener::bind("127.0.0.1:0").await.expect("bind B");
        let port_b = listener_b.local_addr().expect("addr B").port();

        let bus_a = GossipEventBus::new(
            Arc::clone(&local_a),
            GossipConfig {
                listen_port: 0,
                peers: vec![format!("127.0.0.1:{port_b}")],
                max_seen_ids: 100,
            },
        );

        // Subscribe to B's local bus before any events.
        let mut sub_b = local_b.subscribe();

        // Spawn a handler for B's incoming connection.
        let local_b2 = Arc::clone(&local_b);
        let seen_b = Arc::new(Mutex::new(HashSet::new()));
        let seen_b2 = Arc::clone(&seen_b);
        let accept_handle = tokio::spawn(async move {
            let (stream, _) = listener_b.accept().await.expect("accept on B");
            handle_peer_connection(stream, &local_b2, &seen_b2, 100)
                .await
                .expect("handle peer");
        });

        // Publish on A — should broadcast to B.
        bus_a
            .publish(Event::new("mesh.test", "node-a", "from-a"))
            .await
            .expect("publish on A");

        // Wait for B to receive it.
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), sub_b.recv())
            .await
            .expect("timeout waiting for event on B")
            .expect("recv on B");

        assert_eq!(event.topic, "mesh.test");
        assert_eq!(event.payload, "from-a");
        assert_eq!(event.source, "node-a");

        accept_handle.await.expect("accept handle");
    }
}
