use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A chunk of text extracted from a source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextChunk {
    pub source_path: PathBuf,
    pub content: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub chunk_index: usize,
}

/// Splits text content into chunks suitable for embedding.
pub trait Chunker: Send + Sync {
    fn chunk(&self, path: &Path, content: &str) -> Vec<TextChunk>;
}

/// Chunker backed by the `text-splitter` crate.
///
/// Splits text at semantic boundaries (sentences, paragraphs) up to the
/// configured maximum character count.
pub struct TextSplitterChunker {
    max_characters: usize,
}

impl TextSplitterChunker {
    pub fn new(max_characters: usize) -> Self {
        Self { max_characters }
    }
}

impl Default for TextSplitterChunker {
    fn default() -> Self {
        Self {
            max_characters: 1000,
        }
    }
}

impl Chunker for TextSplitterChunker {
    fn chunk(&self, path: &Path, content: &str) -> Vec<TextChunk> {
        use text_splitter::TextSplitter;

        let splitter = TextSplitter::new(self.max_characters);
        let mut chunks = Vec::new();
        let mut byte_offset = 0;

        for (idx, chunk_text) in splitter.chunks(content).enumerate() {
            // Find this chunk's position in the original content
            let start = content[byte_offset..]
                .find(chunk_text)
                .map(|pos| byte_offset + pos)
                .unwrap_or(byte_offset);
            let end = start + chunk_text.len();

            chunks.push(TextChunk {
                source_path: path.to_path_buf(),
                content: chunk_text.to_string(),
                start_byte: start,
                end_byte: end,
                chunk_index: idx,
            });

            byte_offset = end;
        }

        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_short_text_into_single_chunk() {
        let chunker = TextSplitterChunker::new(1000);
        let chunks = chunker.chunk(Path::new("test.txt"), "Hello world");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Hello world");
        assert_eq!(chunks[0].chunk_index, 0);
    }

    #[test]
    fn chunks_long_text_into_multiple() {
        let chunker = TextSplitterChunker::new(50);
        let text = "This is the first sentence. This is the second sentence. This is the third sentence. This is the fourth sentence.";
        let chunks = chunker.chunk(Path::new("test.txt"), text);
        assert!(chunks.len() > 1);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i);
            assert!(!chunk.content.is_empty());
        }
    }
}
