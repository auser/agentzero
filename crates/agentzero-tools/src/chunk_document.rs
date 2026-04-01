//! Semantic document chunking for RAG pipelines.
//!
//! Uses `text-splitter` to split documents into semantically coherent chunks
//! suitable for embedding and retrieval. Supports markdown and code files with
//! syntax-aware splitting that respects heading boundaries, paragraph breaks,
//! and language constructs.

use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::Context;
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Component, PathBuf};
use text_splitter::{MarkdownSplitter, TextSplitter};
use tokio::fs;

const DEFAULT_MAX_CHUNK_SIZE: usize = 512;
const DEFAULT_MIN_CHUNK_SIZE: usize = 64;

/// Tool that splits documents into semantically coherent chunks for RAG indexing.
#[tool(
    name = "chunk_document",
    description = "Split a document into semantic chunks for RAG indexing. Supports markdown, code, and plain text. Returns chunks with byte offsets."
)]
pub struct ChunkDocumentTool {
    allowed_root: PathBuf,
}

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct ChunkDocumentInput {
    /// Relative path to the document to chunk
    path: String,
    /// Maximum characters per chunk (default: 512)
    #[serde(default)]
    max_chunk_size: Option<usize>,
}

impl ChunkDocumentTool {
    pub fn new(allowed_root: PathBuf) -> Self {
        Self { allowed_root }
    }

    fn resolve_safe(&self, input_path: &str, workspace_root: &str) -> anyhow::Result<PathBuf> {
        let base = if workspace_root.is_empty() {
            self.allowed_root.clone()
        } else {
            PathBuf::from(workspace_root)
        };

        let path = PathBuf::from(input_path);

        // Block parent-directory traversal
        for component in path.components() {
            if matches!(component, Component::ParentDir) {
                anyhow::bail!("path traversal not allowed: {input_path}");
            }
        }

        let full = base.join(&path);
        let canonical = full
            .canonicalize()
            .with_context(|| format!("file not found: {}", full.display()))?;

        let allowed = self
            .allowed_root
            .canonicalize()
            .unwrap_or_else(|_| self.allowed_root.clone());

        if !canonical.starts_with(&allowed) {
            anyhow::bail!(
                "path {} is outside allowed root {}",
                canonical.display(),
                allowed.display()
            );
        }

        Ok(canonical)
    }
}

/// Detect the document type from file extension for choosing the right splitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocType {
    Markdown,
    PlainText,
}

fn detect_doc_type(path: &std::path::Path) -> DocType {
    match path.extension().and_then(|e| e.to_str()) {
        Some("md" | "mdx" | "markdown") => DocType::Markdown,
        _ => DocType::PlainText,
    }
}

/// Split text into chunks, returning `(chunk_text, byte_offset)` pairs.
///
/// Uses `MarkdownSplitter` for `.md` files (respects headings, paragraphs)
/// and `TextSplitter` for everything else (splits on sentence/paragraph
/// boundaries).
pub fn split_document(text: &str, doc_type: DocType, max_chars: usize) -> Vec<(String, usize)> {
    let chunks: Vec<&str> = match doc_type {
        DocType::Markdown => {
            let splitter = MarkdownSplitter::new(max_chars);
            splitter.chunks(text).collect()
        }
        DocType::PlainText => {
            let splitter = TextSplitter::new(max_chars);
            splitter.chunks(text).collect()
        }
    };

    // Compute byte offsets by finding each chunk in the original text
    let mut results = Vec::with_capacity(chunks.len());
    let mut search_from = 0;
    for chunk in chunks {
        let offset = text[search_from..]
            .find(chunk)
            .map(|i| i + search_from)
            .unwrap_or(search_from);
        results.push((chunk.to_string(), offset));
        search_from = offset + chunk.len();
    }

    results
}

