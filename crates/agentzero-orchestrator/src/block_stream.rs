//! Markdown-aware block streaming accumulator.
//!
//! Groups raw streaming tokens into semantic blocks (paragraphs, code fences,
//! list items, headers) so subscribers receive coherent chunks instead of
//! individual token fragments.

/// A semantic block emitted by the accumulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A paragraph of text (terminated by double newline or EOF).
    Paragraph(String),
    /// A fenced code block (``` ... ```), including the fence lines.
    CodeBlock { language: String, content: String },
    /// A markdown header (# ... through ######).
    Header { level: u8, text: String },
    /// A list item (- or * or numbered).
    ListItem(String),
}

/// Stateful accumulator that buffers streaming tokens and emits complete blocks.
#[derive(Debug)]
pub struct BlockAccumulator {
    buffer: String,
    in_code_fence: bool,
    code_language: String,
    code_content: String,
}

impl BlockAccumulator {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            in_code_fence: false,
            code_language: String::new(),
            code_content: String::new(),
        }
    }

    /// Feed a token into the accumulator. Returns any complete blocks.
    pub fn push(&mut self, token: &str) -> Vec<Block> {
        self.buffer.push_str(token);
        self.drain_blocks()
    }

    /// Flush any remaining buffered content as a final block.
    pub fn flush(&mut self) -> Vec<Block> {
        let mut blocks = self.drain_blocks();

        // If we're still inside a code fence, merge remaining buffer and emit.
        if self.in_code_fence {
            self.code_content.push_str(&self.buffer);
            self.buffer.clear();
            let content = self.code_content.trim().to_string();
            blocks.push(Block::CodeBlock {
                language: std::mem::take(&mut self.code_language),
                content,
            });
            self.code_content.clear();
            self.in_code_fence = false;
        }

        // Emit any remaining buffer content.
        let remaining = self.buffer.trim().to_string();
        if !remaining.is_empty() {
            let sub = split_into_blocks(&remaining);
            if sub.is_empty() {
                // Plain text that didn't classify as header/list.
                blocks.push(Block::Paragraph(remaining));
            } else {
                blocks.extend(sub);
            }
            self.buffer.clear();
        }

        blocks
    }

    fn drain_blocks(&mut self) -> Vec<Block> {
        let mut blocks = Vec::new();

        loop {
            if self.in_code_fence {
                // Look for closing fence: either "\n```" in the middle or "```" at
                // the start of the buffer (when previous content was already drained).
                let close_pos = if self.buffer.starts_with("```") {
                    Some(0usize)
                } else {
                    self.buffer.find("\n```").map(|p| p + 1)
                };

                if let Some(fence_start) = close_pos {
                    // Everything before the closing fence is code content.
                    if fence_start > 0 {
                        let prefix = if self.buffer.as_bytes().get(fence_start - 1) == Some(&b'\n')
                        {
                            &self.buffer[..fence_start - 1]
                        } else {
                            &self.buffer[..fence_start]
                        };
                        self.code_content.push_str(prefix);
                    }
                    // Skip past the closing fence line.
                    let after_backticks = fence_start + 3; // "```"
                    let end = self.buffer[after_backticks..]
                        .find('\n')
                        .map(|p| after_backticks + p + 1)
                        .unwrap_or(self.buffer.len());

                    blocks.push(Block::CodeBlock {
                        language: std::mem::take(&mut self.code_language),
                        content: std::mem::take(&mut self.code_content),
                    });
                    self.in_code_fence = false;
                    self.buffer = self.buffer[end..].to_string();
                    continue;
                } else {
                    // Accumulate into code content, keeping only the last
                    // partial line in the buffer (it might contain "```").
                    if let Some(last_nl) = self.buffer.rfind('\n') {
                        self.code_content.push_str(&self.buffer[..=last_nl]);
                        self.buffer = self.buffer[last_nl + 1..].to_string();
                    }
                    break;
                }
            }

            // Check for code fence opening — only if we've seen the full fence line
            // (i.e., there's a newline after the opening ```).
            if let Some(fence_start) = find_code_fence_start(&self.buffer) {
                let after_fence = &self.buffer[fence_start..];
                if let Some(nl_offset) = after_fence.find('\n') {
                    // We have the complete fence line.
                    let fence_line_end = fence_start + nl_offset;

                    // Emit anything before the fence as blocks.
                    let before = self.buffer[..fence_start].trim().to_string();
                    if !before.is_empty() {
                        blocks.extend(split_into_blocks(&before));
                    }

                    let fence_line = &self.buffer[fence_start..fence_line_end];
                    self.code_language = fence_line.trim_start_matches('`').trim().to_string();
                    self.in_code_fence = true;
                    self.code_content.clear();
                    self.buffer = self.buffer[fence_line_end + 1..].to_string();
                    continue;
                }
                // Fence line incomplete (no newline yet) — wait for more tokens.
                break;
            }

            // Check for double newline (paragraph break).
            if let Some(pos) = self.buffer.find("\n\n") {
                let paragraph = self.buffer[..pos].trim().to_string();
                if !paragraph.is_empty() {
                    blocks.extend(split_into_blocks(&paragraph));
                }
                self.buffer = self.buffer[pos + 2..].to_string();
                continue;
            }

            // No complete block yet — wait for more tokens.
            break;
        }

        blocks
    }
}

