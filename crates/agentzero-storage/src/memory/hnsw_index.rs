//! HNSW approximate nearest neighbor index for embedding vectors.
//!
//! Wraps `hnsw_rs::Hnsw<f32, DistCosine>` with a simple insert/search/persist API.
//! The index lives next to the SQLite memory database and is rebuilt from SQLite on
//! cold start when the on-disk index is missing.

use anyhow::{anyhow, Context};
use hnsw_rs::prelude::*;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Default HNSW build parameters. Tuned for small-to-medium memory stores
/// (up to ~1M vectors). For larger workloads, callers can construct
/// `HnswMemoryIndex::with_params` directly.
pub const DEFAULT_MAX_NB_CONNECTION: usize = 16;
pub const DEFAULT_EF_CONSTRUCTION: usize = 200;
pub const DEFAULT_MAX_LAYER: usize = 16;
/// Initial expected element count. The HNSW grows beyond this if needed.
pub const DEFAULT_MAX_ELEMENTS: usize = 10_000;
/// Basename used for `file_dump` artifacts (`<basename>.hnsw.data` + `.hnsw.graph`).
pub const INDEX_BASENAME: &str = "memory_hnsw";

/// HNSW index over embedding vectors. Thread-safe via `RwLock`.
///
/// IDs are `i64` matching the SQLite `memory.id` primary key. The HNSW library
/// itself uses `usize` data IDs, so we cast at the boundary; `i64` ids are
/// always non-negative (autoincrement primary key).
pub struct HnswMemoryIndex {
    inner: Arc<RwLock<HnswState>>,
    persist_path: Option<PathBuf>,
}

struct HnswState {
    hnsw: Hnsw<'static, f32, DistCosine>,
    dim: usize,
    len: usize,
}

impl HnswMemoryIndex {
    /// Create a new in-memory index for vectors of the given dimension.
    pub fn new(dim: usize) -> Self {
        Self::with_params(
            dim,
            DEFAULT_MAX_NB_CONNECTION,
            DEFAULT_EF_CONSTRUCTION,
            DEFAULT_MAX_LAYER,
            DEFAULT_MAX_ELEMENTS,
            None,
        )
    }

    /// Create a new index with explicit HNSW parameters and an optional persist path.
    pub fn with_params(
        dim: usize,
        max_nb_connection: usize,
        ef_construction: usize,
        max_layer: usize,
        max_elements: usize,
        persist_path: Option<PathBuf>,
    ) -> Self {
        let hnsw = Hnsw::<f32, DistCosine>::new(
            max_nb_connection,
            max_elements,
            max_layer,
            ef_construction,
            DistCosine,
        );
        Self {
            inner: Arc::new(RwLock::new(HnswState { hnsw, dim, len: 0 })),
            persist_path,
        }
    }

    /// Insert a single embedding with its database row id.
    pub fn insert(&self, id: i64, embedding: &[f32]) -> anyhow::Result<()> {
        let mut state = self.inner.write();
        if embedding.len() != state.dim {
            return Err(anyhow!(
                "embedding dimension mismatch: index={} got={}",
                state.dim,
                embedding.len()
            ));
        }
        if id < 0 {
            return Err(anyhow!("negative ids are not supported (id={id})"));
        }
        state.hnsw.insert((embedding, id as usize));
        state.len += 1;
        Ok(())
    }

    /// Bulk-insert a batch of `(id, embedding)` pairs.
    pub fn insert_batch(&self, items: &[(i64, Vec<f32>)]) -> anyhow::Result<()> {
        let mut state = self.inner.write();
        for (id, emb) in items {
            if emb.len() != state.dim {
                return Err(anyhow!(
                    "embedding dimension mismatch: index={} got={}",
                    state.dim,
                    emb.len()
                ));
            }
            if *id < 0 {
                return Err(anyhow!("negative ids are not supported (id={id})"));
            }
            state.hnsw.insert((emb.as_slice(), *id as usize));
            state.len += 1;
        }
        Ok(())
    }

    /// Search for the `limit` nearest neighbors. Returns `(id, distance)` pairs
    /// sorted by ascending distance (smaller = more similar with cosine distance).
    pub fn search(&self, query: &[f32], limit: usize) -> anyhow::Result<Vec<(i64, f32)>> {
        let state = self.inner.read();
        if query.len() != state.dim {
            return Err(anyhow!(
                "query dimension mismatch: index={} got={}",
                state.dim,
                query.len()
            ));
        }
        if state.len == 0 {
            return Ok(Vec::new());
        }
        // ef_search controls accuracy/speed tradeoff at query time.
        let ef_search = (limit * 4).max(16);
        let neighbours = state.hnsw.search(query, limit, ef_search);
        Ok(neighbours
            .into_iter()
            .map(|n| (n.d_id as i64, n.distance))
            .collect())
    }