#[async_trait]
impl Tool for ChunkDocumentTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(ChunkDocumentInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: ChunkDocumentInput =
            serde_json::from_str(input).context("invalid chunk_document input")?;

        let safe_path = self.resolve_safe(&req.path, &ctx.workspace_root)?;

        let content = fs::read_to_string(&safe_path)
            .await
            .with_context(|| format!("failed to read {}", safe_path.display()))?;

        let max_chars = req.max_chunk_size.unwrap_or(DEFAULT_MAX_CHUNK_SIZE);
        let max_chars = max_chars.max(DEFAULT_MIN_CHUNK_SIZE);

        let doc_type = detect_doc_type(&safe_path);
        let chunks = split_document(&content, doc_type, max_chars);

        let output = serde_json::json!({
            "path": req.path,
            "doc_type": format!("{doc_type:?}"),
            "total_chars": content.len(),
            "chunk_count": chunks.len(),
            "chunks": chunks.iter().enumerate().map(|(i, (text, offset))| {
                serde_json::json!({
                    "index": i,
                    "offset": offset,
                    "length": text.len(),
                    "text": text,
                })
            }).collect::<Vec<_>>(),
        });

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output)
                .unwrap_or_else(|_| format!("{} chunks generated", chunks.len())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_markdown_extensions() {
        assert_eq!(
            detect_doc_type(std::path::Path::new("README.md")),
            DocType::Markdown
        );
        assert_eq!(
            detect_doc_type(std::path::Path::new("doc.mdx")),
            DocType::Markdown
        );
    }

    #[test]
    fn detect_plain_text_for_code_and_other() {
        assert_eq!(
            detect_doc_type(std::path::Path::new("main.rs")),
            DocType::PlainText
        );
        assert_eq!(
            detect_doc_type(std::path::Path::new("notes.txt")),
            DocType::PlainText
        );
        assert_eq!(
            detect_doc_type(std::path::Path::new("data.csv")),
            DocType::PlainText
        );
    }

    #[test]
    fn split_markdown_respects_headings() {
        let md = "# Chapter 1\n\nThis is the first chapter with enough text to form a chunk.\n\n\
                  # Chapter 2\n\nThis is the second chapter with enough text to form a chunk.";
        let chunks = split_document(md, DocType::Markdown, 80);
        assert!(
            chunks.len() >= 2,
            "should split at heading boundary: got {}",
            chunks.len()
        );
    }

    #[test]
    fn split_markdown_preserves_content() {
        let md = "# Title\n\nSome content here.\n\n## Section\n\nMore content.";
        let chunks = split_document(md, DocType::Markdown, 200);
        let reassembled: String = chunks
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join("");
        assert!(reassembled.contains("Title"));
        assert!(reassembled.contains("Some content"));
        assert!(reassembled.contains("More content"));
    }

    #[test]
    fn split_code_as_plain_text() {
        let code = "fn main() {\n    println!(\"hello\");\n}\n\n\
                    fn helper() {\n    let x = 42;\n}\n";
        let chunks = split_document(code, DocType::PlainText, 50);
        assert!(!chunks.is_empty(), "should produce at least one chunk");
    }

    #[test]
    fn split_plain_text() {
        let text = "First paragraph with some content.\n\n\
                    Second paragraph with more content.\n\n\
                    Third paragraph with even more content.";
        let chunks = split_document(text, DocType::PlainText, 60);
        assert!(
            chunks.len() >= 2,
            "should split paragraphs: got {}",
            chunks.len()
        );
    }

    #[test]
    fn split_document_offsets_are_valid() {
        let text = "Hello world.\n\nThis is a test.\n\nAnother paragraph here.";
        let chunks = split_document(text, DocType::PlainText, 30);
        for (chunk_text, offset) in &chunks {
            assert!(
                text[*offset..].starts_with(chunk_text.trim()),
                "offset {offset} does not point to chunk text"
            );
        }
    }

    #[test]
    fn split_document_min_chunk_size() {
        let text = "Short.";
        let chunks = split_document(text, DocType::PlainText, 10);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0.trim(), "Short.");
    }

    #[tokio::test]
    async fn chunk_document_tool_path_traversal_blocked() {
        let tool = ChunkDocumentTool::new(PathBuf::from("/tmp"));
        let input = r#"{"path": "../etc/passwd"}"#;
        let ctx = ToolContext::new("/tmp".to_string());
        let result = tool.execute(input, &ctx).await;
        assert!(result.is_err(), "should reject path traversal");
    }
}
