use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::embed::EmbeddedChunk;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialize(String),
    #[error("deserialization error: {0}")]
    Deserialize(String),
    #[error("index not found at {0}")]
    NotFound(String),
}

/// Metadata about an index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    pub model_name: String,
    pub chunk_count: usize,
    pub file_count: usize,
    pub created_at: String,
}

/// On-disk format: metadata + embedded chunks.
#[derive(Serialize, Deserialize)]
struct IndexData {
    metadata: IndexMetadata,
    chunks: Vec<EmbeddedChunk>,
}

/// In-memory vector store backed by serialized file on disk.
pub struct FileStore {
    chunks: Vec<EmbeddedChunk>,
    metadata: IndexMetadata,
}

impl FileStore {
    /// Create a new empty store.
    pub fn new(model_name: &str) -> Self {
        Self {
            chunks: Vec::new(),
            metadata: IndexMetadata {
                model_name: model_name.to_string(),
                chunk_count: 0,
                file_count: 0,
                created_at: chrono_now(),
            },
        }
    }

    /// Store embedded chunks, replacing any existing data.
    pub fn store(&mut self, chunks: Vec<EmbeddedChunk>, file_count: usize) {
        self.metadata.chunk_count = chunks.len();
        self.metadata.file_count = file_count;
        self.metadata.created_at = chrono_now();
        self.chunks = chunks;
    }

    /// Get all stored chunks.
    pub fn chunks(&self) -> &[EmbeddedChunk] {
        &self.chunks
    }

    /// Get index metadata.
    pub fn metadata(&self) -> &IndexMetadata {
        &self.metadata
    }

    /// Save the index to disk using bincode.
    pub fn save(&self, path: &Path) -> Result<(), StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let data = IndexData {
            metadata: self.metadata.clone(),
            chunks: self.chunks.clone(),
        };

        let bytes = bincode::serde::encode_to_vec(&data, bincode::config::standard())
            .map_err(|e| StoreError::Serialize(e.to_string()))?;

        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Load an index from disk.
    pub fn load(path: &Path) -> Result<Self, StoreError> {
        if !path.exists() {
            return Err(StoreError::NotFound(path.display().to_string()));
        }

        let bytes = std::fs::read(path)?;
        let (data, _): (IndexData, _) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
                .map_err(|e| StoreError::Deserialize(e.to_string()))?;

        Ok(Self {
            chunks: data.chunks,
            metadata: data.metadata,
        })
    }

    /// Save metadata as human-readable JSON alongside the index.
    pub fn save_metadata_json(&self, path: &Path) -> Result<(), StoreError> {
        let json = serde_json::to_string_pretty(&self.metadata)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

fn chrono_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::TextChunk;
    use std::path::PathBuf;

    fn sample_chunks() -> Vec<EmbeddedChunk> {
        vec![EmbeddedChunk {
            chunk: TextChunk {
                source_path: PathBuf::from("test.txt"),
                content: "hello world".into(),
                start_byte: 0,
                end_byte: 11,
                chunk_index: 0,
            },
            embedding: vec![0.1, 0.2, 0.3],
        }]
    }

    #[test]
    fn round_trip_save_load() {
        let dir = std::env::temp_dir().join("agentzero-index-test");
        let idx_path = dir.join("test.idx");

        let mut store = FileStore::new("test-model");
        store.store(sample_chunks(), 1);
        store.save(&idx_path).expect("save should succeed");

        let loaded = FileStore::load(&idx_path).expect("load should succeed");
        assert_eq!(loaded.metadata().chunk_count, 1);
        assert_eq!(loaded.metadata().model_name, "test-model");
        assert_eq!(loaded.chunks().len(), 1);
        assert_eq!(loaded.chunks()[0].chunk.content, "hello world");

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }
}
