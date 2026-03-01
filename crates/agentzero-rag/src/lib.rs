use agentzero_storage::EncryptedJsonStore;
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RagQueryMatch {
    pub id: String,
    pub text: String,
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

    let store = index_store(index_path.as_ref())?;
    let mut docs = load_documents(index_path.as_ref(), &store)?;
    docs.push(doc.clone());
    store
        .save(&docs)
        .with_context(|| format!("failed to write rag index {}", store.path().display()))?;

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

    let query_norm = query.to_ascii_lowercase();
    let mut matches = Vec::new();
    for doc in docs {
        if doc.text.to_ascii_lowercase().contains(&query_norm)
            || doc.id.to_ascii_lowercase().contains(&query_norm)
        {
            matches.push(RagQueryMatch {
                id: doc.id,
                text: doc.text,
            });
            if matches.len() >= limit {
                break;
            }
        }
    }

    Ok(matches)
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
    use super::{ingest_document, query_documents, RagDocument};
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
}
