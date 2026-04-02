//! Advanced 4-phase context compression for reducing LLM token costs.
//!
//! ## Phases
//! 1. **Tool result pruning** — truncate long tool outputs (pure fn, no LLM)
//! 2. **Boundary protection** — preserve first N + last M messages; never split tool pairs
//! 3. **Middle turn summarization** — LLM-summarize the expendable middle section
//! 4. **Iterative updates** — merge subsequent compressions into existing summary
//!
//! Phases 1-2 are zero-allocation transforms. Phase 3 requires one async LLM call.
//! Phase 4 is stateful within a session via `CompressionState`.

use crate::{ConversationMessage, Provider};
use tracing::warn;

/// Configuration for the compression pipeline.
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Maximum characters for a single tool result before truncation (Phase 1).
    /// Set to 0 to disable tool result pruning.
    pub max_tool_result_chars: usize,
    /// Number of messages to protect at the start (Phase 2).
    pub protect_head: usize,
    /// Number of messages to protect at the tail (Phase 2).
    pub protect_tail: usize,
    /// Maximum characters for the generated middle summary (Phase 3).
    pub max_summary_chars: usize,
    /// Whether to enable LLM-based middle summarization (Phase 3).
    pub enable_summarization: bool,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            max_tool_result_chars: 4000,
            protect_head: 3,
            protect_tail: 10,
            max_summary_chars: 2000,
            enable_summarization: true,
        }
    }
}

/// Tracks state across multiple compressions within a session.
#[derive(Default)]
pub struct CompressionState {
    /// The current running summary of compressed middle turns.
    pub current_summary: Option<String>,
    /// How many times we've compressed in this session.
    pub compression_count: u32,
}

// ── Phase 1: Tool Result Pruning ────────────────────────────────────────────

/// Truncate `ToolResult` content beyond `max_chars`. Pure function, no LLM call.
/// Returns the number of tool results that were truncated.
pub fn prune_tool_results(messages: &mut [ConversationMessage], max_chars: usize) -> usize {
    if max_chars == 0 {
        return 0;
    }
    let mut truncated = 0;
    for msg in messages.iter_mut() {
        if let ConversationMessage::ToolResult(ref mut result) = msg {
            if result.content.len() > max_chars {
                let original_len = result.content.len();
                // Keep the first `max_chars` characters and add a truncation marker.
                let truncated_content = result.content.chars().take(max_chars).collect::<String>();
                result.content = format!(
                    "{truncated_content}\n\n[... truncated: {original_len} chars total, showing first {max_chars}]"
                );
                truncated += 1;
            }
        }
    }
    truncated
}

// ── Phase 2: Boundary Protection ────────────────────────────────────────────

/// Find the safe middle section that can be compressed, respecting boundaries.
///
/// Returns `(head_end, tail_start)` indices. Messages in `[head_end..tail_start]`
/// are expendable. The function ensures:
/// - System prompt and first user message are always in the head
/// - Tool use / tool result pairs are never split at boundaries
/// - Head and tail sections overlap gracefully when the conversation is short
pub fn find_compressible_range(
    messages: &[ConversationMessage],
    protect_head: usize,
    protect_tail: usize,
) -> (usize, usize) {
    let len = messages.len();
    if len == 0 {
        return (0, 0);
    }

    // Head: at minimum protect system prompt + first user message.
    let min_head = messages
        .iter()
        .position(|m| matches!(m, ConversationMessage::User { .. }))
        .map(|i| i + 1)
        .unwrap_or(1);
    let mut head_end = protect_head.max(min_head).min(len);

    // Extend head to avoid splitting tool pairs: if head_end lands on a
    // ToolResult, walk forward until we're past the tool result run.
    while head_end < len {
        if matches!(&messages[head_end], ConversationMessage::ToolResult(_)) {
            head_end += 1;
        } else {
            break;
        }
    }

    // Tail: protect the last N messages.
    let mut tail_start = len.saturating_sub(protect_tail);

    // Walk tail_start backward to avoid splitting: if it lands on a ToolResult,
    // walk back to include the preceding Assistant with tool_calls.
    while tail_start > 0 {
        if matches!(&messages[tail_start], ConversationMessage::ToolResult(_)) {
            tail_start -= 1;
        } else {
            break;
        }
    }

    // Ensure head doesn't overlap tail.
    if head_end >= tail_start {
        return (len / 2, len / 2); // nothing compressible
    }

    (head_end, tail_start)
}

