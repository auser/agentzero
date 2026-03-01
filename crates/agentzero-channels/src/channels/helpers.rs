use std::time::{SystemTime, UNIX_EPOCH};

/// Generate a new unique message ID (UUID v4).
pub fn new_message_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Get the current Unix timestamp in seconds.
pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_secs()
}

/// Check if a user is in the allowlist.
/// Returns true if the allowlist is empty (open access) or contains "*" or the user.
#[allow(dead_code)]
pub fn is_user_allowed(user: &str, allowlist: &[String]) -> bool {
    allowlist.is_empty()
        || allowlist
            .iter()
            .any(|allowed| allowed == "*" || allowed == user)
}

/// Split a message into chunks that respect a platform's max length.
/// Tries to split at newlines or word boundaries when possible.
#[allow(dead_code)]
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let continuation_overhead = 30; // "(continued)\n\n" + "\n\n(continues...)"
    let chunk_limit = max_len.saturating_sub(continuation_overhead);
    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        let hard_split = remaining
            .char_indices()
            .nth(chunk_limit)
            .map_or(remaining.len(), |(idx, _)| idx);

        let chunk_end = if hard_split == remaining.len() {
            hard_split
        } else {
            let search_area = &remaining[..hard_split];
            if let Some(pos) = search_area.rfind('\n') {
                if search_area[..pos].len() >= chunk_limit / 2 {
                    pos + 1
                } else {
                    search_area.rfind(' ').unwrap_or(hard_split) + 1
                }
            } else if let Some(pos) = search_area.rfind(' ') {
                pos + 1
            } else {
                hard_split
            }
        };

        chunks.push(remaining[..chunk_end].to_string());
        remaining = &remaining[chunk_end..];
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_message_id_is_unique() {
        let a = new_message_id();
        let b = new_message_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36); // UUID v4 format
    }

    #[test]
    fn now_epoch_secs_is_reasonable() {
        let ts = now_epoch_secs();
        assert!(ts > 1_700_000_000); // after ~2023
    }

    #[test]
    fn is_user_allowed_empty_allowlist_allows_all() {
        assert!(is_user_allowed("anyone", &[]));
    }

    #[test]
    fn is_user_allowed_wildcard_allows_all() {
        let list = vec!["*".to_string()];
        assert!(is_user_allowed("anyone", &list));
    }

    #[test]
    fn is_user_allowed_specific_user() {
        let list = vec!["alice".to_string(), "bob".to_string()];
        assert!(is_user_allowed("alice", &list));
        assert!(!is_user_allowed("charlie", &list));
    }

    #[test]
    fn split_message_short_text_no_split() {
        let chunks = split_message("hello", 100);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn split_message_long_text_splits_at_boundary() {
        let text = "word ".repeat(100); // 500 chars
        let chunks = split_message(&text, 100);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.len() <= 100);
        }
    }
}
