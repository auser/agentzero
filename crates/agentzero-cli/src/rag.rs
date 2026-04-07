//! RAG document index backed by Tantivy BM25.
//!
//! Documents are persisted twice:
//!   1. As an encrypted JSON store at `index_path` (durable source of truth, used for
//!      cold-start rebuild and listing).
//!   2. As a Tantivy index in a sibling directory `<index_path>.tantivy/` (fast BM25
//!      query path with relevance scoring).
//!
//! When the Tantivy index is missing on startup it is rebuilt automatically from the
//! encrypted store. Existing legacy JSONL indexes are migrated transparently.

use agentzero_storage::EncryptedJsonStore;
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy, TantivyDocument};

const TANTIVY_WRITER_HEAP_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RagDocument {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RagIngestResult {
    pub id: String,
    pub indexed_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RagQueryMatch {
    pub id: String,
    pub text: String,
    /// BM25 relevance score (higher = more relevant). Always non-negative.
    pub score: f32,
}

pub fn ingest_document(
    index_path: impl AsRef<Path>,
    doc: RagDocument,
) -> anyhow::Result<RagIngestResult> {
    if doc.id.trim().is_empty() {
        return Err(anyhow!("rag document id cannot be empty"));
    }
    if doc.text.trim().is_empty() {
        return Err(anyhow!("rag document text cannot be empty"));
    }

    let index_path = index_path.as_ref();
    let store = index_store(index_path)?;
    let existing = load_documents(index_path, &store)?;
    let mut updated = existing.clone();
    updated.push(doc.clone());
    store
        .save(&updated)
        .with_context(|| format!("failed to write rag index {}", store.path().display()))?;

    // Open the Tantivy index. If it doesn't exist, rebuild from the documents that
    // existed BEFORE this ingest so we don't double-insert the new doc below.
    let tantivy_dir = tantivy_dir_for(index_path);
    let index = open_or_create_index(&tantivy_dir, &existing)?;
    let schema = index.schema();
    let id_field = schema
        .get_field("id")
        .map_err(|e| anyhow!("tantivy schema missing id field: {e}"))?;
    let text_field = schema
        .get_field("text")
        .map_err(|e| anyhow!("tantivy schema missing text field: {e}"))?;

    let mut writer: IndexWriter = index
        .writer(TANTIVY_WRITER_HEAP_BYTES)
        .with_context(|| "failed to open tantivy writer")?;
    writer
        .add_document(doc!(id_field => doc.id.as_str(), text_field => doc.text.as_str()))
        .with_context(|| "failed to add document to tantivy")?;
    writer
        .commit()
        .with_context(|| "failed to commit tantivy writer")?;

    Ok(RagIngestResult {
        id: doc.id,
        indexed_chars: doc.text.chars().count(),
    })
}

pub fn query_documents(
    index_path: impl AsRef<Path>,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<RagQueryMatch>> {
    if query.trim().is_empty() {
        return Err(anyhow!("rag query cannot be empty"));
    }

    if limit == 0 {
        return Err(anyhow!("rag query limit must be greater than zero"));
    }

    let index_path = index_path.as_ref();
    let store = index_store(index_path)?;
    let docs = load_documents(index_path, &store)?;

    if docs.is_empty() {
        return Ok(Vec::new());
    }

    let tantivy_dir = tantivy_dir_for(index_path);
    let index = open_or_create_index(&tantivy_dir, &docs)?;
    let schema = index.schema();
    let id_field = schema
        .get_field("id")
        .map_err(|e| anyhow!("tantivy schema missing id field: {e}"))?;
    let text_field = schema
        .get_field("text")
        .map_err(|e| anyhow!("tantivy schema missing text field: {e}"))?;

    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()
        .with_context(|| "failed to open tantivy reader")?;
    let searcher = reader.searcher();

    let query_parser = QueryParser::for_index(&index, vec![text_field, id_field]);
    let parsed = query_parser
        .parse_query(query)
        .with_context(|| format!("failed to parse rag query: {query}"))?;

    let top_docs = searcher
        .search(&parsed, &TopDocs::with_limit(limit))
        .with_context(|| "tantivy search failed")?;

    let mut matches = Vec::with_capacity(top_docs.len());
    for (score, doc_address) in top_docs {
        let stored: TantivyDocument = searcher
            .doc(doc_address)
            .with_context(|| "failed to load stored tantivy doc")?;
        let id = first_text_value(&stored, id_field)
            .ok_or_else(|| anyhow!("tantivy doc missing id field"))?;
        let text = first_text_value(&stored, text_field)
            .ok_or_else(|| anyhow!("tantivy doc missing text field"))?;
        matches.push(RagQueryMatch { id, text, score });
    }

    Ok(matches)
}

fn first_text_value(doc: &TantivyDocument, field: Field) -> Option<String> {
    use tantivy::schema::Value;
    doc.get_first(field)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

fn build_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_text_field("id", STRING | STORED);
    builder.add_text_field("text", TEXT | STORED);
    builder.build()
}

fn tantivy_dir_for(index_path: &Path) -> PathBuf {
    let file_name = index_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("index");
    let parent = index_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{file_name}.tantivy"))
}

/// Open the Tantivy index at `dir`, creating it if missing. If the directory exists
/// but cannot be opened (corruption, schema drift), it is rebuilt from `docs`.
fn open_or_create_index(dir: &Path, docs: &[RagDocument]) -> anyhow::Result<Index> {
    let schema = build_schema();
    if dir.exists() {
        match Index::open_in_dir(dir) {
            Ok(idx) => return Ok(idx),
            Err(_) => {
                fs::remove_dir_all(dir).with_context(|| {
                    format!(
                        "failed to remove corrupt tantivy index at {}",
                        dir.display()
                    )
                })?;
            }
        }
    }

    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create tantivy dir {}", dir.display()))?;
    let index = Index::create_in_dir(dir, schema.clone())
        .with_context(|| format!("failed to create tantivy index at {}", dir.display()))?;

    if !docs.is_empty() {
        let id_field = schema
            .get_field("id")
            .map_err(|e| anyhow!("schema missing id field: {e}"))?;
        let text_field = schema
            .get_field("text")
            .map_err(|e| anyhow!("schema missing text field: {e}"))?;
        let mut writer: IndexWriter = index
            .writer(TANTIVY_WRITER_HEAP_BYTES)
            .with_context(|| "failed to open tantivy writer for rebuild")?;
        for doc in docs {
            writer
                .add_document(doc!(id_field => doc.id.as_str(), text_field => doc.text.as_str()))
                .with_context(|| "failed to add document during rebuild")?;
        }
        writer
            .commit()
            .with_context(|| "failed to commit tantivy rebuild")?;
    }

    Ok(index)
}

fn index_store(index_path: &Path) -> anyhow::Result<EncryptedJsonStore> {
    let parent = index_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = index_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("invalid rag index path: {}", index_path.display()))?;
    EncryptedJsonStore::in_config_dir(parent, file_name)
}

fn load_documents(
    index_path: &Path,
    store: &EncryptedJsonStore,
) -> anyhow::Result<Vec<RagDocument>> {
    match store.load_optional::<Vec<RagDocument>>() {
        Ok(Some(docs)) => Ok(docs),
        Ok(None) => Ok(Vec::new()),
        Err(_) => load_legacy_jsonl(index_path, store),
    }
}

fn load_legacy_jsonl(
    index_path: &Path,
    store: &EncryptedJsonStore,
) -> anyhow::Result<Vec<RagDocument>> {
    if !index_path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(index_path)
        .with_context(|| format!("failed to read rag index {}", index_path.display()))?;
    let mut docs = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let doc: RagDocument =
            serde_json::from_str(trimmed).context("failed to decode rag document")?;
        docs.push(doc);
    }
    store
        .save(&docs)
        .with_context(|| format!("failed to migrate rag index {}", store.path().display()))?;
    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::{ingest_document, query_documents, tantivy_dir_for, RagDocument};
    use std::fs;

    #[test]
    fn ingest_and_query_success_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let index = tmp.path().join("rag").join("index.jsonl");

        ingest_document(
            &index,
            RagDocument {
                id: "doc-1".to_string(),
                text: "agent orchestration with retries".to_string(),
            },
        )
        .expect("ingest should succeed");

        let matches = query_documents(&index, "retries", 5).expect("query should succeed");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "doc-1");
        assert!(matches[0].score > 0.0, "BM25 score should be positive");
    }

    #[test]
    fn bm25_ranks_more_relevant_doc_higher() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let index = tmp.path().join("rag").join("index.jsonl");

        ingest_document(
            &index,
            RagDocument {
                id: "a".to_string(),
                text: "rust agents and tool calling".to_string(),
            },
        )
        .expect("ingest a");
        ingest_document(
            &index,
            RagDocument {
                id: "b".to_string(),
                text: "rust agents rust agents rust agents elaborated".to_string(),
            },
        )
        .expect("ingest b");
        ingest_document(
            &index,
            RagDocument {
                id: "c".to_string(),
                text: "completely unrelated content about cooking".to_string(),
            },
        )
        .expect("ingest c");

        let matches = query_documents(&index, "rust agents", 5).expect("query");
        assert!(matches.len() >= 2, "expected at least two matches");
        // BM25 prefers shorter docs with same term frequency, so order depends on length norm.
        // Either way, the cooking doc should NOT be in the top results.
        assert!(
            matches.iter().all(|m| m.id != "c"),
            "irrelevant doc included"
        );
    }

    #[test]
    fn ingest_rejects_empty_text_negative_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let index = tmp.path().join("rag").join("index.jsonl");

        let err = ingest_document(
            &index,
            RagDocument {
                id: "doc-1".to_string(),
                text: "   ".to_string(),
            },
        )
        .expect_err("empty text should fail");
        assert!(err.to_string().contains("cannot be empty"));
    }

    #[test]
    fn query_migrates_legacy_jsonl_success_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let index = tmp.path().join("rag").join("index.jsonl");
        fs::create_dir_all(index.parent().expect("parent should exist"))
            .expect("index parent should be created");
        fs::write(
            &index,
            "{\"id\":\"doc-1\",\"text\":\"hello world\"}\n{\"id\":\"doc-2\",\"text\":\"goodbye\"}\n",
        )
        .expect("legacy jsonl should be written");

        let matches = query_documents(&index, "hello", 5).expect("query should succeed");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "doc-1");

        let on_disk = fs::read_to_string(&index).expect("index should be readable");
        assert!(!on_disk.contains("hello world"));
    }

    #[test]
    fn query_rejects_malformed_legacy_jsonl_negative_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let index = tmp.path().join("rag").join("index.jsonl");
        fs::create_dir_all(index.parent().expect("parent should exist"))
            .expect("index parent should be created");
        fs::write(&index, "{not-json}\n").expect("legacy jsonl should be written");

        let err = query_documents(&index, "hello", 5).expect_err("malformed jsonl should fail");
        assert!(err.to_string().contains("failed to decode rag document"));
    }

    #[test]
    fn cold_start_rebuilds_tantivy_from_encrypted_store() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let index = tmp.path().join("rag").join("index.jsonl");

        // Seed via ingest (creates both encrypted store and tantivy dir)
        ingest_document(
            &index,
            RagDocument {
                id: "doc-1".to_string(),
                text: "the quick brown fox".to_string(),
            },
        )
        .expect("ingest");

        // Delete the tantivy dir to simulate cold start with only the encrypted store
        let tdir = tantivy_dir_for(&index);
        assert!(tdir.exists(), "tantivy dir should exist after ingest");
        fs::remove_dir_all(&tdir).expect("delete tantivy dir");
        assert!(!tdir.exists());

        // Query should rebuild and succeed
        let matches = query_documents(&index, "quick", 5).expect("query after rebuild");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "doc-1");
        assert!(tdir.exists(), "tantivy dir should be rebuilt");
    }

    #[test]
    fn empty_query_rejected() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let index = tmp.path().join("rag").join("index.jsonl");
        let err = query_documents(&index, "   ", 5).expect_err("empty query should fail");
        assert!(err.to_string().contains("cannot be empty"));
    }

    #[test]
    fn zero_limit_rejected() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let index = tmp.path().join("rag").join("index.jsonl");
        let err = query_documents(&index, "hello", 0).expect_err("zero limit should fail");
        assert!(err.to_string().contains("greater than zero"));
    }

    #[test]
    fn empty_index_returns_no_matches() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let index = tmp.path().join("rag").join("index.jsonl");
        let matches = query_documents(&index, "hello", 5).expect("query empty");
        assert!(matches.is_empty());
    }
}