// ── Phase 3: Middle Turn Summarization ──────────────────────────────────────

/// Summarize the middle section of messages using an LLM call.
///
/// Returns a summary string, or `None` if the middle section is empty or
/// the LLM call fails.
pub async fn summarize_middle(
    messages: &[ConversationMessage],
    head_end: usize,
    tail_start: usize,
    provider: &dyn Provider,
    max_summary_chars: usize,
    existing_summary: Option<&str>,
) -> Option<String> {
    if head_end >= tail_start {
        return None;
    }

    let middle = &messages[head_end..tail_start];
    if middle.is_empty() {
        return None;
    }

    // Serialize middle messages into a text block.
    let mut text = String::new();
    for msg in middle {
        match msg {
            ConversationMessage::System { content } => {
                text.push_str("[System]: ");
                text.push_str(content);
            }
            ConversationMessage::User { content, .. } => {
                text.push_str("[User]: ");
                text.push_str(content);
            }
            ConversationMessage::Assistant {
                content,
                tool_calls,
            } => {
                text.push_str("[Assistant]: ");
                if let Some(c) = content {
                    text.push_str(c);
                }
                for tc in tool_calls {
                    text.push_str(&format!("\n  [tool_call: {}]", tc.name));
                }
            }
            ConversationMessage::ToolResult(r) => {
                // Only include a brief snippet of tool results in the summary input.
                let snippet: String = r.content.chars().take(200).collect();
                text.push_str(&format!("[ToolResult {}]: {snippet}", r.tool_use_id));
                if r.content.len() > 200 {
                    text.push_str("...");
                }
            }
        }
        text.push('\n');
    }

    let prompt = if let Some(existing) = existing_summary {
        format!(
            "You previously summarized a conversation as:\n\n{existing}\n\n\
             New turns have occurred since then:\n\n{text}\n\n\
             Update your summary to incorporate the new information. \
             Keep it under {max_summary_chars} characters. \
             Structure as: Goal / Progress / Decisions / Next Steps."
        )
    } else {
        format!(
            "Summarize the following conversation turns in under {max_summary_chars} characters. \
             Preserve: the user's goal, progress made, key decisions, and what should happen next.\n\
             Structure as: Goal / Progress / Decisions / Next Steps.\n\n{text}"
        )
    };

    match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        provider.complete(&prompt),
    )
    .await
    {
        Ok(Ok(result)) => {
            let summary = result.output_text.trim().to_string();
            if summary.is_empty() {
                None
            } else {
                Some(summary)
            }
        }
        Ok(Err(e)) => {
            warn!(error = %e, "context summarization LLM call failed");
            None
        }
        Err(_) => {
            warn!("context summarization timed out");
            None
        }
    }
}

// ── Full Pipeline ───────────────────────────────────────────────────────────

