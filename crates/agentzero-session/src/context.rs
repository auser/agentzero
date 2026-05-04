//! Context management for long conversations.
//!
//! Implements compaction strategies to keep conversations within
//! model context limits without losing important information.

use crate::ollama::ChatMessage;

/// Context compaction configuration.
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Maximum number of messages to keep before compacting.
    pub max_messages: usize,
    /// Number of recent messages to always preserve.
    pub preserve_recent: usize,
    /// Maximum total character length before compacting.
    pub max_chars: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_messages: 50,
            preserve_recent: 10,
            max_chars: 32_000,
        }
    }
}

/// Compact a conversation by summarizing older messages.
///
/// Strategy:
/// 1. Always keep the system message (index 0)
/// 2. Always keep the most recent `preserve_recent` messages
/// 3. Summarize everything in between into a single "context summary" message
pub fn compact(messages: &[ChatMessage], config: &ContextConfig) -> Vec<ChatMessage> {
    // No compaction needed if under limits
    if messages.len() <= config.max_messages {
        let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
        if total_chars <= config.max_chars {
            return messages.to_vec();
        }
    }

    if messages.len() <= config.preserve_recent + 1 {
        return messages.to_vec();
    }

    let mut result = Vec::new();

    // Keep system message
    if let Some(first) = messages.first() {
        if first.role == "system" {
            result.push(first.clone());
        }
    }

    // Determine split point
    let start_preserve = messages.len().saturating_sub(config.preserve_recent);
    let summary_start = if messages.first().is_some_and(|m| m.role == "system") {
        1
    } else {
        0
    };
    let summary_end = start_preserve;

    // Summarize middle messages
    if summary_end > summary_start {
        let middle = &messages[summary_start..summary_end];
        let summary = summarize_messages(middle);
        result.push(ChatMessage::system(format!(
            "[Context summary of {} earlier messages]\n{}",
            middle.len(),
            summary
        )));
    }

    // Keep recent messages
    result.extend_from_slice(&messages[start_preserve..]);

    result
}

/// Create a text summary of a sequence of messages.
fn summarize_messages(messages: &[ChatMessage]) -> String {
    let mut summary = String::new();

    let mut user_topics = Vec::new();
    let mut tool_calls = Vec::new();
    let mut assistant_points = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "user" => {
                let topic = if msg.content.len() > 80 {
                    format!("{}...", &msg.content[..80])
                } else {
                    msg.content.clone()
                };
                user_topics.push(topic);
            }
            "assistant" => {
                let point = if msg.content.len() > 100 {
                    format!("{}...", &msg.content[..100])
                } else {
                    msg.content.clone()
                };
                assistant_points.push(point);
            }
            "tool" => {
                let preview = if msg.content.len() > 60 {
                    format!("{}...", &msg.content[..60])
                } else {
                    msg.content.clone()
                };
                tool_calls.push(preview);
            }
            _ => {}
        }
    }

    if !user_topics.is_empty() {
        summary.push_str("User asked about: ");
        summary.push_str(&user_topics.join("; "));
        summary.push('\n');
    }
    if !tool_calls.is_empty() {
        summary.push_str(&format!("Tools used: {} calls\n", tool_calls.len()));
    }
    if !assistant_points.is_empty() {
        summary.push_str("Key points discussed: ");
        // Keep only first few to avoid bloat
        let kept: Vec<_> = assistant_points.iter().take(3).cloned().collect();
        summary.push_str(&kept.join("; "));
        if assistant_points.len() > 3 {
            summary.push_str(&format!(" (+{} more)", assistant_points.len() - 3));
        }
        summary.push('\n');
    }

    summary
}

/// Check if compaction is needed.
pub fn needs_compaction(messages: &[ChatMessage], config: &ContextConfig) -> bool {
    if messages.len() > config.max_messages {
        return true;
    }
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    total_chars > config.max_chars
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(count: usize) -> Vec<ChatMessage> {
        let mut msgs = vec![ChatMessage::system("You are a helpful assistant.")];
        for i in 0..count {
            msgs.push(ChatMessage::user(format!("Question {i}")));
            msgs.push(ChatMessage::assistant(format!("Answer to question {i}")));
        }
        msgs
    }

    #[test]
    fn no_compaction_when_under_limit() {
        let msgs = make_messages(5); // 11 messages
        let config = ContextConfig {
            max_messages: 50,
            ..Default::default()
        };
        let result = compact(&msgs, &config);
        assert_eq!(result.len(), msgs.len());
    }

    #[test]
    fn compacts_when_over_message_limit() {
        let msgs = make_messages(30); // 61 messages
        let config = ContextConfig {
            max_messages: 20,
            preserve_recent: 6,
            ..Default::default()
        };
        let result = compact(&msgs, &config);
        // System + summary + 6 recent
        assert!(result.len() <= 8);
        // System message preserved
        assert_eq!(result[0].role, "system");
        // Last message is from original
        assert_eq!(
            result.last().expect("should have last").role,
            msgs.last().expect("should have").role
        );
    }

    #[test]
    fn preserves_system_message() {
        let msgs = make_messages(30);
        let config = ContextConfig {
            max_messages: 10,
            preserve_recent: 4,
            ..Default::default()
        };
        let result = compact(&msgs, &config);
        assert_eq!(result[0].role, "system");
        assert!(result[0].content.contains("helpful assistant"));
    }

    #[test]
    fn summary_contains_context_info() {
        let msgs = make_messages(30);
        let config = ContextConfig {
            max_messages: 10,
            preserve_recent: 4,
            ..Default::default()
        };
        let result = compact(&msgs, &config);
        // Second message should be the summary
        assert!(result[1].content.contains("Context summary"));
        assert!(result[1].content.contains("earlier messages"));
    }

    #[test]
    fn needs_compaction_detects_overflow() {
        let msgs = make_messages(30);
        let config = ContextConfig {
            max_messages: 20,
            ..Default::default()
        };
        assert!(needs_compaction(&msgs, &config));
    }

    #[test]
    fn needs_compaction_false_when_ok() {
        let msgs = make_messages(5);
        let config = ContextConfig::default();
        assert!(!needs_compaction(&msgs, &config));
    }
}