impl Default for BlockAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the start position of a code fence (``` at start of line).
fn find_code_fence_start(s: &str) -> Option<usize> {
    // Check start of string.
    if s.starts_with("```") {
        return Some(0);
    }
    // Check after newlines.
    for (i, _) in s.match_indices('\n') {
        let after = &s[i + 1..];
        if after.starts_with("```") {
            return Some(i + 1);
        }
    }
    None
}

/// Split a text block into semantic blocks (headers, list items, paragraphs).
fn split_into_blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph_lines = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(block) = classify_line_block(trimmed) {
            // Flush accumulated paragraph lines.
            if !paragraph_lines.is_empty() {
                blocks.push(Block::Paragraph(paragraph_lines.join("\n")));
                paragraph_lines.clear();
            }
            blocks.push(block);
        } else if !trimmed.is_empty() {
            paragraph_lines.push(trimmed.to_string());
        }
    }

    if !paragraph_lines.is_empty() {
        blocks.push(Block::Paragraph(paragraph_lines.join("\n")));
    }

    blocks
}

/// Classify a single line as a header, list item, or None (paragraph content).
fn classify_line_block(line: &str) -> Option<Block> {
    let trimmed = line.trim();

    // Header: # through ######
    if trimmed.starts_with('#') {
        let level = trimmed.chars().take_while(|c| *c == '#').count() as u8;
        if level <= 6 {
            let text = trimmed[level as usize..].trim().to_string();
            if !text.is_empty() {
                return Some(Block::Header { level, text });
            }
        }
    }

    // List item: - or * or 1. etc.
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return Some(Block::ListItem(trimmed[2..].to_string()));
    }
    // Numbered list: "1. ", "2. ", etc.
    if let Some(dot_pos) = trimmed.find(". ") {
        if dot_pos <= 3 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
            return Some(Block::ListItem(trimmed[dot_pos + 2..].to_string()));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraph_on_double_newline() {
        let mut acc = BlockAccumulator::new();
        let blocks = acc.push("Hello world.\n\nSecond paragraph.");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], Block::Paragraph("Hello world.".to_string()));

        let remaining = acc.flush();
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0],
            Block::Paragraph("Second paragraph.".to_string())
        );
    }

    #[test]
    fn code_block_detected() {
        let mut acc = BlockAccumulator::new();
        let blocks = acc.push("```rust\nfn main() {}\n```\n\n");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::CodeBlock { language, content } => {
                assert_eq!(language, "rust");
                assert_eq!(content, "fn main() {}");
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn code_block_incremental() {
        let mut acc = BlockAccumulator::new();
        assert!(acc.push("```py").is_empty());
        assert!(acc.push("thon\nprint").is_empty());
        assert!(acc.push("('hi')\n").is_empty());
        let blocks = acc.push("```\n\n");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::CodeBlock { language, content } => {
                assert_eq!(language, "python");
                assert!(content.contains("print('hi')"));
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn header_detected() {
        let mut acc = BlockAccumulator::new();
        let blocks = acc.push("## Introduction\n\nSome text.");
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0],
            Block::Header {
                level: 2,
                text: "Introduction".to_string()
            }
        );
    }

    #[test]
    fn list_items_detected() {
        let mut acc = BlockAccumulator::new();
        let blocks = acc.push("- Item one\n- Item two\n\n");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], Block::ListItem("Item one".to_string()));
        assert_eq!(blocks[1], Block::ListItem("Item two".to_string()));
    }

    #[test]
    fn numbered_list_detected() {
        let mut acc = BlockAccumulator::new();
        let blocks = acc.push("1. First\n2. Second\n\n");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], Block::ListItem("First".to_string()));
        assert_eq!(blocks[1], Block::ListItem("Second".to_string()));
    }

    #[test]
    fn mixed_content() {
        let mut acc = BlockAccumulator::new();
        let blocks = acc.push("# Title\n\nA paragraph.\n\n```\ncode\n```\n\n- list item\n\n");
        // Should get: Header, Paragraph, CodeBlock, ListItem
        assert_eq!(blocks.len(), 4);
        assert!(matches!(blocks[0], Block::Header { level: 1, .. }));
        assert!(matches!(blocks[1], Block::Paragraph(_)));
        assert!(matches!(blocks[2], Block::CodeBlock { .. }));
        assert!(matches!(blocks[3], Block::ListItem(_)));
    }

    #[test]
    fn flush_emits_unclosed_code_block() {
        let mut acc = BlockAccumulator::new();
        acc.push("```rust\nlet x = 1;");
        let blocks = acc.flush();
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::CodeBlock { language, content } => {
                assert_eq!(language, "rust");
                assert!(content.contains("let x = 1;"));
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn flush_emits_remaining_text() {
        let mut acc = BlockAccumulator::new();
        acc.push("Some trailing text");
        let blocks = acc.flush();
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0],
            Block::Paragraph("Some trailing text".to_string())
        );
    }

    #[test]
    fn empty_input_produces_no_blocks() {
        let mut acc = BlockAccumulator::new();
        assert!(acc.push("").is_empty());
        assert!(acc.flush().is_empty());
    }

    #[test]
    fn paragraph_before_code_block() {
        let mut acc = BlockAccumulator::new();
        let blocks = acc.push("Some text\n```\ncode\n```\n\n");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], Block::Paragraph("Some text".to_string()));
        assert!(matches!(blocks[1], Block::CodeBlock { .. }));
    }
}
