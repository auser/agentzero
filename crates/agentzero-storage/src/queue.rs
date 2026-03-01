use agentzero_crypto::{decrypt_json, encrypt_json, StorageKey};
use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A queued item with metadata for ordering and deduplication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedItem<T> {
    pub id: String,
    pub enqueued_at_epoch_ms: u64,
    pub payload: T,
}

/// Encrypted file-backed persistent queue.
///
/// Each item is stored as a separate encrypted file in a dedicated directory.
/// File naming uses `{epoch_ms}_{id}.enc` for natural sort ordering.
/// Atomic writes via temp file + rename prevent corruption from partial writes.
#[derive(Debug, Clone)]
pub struct EncryptedQueue {
    dir: PathBuf,
    key: StorageKey,
}

impl EncryptedQueue {
    /// Create or open a queue backed by the given directory.
    pub fn new(dir: PathBuf, key: StorageKey) -> anyhow::Result<Self> {
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create queue directory {}", dir.display()))?;
        Ok(Self { dir, key })
    }

    /// Enqueue an item. Writes an encrypted file atomically.
    pub fn enqueue<T: Serialize>(&self, id: &str, payload: &T) -> anyhow::Result<()> {
        let now_ms = now_epoch_ms();
        let item = QueuedItem {
            id: id.to_string(),
            enqueued_at_epoch_ms: now_ms,
            payload,
        };

        let serialized = serde_json::to_vec(&item).context("failed to serialize queue item")?;
        let encrypted = encrypt_json(self.key.as_bytes(), &serialized)?;

        let file_name = format!("{now_ms}_{}.enc", sanitize_id(id));
        let target = self.dir.join(&file_name);
        let temp = self.dir.join(format!(".{file_name}.tmp"));

        fs::write(&temp, &encrypted)
            .with_context(|| format!("failed to write temp queue file {}", temp.display()))?;
        enforce_private_permissions(&temp)?;
        fs::rename(&temp, &target).with_context(|| {
            format!(
                "failed to atomically move {} -> {}",
                temp.display(),
                target.display()
            )
        })?;

        Ok(())
    }

    /// Dequeue (remove) an item by id. Deletes the backing file.
    pub fn dequeue(&self, id: &str) -> anyhow::Result<()> {
        let sanitized = sanitize_id(id);
        let entries =
            fs::read_dir(&self.dir).context("failed to read queue directory for dequeue")?;

        for entry in entries {
            let entry = entry.context("failed to read queue directory entry")?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".enc") && name.contains(&sanitized) {
                fs::remove_file(entry.path()).with_context(|| {
                    format!("failed to remove queue file {}", entry.path().display())
                })?;
                return Ok(());
            }
        }

        Ok(())
    }

    /// Load all pending items, sorted by enqueued_at_epoch_ms ascending.
    /// Used on startup for crash recovery replay.
    pub fn drain_all<T: DeserializeOwned>(&self) -> anyhow::Result<Vec<QueuedItem<T>>> {
        let mut files: Vec<PathBuf> = Vec::new();

        let entries =
            fs::read_dir(&self.dir).context("failed to read queue directory for drain")?;
        for entry in entries {
            let entry = entry.context("failed to read queue directory entry")?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("enc") {
                files.push(path);
            }
        }

        // Sort by filename (epoch_ms prefix gives natural ordering)
        files.sort();

        let mut items = Vec::with_capacity(files.len());
        for path in &files {
            let raw =
                fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
            let decrypted = decrypt_json(self.key.as_bytes(), &raw)
                .with_context(|| format!("failed to decrypt {}", path.display()))?;
            let item: QueuedItem<T> = serde_json::from_slice(&decrypted)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            items.push(item);

            // Remove after draining
            let _ = fs::remove_file(path);
        }

        Ok(items)
    }

    /// Count of pending items (file count in directory).
    pub fn len(&self) -> anyhow::Result<usize> {
        let entries = fs::read_dir(&self.dir).context("failed to read queue directory")?;
        let count = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext == "enc")
                    .unwrap_or(false)
            })
            .count();
        Ok(count)
    }

    pub fn is_empty(&self) -> anyhow::Result<bool> {
        self.len().map(|n| n == 0)
    }
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_millis() as u64
}

/// Sanitize an ID for use in filenames (replace non-alphanumeric with underscore).
fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn enforce_private_permissions(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to chmod {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_queue() -> EncryptedQueue {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agentzero-queue-{now}-{seq}"));
        let key = StorageKey::from_config_dir(&dir).expect("key should load");
        EncryptedQueue::new(dir, key).expect("queue should construct")
    }

    #[test]
    fn enqueue_and_drain_round_trip_success_path() {
        let queue = test_queue();
        queue
            .enqueue("msg-1", &"hello")
            .expect("enqueue should succeed");
        queue
            .enqueue("msg-2", &"world")
            .expect("enqueue should succeed");

        let items: Vec<QueuedItem<String>> = queue.drain_all().expect("drain should succeed");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].payload, "hello");
        assert_eq!(items[1].payload, "world");

        // After drain, queue should be empty
        assert!(queue.is_empty().unwrap());

        let _ = fs::remove_dir_all(&queue.dir);
    }

    #[test]
    fn dequeue_removes_item_success_path() {
        let queue = test_queue();
        queue
            .enqueue("msg-1", &"keep")
            .expect("enqueue should succeed");
        queue
            .enqueue("msg-2", &"remove")
            .expect("enqueue should succeed");

        queue.dequeue("msg-2").expect("dequeue should succeed");
        assert_eq!(queue.len().unwrap(), 1);

        let items: Vec<QueuedItem<String>> = queue.drain_all().expect("drain should succeed");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].payload, "keep");

        let _ = fs::remove_dir_all(&queue.dir);
    }

    #[test]
    fn drain_returns_items_in_order_success_path() {
        let queue = test_queue();
        queue.enqueue("a", &1_u32).expect("enqueue should succeed");
        std::thread::sleep(std::time::Duration::from_millis(5));
        queue.enqueue("b", &2_u32).expect("enqueue should succeed");
        std::thread::sleep(std::time::Duration::from_millis(5));
        queue.enqueue("c", &3_u32).expect("enqueue should succeed");

        let items: Vec<QueuedItem<u32>> = queue.drain_all().expect("drain should succeed");
        assert_eq!(items.len(), 3);
        assert!(items[0].enqueued_at_epoch_ms <= items[1].enqueued_at_epoch_ms);
        assert!(items[1].enqueued_at_epoch_ms <= items[2].enqueued_at_epoch_ms);

        let _ = fs::remove_dir_all(&queue.dir);
    }

    #[test]
    fn empty_queue_drain_returns_empty_vec() {
        let queue = test_queue();
        let items: Vec<QueuedItem<String>> = queue.drain_all().expect("drain should succeed");
        assert!(items.is_empty());
        assert!(queue.is_empty().unwrap());

        let _ = fs::remove_dir_all(&queue.dir);
    }
}
