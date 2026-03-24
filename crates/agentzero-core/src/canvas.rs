//! Canvas store for real-time rich content delivery.
//!
//! Agents push content via `CanvasTool` and the gateway exposes REST + WebSocket
//! endpoints for clients to consume it. The store is scoped per canvas ID.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;

/// Maximum content size per frame (256 KiB).
pub const MAX_CONTENT_BYTES: usize = 262_144;

/// Maximum history frames per canvas.
pub const MAX_HISTORY_FRAMES: usize = 100;

/// Allowed content types for canvas rendering.
const ALLOWED_CONTENT_TYPES: &[&str] =
    &["text/html", "image/svg+xml", "text/markdown", "text/plain"];

/// A single frame of canvas content.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CanvasFrame {
    pub content_type: String,
    pub content: String,
    pub timestamp: u64,
}

/// A canvas instance with current content and history.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Canvas {
    pub id: String,
    pub current: Option<CanvasFrame>,
    pub history: VecDeque<CanvasFrame>,
    pub created_at: u64,
}

/// Summary info for listing canvases.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CanvasSummary {
    pub id: String,
    pub content_type: Option<String>,
    pub frame_count: usize,
    pub created_at: u64,
}

/// Thread-safe store for canvas state. Shared between the tool and gateway.
#[derive(Debug, Clone)]
pub struct CanvasStore {
    canvases: Arc<RwLock<HashMap<String, Canvas>>>,
    /// Broadcast sender for real-time updates. Each message contains (canvas_id, frame).
    update_tx: tokio::sync::broadcast::Sender<(String, CanvasFrame)>,
}

impl CanvasStore {
    /// Create a new empty canvas store.
    pub fn new() -> Self {
        let (update_tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            canvases: Arc::new(RwLock::new(HashMap::new())),
            update_tx,
        }
    }

    /// List all active canvases.
    pub async fn list(&self) -> Vec<CanvasSummary> {
        let canvases = self.canvases.read().await;
        let mut summaries: Vec<CanvasSummary> = canvases
            .values()
            .map(|c| CanvasSummary {
                id: c.id.clone(),
                content_type: c.current.as_ref().map(|f| f.content_type.clone()),
                frame_count: c.history.len() + usize::from(c.current.is_some()),
                created_at: c.created_at,
            })
            .collect();
        summaries.sort_by_key(|s| s.created_at);
        summaries
    }

    /// Get current snapshot of a canvas.
    pub async fn snapshot(&self, id: &str) -> Option<Canvas> {
        let canvases = self.canvases.read().await;
        canvases.get(id).cloned()
    }

    /// Get frame history for a canvas.
    pub async fn history(&self, id: &str) -> Option<VecDeque<CanvasFrame>> {
        let canvases = self.canvases.read().await;
        canvases.get(id).map(|c| c.history.clone())
    }

    /// Render content to a canvas.
    ///
    /// Creates the canvas if it does not exist. Pushes the previous current frame
    /// into history (capped at [`MAX_HISTORY_FRAMES`]) and broadcasts the update.
    ///
    /// Returns an error if the content type is not in [`ALLOWED_CONTENT_TYPES`] or
    /// the content exceeds [`MAX_CONTENT_BYTES`].
    pub async fn render(&self, id: &str, content_type: &str, content: &str) -> anyhow::Result<()> {
        if !ALLOWED_CONTENT_TYPES.contains(&content_type) {
            anyhow::bail!(
                "invalid content type '{}'; allowed: {:?}",
                content_type,
                ALLOWED_CONTENT_TYPES
            );
        }

        if content.len() > MAX_CONTENT_BYTES {
            anyhow::bail!(
                "content size {} exceeds maximum {} bytes",
                content.len(),
                MAX_CONTENT_BYTES
            );
        }

        let frame = CanvasFrame {
            content_type: content_type.to_string(),
            content: content.to_string(),
            timestamp: now(),
        };

        let mut canvases = self.canvases.write().await;
        let canvas = canvases.entry(id.to_string()).or_insert_with(|| Canvas {
            id: id.to_string(),
            current: None,
            history: VecDeque::new(),
            created_at: now(),
        });

        // Push old current frame into history.
        if let Some(old) = canvas.current.take() {
            canvas.history.push_back(old);
            while canvas.history.len() > MAX_HISTORY_FRAMES {
                canvas.history.pop_front();
            }
        }

        canvas.current = Some(frame.clone());

        // Broadcast; ignore error (no active receivers is fine).
        let _ = self.update_tx.send((id.to_string(), frame));

        Ok(())
    }