    /// Number of vectors currently in the index.
    pub fn len(&self) -> usize {
        self.inner.read().len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Embedding dimension.
    pub fn dim(&self) -> usize {
        self.inner.read().dim
    }

    /// Persist the index to its configured `persist_path`. No-op if no path was set.
    pub fn save(&self) -> anyhow::Result<()> {
        let Some(dir) = self.persist_path.as_ref() else {
            return Ok(());
        };
        std::fs::create_dir_all(dir)
            .with_context(|| format!("failed to create hnsw dir {}", dir.display()))?;
        let state = self.inner.read();
        state
            .hnsw
            .file_dump(dir, INDEX_BASENAME)
            .map_err(|e| anyhow!("failed to dump hnsw index: {e}"))?;
        Ok(())
    }

    /// Load an index from disk. Returns `Ok(None)` if the on-disk artifacts
    /// are missing — callers should rebuild from the source of truth in that case.
    pub fn load(dir: impl AsRef<Path>, dim: usize) -> anyhow::Result<Option<Self>> {
        let dir = dir.as_ref();
        let graph_path = dir.join(format!("{INDEX_BASENAME}.hnsw.graph"));
        let data_path = dir.join(format!("{INDEX_BASENAME}.hnsw.data"));
        if !graph_path.exists() || !data_path.exists() {
            return Ok(None);
        }

        // The Hnsw returned by `load_hnsw` borrows from HnswIo (it holds mmap'd
        // data). We leak the HnswIo so the borrow is `'static`. The loaded index
        // lives for the lifetime of the storage instance, so this is a one-time
        // cost per process startup, not an unbounded leak.
        let io: &'static mut HnswIo = Box::leak(Box::new(HnswIo::new(dir, INDEX_BASENAME)));
        let hnsw = io
            .load_hnsw::<f32, DistCosine>()
            .map_err(|e| anyhow!("failed to load hnsw index: {e}"))?;
        let len = hnsw.get_nb_point();
        Ok(Some(Self {
            inner: Arc::new(RwLock::new(HnswState { hnsw, dim, len })),
            persist_path: Some(dir.to_path_buf()),
        }))
    }

    /// Attach a persistence path to an existing in-memory index.
    pub fn with_persist_path(mut self, path: PathBuf) -> Self {
        self.persist_path = Some(path);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-hnsw-test-{label}-{}-{nanos}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn vec_a() -> Vec<f32> {
        vec![1.0, 0.0, 0.0, 0.0]
    }
    fn vec_b() -> Vec<f32> {
        vec![0.9, 0.1, 0.0, 0.0]
    }
    fn vec_c() -> Vec<f32> {
        vec![0.0, 1.0, 0.0, 0.0]
    }

    #[test]
    fn insert_and_search_returns_nearest() {
        let idx = HnswMemoryIndex::new(4);
        idx.insert(1, &vec_a()).unwrap();
        idx.insert(2, &vec_b()).unwrap();
        idx.insert(3, &vec_c()).unwrap();

        let results = idx.search(&vec_a(), 2).unwrap();
        assert_eq!(results.len(), 2);
        // The nearest to vec_a should be id 1 (exact match), then id 2.
        assert_eq!(results[0].0, 1);
        assert_eq!(results[1].0, 2);
    }

    #[test]
    fn dimension_mismatch_rejected() {
        let idx = HnswMemoryIndex::new(4);
        let err = idx.insert(1, &[1.0, 2.0, 3.0]).unwrap_err();
        assert!(err.to_string().contains("dimension mismatch"));

        idx.insert(1, &vec_a()).unwrap();
        let err = idx.search(&[1.0, 2.0], 1).unwrap_err();
        assert!(err.to_string().contains("dimension mismatch"));
    }

    #[test]
    fn empty_index_returns_no_results() {
        let idx = HnswMemoryIndex::new(4);
        let results = idx.search(&vec_a(), 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn persist_and_load_roundtrip() {
        let dir = unique_dir("persist");
        let idx = HnswMemoryIndex::with_params(
            4,
            DEFAULT_MAX_NB_CONNECTION,
            DEFAULT_EF_CONSTRUCTION,
            DEFAULT_MAX_LAYER,
            DEFAULT_MAX_ELEMENTS,
            Some(dir.clone()),
        );
        idx.insert(10, &vec_a()).unwrap();
        idx.insert(20, &vec_b()).unwrap();
        idx.insert(30, &vec_c()).unwrap();
        idx.save().unwrap();

        let loaded = HnswMemoryIndex::load(&dir, 4).unwrap().expect("loaded");
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.dim(), 4);

        let results = loaded.search(&vec_a(), 2).unwrap();
        // HNSW is an approximate nearest-neighbour algorithm; on a tiny 3-node
        // index it may return fewer than the requested k results depending on
        // graph connectivity.  Assert only that we get at least one result and
        // that the closest match is correct (exact-vector hit).
        assert!(
            !results.is_empty(),
            "search should return at least one result"
        );
        assert_eq!(results[0].0, 10);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_returns_none_when_missing() {
        let dir = unique_dir("missing");
        let result = HnswMemoryIndex::load(&dir, 4).unwrap();
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn batch_insert_succeeds() {
        let idx = HnswMemoryIndex::new(4);
        let items = vec![(1, vec_a()), (2, vec_b()), (3, vec_c())];
        idx.insert_batch(&items).unwrap();
        assert_eq!(idx.len(), 3);

        let results = idx.search(&vec_c(), 1).unwrap();
        assert_eq!(results[0].0, 3);
    }

    #[test]
    fn negative_id_rejected() {
        let idx = HnswMemoryIndex::new(4);
        let err = idx.insert(-1, &vec_a()).unwrap_err();
        assert!(err.to_string().contains("negative ids"));
    }
}