/// Run the full compression pipeline on a message list.
///
/// - Phase 1 runs in-place (mutates tool results)
/// - Phase 2 identifies the compressible range
/// - Phase 3 (if enabled + provider given) summarizes the middle
/// - Phase 4 merges with existing summary state
///
/// Returns the number of messages removed.
pub async fn compress(
    messages: &mut Vec<ConversationMessage>,
    config: &CompressionConfig,
    state: &mut CompressionState,
    provider: Option<&dyn Provider>,
) -> usize {
    // Phase 1: prune tool results.
    prune_tool_results(messages, config.max_tool_result_chars);

    // Phase 2: find compressible range.
    let (head_end, tail_start) =
        find_compressible_range(messages, config.protect_head, config.protect_tail);

    if head_end >= tail_start {
        return 0; // nothing to compress
    }

    // Phase 3: summarize middle (if LLM available).
    let summary = if config.enable_summarization {
        if let Some(p) = provider {
            summarize_middle(
                messages,
                head_end,
                tail_start,
                p,
                config.max_summary_chars,
                state.current_summary.as_deref(),
            )
            .await
        } else {
            None
        }
    } else {
        None
    };

    // Phase 4: replace middle with summary (or just drop it).
    let middle_len = tail_start - head_end;
    messages.drain(head_end..tail_start);

    if let Some(ref s) = summary {
        // Insert summary as a System message at the boundary.
        messages.insert(
            head_end,
            ConversationMessage::System {
                content: format!("[Conversation summary]: {s}"),
            },
        );
        // Update state for iterative compression.
        state.current_summary = Some(s.clone());
        state.compression_count += 1;
    }

    if summary.is_some() {
        middle_len.saturating_sub(1) // we added 1 summary message
    } else {
        middle_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToolResultMessage, ToolUseRequest};

    fn tool_result(id: &str, content: &str) -> ConversationMessage {
        ConversationMessage::ToolResult(ToolResultMessage {
            tool_use_id: id.to_string(),
            content: content.to_string(),
            is_error: false,
        })
    }

    fn assistant_with_tools(text: &str, tools: &[&str]) -> ConversationMessage {
        ConversationMessage::Assistant {
            content: if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            },
            tool_calls: tools
                .iter()
                .map(|name| ToolUseRequest {
                    id: format!("call_{name}"),
                    name: name.to_string(),
                    input: serde_json::json!({}),
                })
                .collect(),
        }
    }

    // ── Phase 1 tests ──

    #[test]
    fn prune_truncates_long_tool_results() {
        let mut messages = vec![
            ConversationMessage::user("hi".to_string()),
            tool_result("1", &"x".repeat(10000)),
            tool_result("2", "short"),
        ];
        let count = prune_tool_results(&mut messages, 500);
        assert_eq!(count, 1);
        if let ConversationMessage::ToolResult(r) = &messages[1] {
            assert!(r.content.len() < 10000);
            assert!(r.content.contains("[... truncated:"));
        } else {
            panic!("expected ToolResult");
        }
        // Short result unchanged.
        if let ConversationMessage::ToolResult(r) = &messages[2] {
            assert_eq!(r.content, "short");
        }
    }

    #[test]
    fn prune_zero_max_disables() {
        let mut messages = vec![tool_result("1", &"x".repeat(10000))];
        let count = prune_tool_results(&mut messages, 0);
        assert_eq!(count, 0);
    }

    // ── Phase 2 tests ──

    #[test]
    fn find_range_basic() {
        let messages = vec![
            ConversationMessage::System {
                content: "sys".to_string(),
            },
            ConversationMessage::user("first".to_string()),
            ConversationMessage::Assistant {
                content: Some("mid1".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("mid2".to_string()),
            ConversationMessage::Assistant {
                content: Some("mid3".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("mid4".to_string()),
            ConversationMessage::Assistant {
                content: Some("recent1".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("recent2".to_string()),
        ];
        let (head_end, tail_start) = find_compressible_range(&messages, 2, 2);
        assert_eq!(head_end, 2); // after System + User
        assert_eq!(tail_start, 6); // last 2 messages protected
    }

    #[test]
    fn find_range_respects_tool_pairs_at_tail() {
        let messages = vec![
            ConversationMessage::user("first".to_string()),
            ConversationMessage::Assistant {
                content: Some("mid".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("mid2".to_string()),
            assistant_with_tools("", &["shell"]),
            tool_result("call_shell", "output"),
            ConversationMessage::user("last".to_string()),
        ];
        // protect_tail=2 would land on tool_result at index 4, should walk back to 3
        let (_, tail_start) = find_compressible_range(&messages, 1, 2);
        assert!(
            tail_start <= 3,
            "tail should walk back to include the assistant with tool_calls"
        );
    }

    #[test]
    fn find_range_short_conversation_returns_empty() {
        let messages = vec![
            ConversationMessage::user("hi".to_string()),
            ConversationMessage::Assistant {
                content: Some("hello".to_string()),
                tool_calls: vec![],
            },
        ];
        let (head_end, tail_start) = find_compressible_range(&messages, 3, 3);
        assert_eq!(head_end, tail_start, "nothing compressible in short conv");
    }

    // ── Phase 3 tests (no real LLM, just structural) ──

    #[tokio::test]
    async fn summarize_empty_middle_returns_none() {
        // head_end == tail_start → empty middle
        let messages = vec![ConversationMessage::user("hi".to_string())];
        let result = summarize_middle(&messages, 1, 1, &NoOpProvider, 2000, None).await;
        assert!(result.is_none());
    }

    // ── Full pipeline tests ──

    #[tokio::test]
    async fn compress_without_provider_drops_middle() {
        let mut messages = vec![
            ConversationMessage::System {
                content: "system".to_string(),
            },
            ConversationMessage::user("first".to_string()),
            ConversationMessage::Assistant {
                content: Some("mid1".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("mid2".to_string()),
            ConversationMessage::Assistant {
                content: Some("mid3".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("recent1".to_string()),
            ConversationMessage::Assistant {
                content: Some("recent2".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("recent3".to_string()),
        ];
        let config = CompressionConfig {
            max_tool_result_chars: 500,
            protect_head: 2,
            protect_tail: 3,
            enable_summarization: false,
            ..Default::default()
        };
        let mut state = CompressionState::default();
        let removed = compress(&mut messages, &config, &mut state, None).await;
        assert!(removed > 0);
        // System + first user should be preserved.
        assert!(matches!(
            &messages[0],
            ConversationMessage::System { content } if content == "system"
        ));
        assert!(matches!(
            &messages[1],
            ConversationMessage::User { content, .. } if content == "first"
        ));
        // Last messages should be preserved.
        assert!(matches!(
            messages.last().expect("non-empty"),
            ConversationMessage::User { content, .. } if content == "recent3"
        ));
    }

    #[tokio::test]
    async fn compress_with_mock_provider_inserts_summary() {
        let mut messages = vec![
            ConversationMessage::user("first".to_string()),
            ConversationMessage::Assistant {
                content: Some("mid".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("mid2".to_string()),
            ConversationMessage::Assistant {
                content: Some("mid3".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("mid4".to_string()),
            ConversationMessage::Assistant {
                content: Some("mid5".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("recent1".to_string()),
            ConversationMessage::Assistant {
                content: Some("recent2".to_string()),
                tool_calls: vec![],
            },
        ];
        let config = CompressionConfig {
            protect_head: 1,
            protect_tail: 2,
            enable_summarization: true,
            ..Default::default()
        };
        let mut state = CompressionState::default();
        let provider = MockSummaryProvider("Goal: test / Progress: done".to_string());
        let removed = compress(&mut messages, &config, &mut state, Some(&provider)).await;
        assert!(removed > 0);

        // Should have a summary system message.
        let has_summary = messages.iter().any(|m| {
            matches!(
                m,
                ConversationMessage::System { content } if content.contains("Conversation summary")
            )
        });
        assert!(has_summary, "should insert summary message");
        assert!(state.current_summary.is_some());
        assert_eq!(state.compression_count, 1);
    }

    #[tokio::test]
    async fn prune_and_compress_combined() {
        let mut messages = vec![
            ConversationMessage::user("do work".to_string()),
            assistant_with_tools("", &["shell"]),
            tool_result("call_shell", &"x".repeat(50000)),
            ConversationMessage::Assistant {
                content: Some("mid".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("more".to_string()),
            ConversationMessage::Assistant {
                content: Some("done".to_string()),
                tool_calls: vec![],
            },
        ];
        let config = CompressionConfig {
            max_tool_result_chars: 500,
            protect_head: 1,
            protect_tail: 2,
            enable_summarization: false,
            ..Default::default()
        };
        let mut state = CompressionState::default();
        compress(&mut messages, &config, &mut state, None).await;

        // Tool result should be pruned.
        for msg in &messages {
            if let ConversationMessage::ToolResult(r) = msg {
                assert!(
                    r.content.len() < 50000,
                    "tool result should have been pruned"
                );
            }
        }
    }

    // ── Test helpers ──

    struct NoOpProvider;

    #[async_trait::async_trait]
    impl Provider for NoOpProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<crate::ChatResult> {
            Ok(crate::ChatResult::default())
        }
    }

    struct MockSummaryProvider(String);

    #[async_trait::async_trait]
    impl Provider for MockSummaryProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<crate::ChatResult> {
            Ok(crate::ChatResult {
                output_text: self.0.clone(),
                ..crate::ChatResult::default()
            })
        }
    }
}