    /// Clear a canvas (remove current content, keep history).
    ///
    /// Returns `true` if the canvas existed, `false` otherwise.
    pub async fn clear(&self, id: &str) -> bool {
        let mut canvases = self.canvases.write().await;
        if let Some(canvas) = canvases.get_mut(id) {
            if let Some(old) = canvas.current.take() {
                canvas.history.push_back(old);
                while canvas.history.len() > MAX_HISTORY_FRAMES {
                    canvas.history.pop_front();
                }
            }
            true
        } else {
            false
        }
    }

    /// Subscribe to canvas updates (for WebSocket streaming).
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<(String, CanvasFrame)> {
        self.update_tx.subscribe()
    }
}

impl Default for CanvasStore {
    fn default() -> Self {
        Self::new()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn render_creates_canvas() {
        let store = CanvasStore::new();
        store
            .render("c1", "text/html", "<h1>hi</h1>")
            .await
            .expect("render should succeed");

        let snap = store.snapshot("c1").await.expect("canvas should exist");
        assert_eq!(snap.id, "c1");
        assert!(snap.current.is_some());
        assert_eq!(
            snap.current.as_ref().expect("current frame exists").content,
            "<h1>hi</h1>"
        );
    }

    #[tokio::test]
    async fn render_updates_existing() {
        let store = CanvasStore::new();
        store
            .render("c1", "text/plain", "first")
            .await
            .expect("first render");
        store
            .render("c1", "text/plain", "second")
            .await
            .expect("second render");

        let snap = store.snapshot("c1").await.expect("canvas should exist");
        assert_eq!(
            snap.current.as_ref().expect("current frame exists").content,
            "second"
        );
        assert_eq!(snap.history.len(), 1);
        assert_eq!(snap.history[0].content, "first");
    }

    #[tokio::test]
    async fn snapshot_returns_current() {
        let store = CanvasStore::new();
        store
            .render("s1", "text/markdown", "# Title")
            .await
            .expect("render");

        let snap = store.snapshot("s1").await.expect("canvas exists");
        let frame = snap.current.expect("has current");
        assert_eq!(frame.content_type, "text/markdown");
        assert_eq!(frame.content, "# Title");
    }

    #[tokio::test]
    async fn list_returns_summaries() {
        let store = CanvasStore::new();
        store
            .render("a", "text/plain", "hello")
            .await
            .expect("render a");
        store
            .render("b", "text/html", "<p>world</p>")
            .await
            .expect("render b");

        let list = store.list().await;
        assert_eq!(list.len(), 2);

        let ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"b"));
    }

    #[tokio::test]
    async fn history_truncates_at_max() {
        let store = CanvasStore::new();
        let total = MAX_HISTORY_FRAMES + 5;
        for i in 0..=total {
            store
                .render("h", "text/plain", &format!("frame-{i}"))
                .await
                .expect("render");
        }

        let hist = store.history("h").await.expect("canvas exists");
        assert_eq!(hist.len(), MAX_HISTORY_FRAMES);
    }

    #[tokio::test]
    async fn clear_removes_current() {
        let store = CanvasStore::new();
        store
            .render("cl", "text/plain", "data")
            .await
            .expect("render");

        let cleared = store.clear("cl").await;
        assert!(cleared);

        let snap = store.snapshot("cl").await.expect("canvas still exists");
        assert!(snap.current.is_none());
        assert_eq!(snap.history.len(), 1);
    }

    #[tokio::test]
    async fn clear_nonexistent_returns_false() {
        let store = CanvasStore::new();
        assert!(!store.clear("nope").await);
    }

    #[tokio::test]
    async fn invalid_content_type_rejected() {
        let store = CanvasStore::new();
        let err = store
            .render("bad", "application/octet-stream", "bytes")
            .await;
        assert!(err.is_err());
        let msg = format!("{}", err.expect_err("should be error"));
        assert!(msg.contains("invalid content type"));
    }

    #[tokio::test]
    async fn oversized_content_rejected() {
        let store = CanvasStore::new();
        let big = "x".repeat(MAX_CONTENT_BYTES + 1);
        let err = store.render("big", "text/plain", &big).await;
        assert!(err.is_err());
        let msg = format!("{}", err.expect_err("should be error"));
        assert!(msg.contains("exceeds maximum"));
    }

    #[tokio::test]
    async fn subscribe_receives_updates() {
        let store = CanvasStore::new();
        let mut rx = store.subscribe();

        store
            .render("sub", "text/plain", "hello")
            .await
            .expect("render");

        let (id, frame) = rx.recv().await.expect("should receive update");
        assert_eq!(id, "sub");
        assert_eq!(frame.content, "hello");
    }
}
