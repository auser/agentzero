//! Document indexing and semantic query engine for AgentZero.
//!
//! Indexes text files in a directory using embeddings from Ollama's `/api/embed`
//! endpoint, stores the index to disk, and supports cosine-similarity queries.

pub mod chunk;
pub mod embed;
pub mod parse;
pub mod query;
pub mod store;

use std::path::{Path, PathBuf};

use agentzero_tracing::info;
use thiserror::Error;

use chunk::{Chunker, TextSplitterChunker};
use embed::{EmbedError, EmbeddedChunk, Embedder, OllamaEmbedder};
use query::QueryResult;
use store::{FileStore, StoreError};

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("embedding failed: {0}")]
    Embed(#[from] EmbedError),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("no files found to index in {0}")]
    NoFiles(String),
}

/// Configuration for building an index.
pub struct IndexConfig {
    /// Ollama server URL.
    pub ollama_url: String,
    /// Embedding model name.
    pub embed_model: String,
    /// Maximum characters per chunk.
    pub chunk_size: usize,
    /// Number of results to return from queries.
    pub top_k: usize,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            ollama_url: "http://localhost:11434".into(),
            embed_model: "nomic-embed-text".into(),
            chunk_size: 1000,
            top_k: 5,
        }
    }
}

/// Statistics returned after building an index.
#[derive(Debug)]
pub struct BuildStats {
    pub files_indexed: usize,
    pub chunks_created: usize,
    pub model_name: String,
}

/// Orchestrates indexing and querying.
pub struct IndexEngine {
    config: IndexConfig,
    index_dir: PathBuf,
}

impl IndexEngine {
    /// Create an engine for the given project directory.
    ///
    /// The index will be stored at `<project_root>/.agentzero/index/`.
    pub fn new(project_root: &Path, config: IndexConfig) -> Self {
        Self {
            config,
            index_dir: project_root.join(".agentzero").join("index"),
        }
    }

    /// Build the index by walking the directory, chunking, and embedding all text files.
    pub async fn build(&self, root: &Path) -> Result<BuildStats, IndexError> {
        let files = parse::walk_indexable(root);
        if files.is_empty() {
            return Err(IndexError::NoFiles(root.display().to_string()));
        }

        info!(files = files.len(), "discovered indexable files");

        let chunker = TextSplitterChunker::new(self.config.chunk_size);
        let embedder = OllamaEmbedder::new(&self.config.ollama_url, &self.config.embed_model);

        // Chunk all files
        let mut all_chunks = Vec::new();
        for file_path in &files {
            if let Some(content) = parse::read_text(file_path) {
                if content.trim().is_empty() {
                    continue;
                }
                let file_chunks = chunker.chunk(file_path, &content);
                all_chunks.extend(file_chunks);
            }
        }

        info!(
            chunks = all_chunks.len(),
            "chunked files, starting embedding"
        );

        // Embed in batches to avoid overwhelming the server
        let batch_size = 32;
        let mut embedded_chunks = Vec::with_capacity(all_chunks.len());

        for batch_start in (0..all_chunks.len()).step_by(batch_size) {
            let batch_end = (batch_start + batch_size).min(all_chunks.len());
            let batch = &all_chunks[batch_start..batch_end];

            let texts: Vec<&str> = batch.iter().map(|c| c.content.as_str()).collect();
            let embeddings = embedder.embed_texts(&texts).await?;

            for (chunk, embedding) in batch.iter().zip(embeddings) {
                embedded_chunks.push(EmbeddedChunk {
                    chunk: chunk.clone(),
                    embedding,
                });
            }
        }

        // Save to disk
        let file_count = files.len();
        let chunk_count = embedded_chunks.len();

        let mut store = FileStore::new(embedder.model_name());
        store.store(embedded_chunks, file_count);
        store.save(&self.index_path())?;
        store.save_metadata_json(&self.metadata_path())?;

        info!(
            files = file_count,
            chunks = chunk_count,
            "index built successfully"
        );

        Ok(BuildStats {
            files_indexed: file_count,
            chunks_created: chunk_count,
            model_name: self.config.embed_model.clone(),
        })
    }

    /// Query the index with a natural language question.
    pub async fn query(&self, question: &str) -> Result<Vec<QueryResult>, IndexError> {
        let store = FileStore::load(&self.index_path())?;
        let embedder = OllamaEmbedder::new(&self.config.ollama_url, &self.config.embed_model);

        let embeddings = embedder.embed_texts(&[question]).await?;
        let query_embedding = &embeddings[0];

        let results = query::top_k(query_embedding, store.chunks(), self.config.top_k);
        Ok(results)
    }

    /// Get index status (metadata), or `None` if no index exists.
    pub fn status(&self) -> Option<store::IndexMetadata> {
        FileStore::load(&self.index_path())
            .ok()
            .map(|s| s.metadata().clone())
    }

    /// Remove the index from disk.
    pub fn clear(&self) -> Result<(), StoreError> {
        let idx = self.index_path();
        if idx.exists() {
            std::fs::remove_file(&idx).map_err(StoreError::Io)?;
        }
        let meta = self.metadata_path();
        if meta.exists() {
            std::fs::remove_file(&meta).map_err(StoreError::Io)?;
        }
        Ok(())
    }

    fn index_path(&self) -> PathBuf {
        self.index_dir.join("default.idx")
    }

    fn metadata_path(&self) -> PathBuf {
        self.index_dir.join("metadata.json")
    }
}

/// Format query results as text suitable for returning from a tool call.
pub fn format_results(results: &[QueryResult]) -> String {
    if results.is_empty() {
        return "No relevant results found.".into();
    }

    let mut out = String::new();
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!(
            "--- Result {} (score: {:.3}) from {} ---\n{}\n\n",
            i + 1,
            r.score,
            r.chunk.chunk.source_path.display(),
            r.chunk.chunk.content,
        ));
    }
    out
}
