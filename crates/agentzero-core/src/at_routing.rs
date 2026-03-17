//! `@agent` mention parsing for message routing.
//!
//! When a message starts with `@agent_name`, it should be routed to the named
//! agent via the existing delegation mechanism.

/// Parse an `@agent` mention at the start of a message.
///
/// Returns `(agent_name, remaining_message)` if the message begins with
/// `@some_name`. The remaining message is trimmed of leading whitespace.
/// Returns `None` if the message does not start with a valid `@mention`.
///
/// # Examples
///
/// ```
/// use agentzero_core::parse_at_mention;
///
/// let (name, rest) = parse_at_mention("@reviewer check this code").unwrap();
/// assert_eq!(name, "reviewer");
/// assert_eq!(rest, "check this code");
///
/// assert!(parse_at_mention("hello @reviewer").is_none());
/// ```
pub fn parse_at_mention(text: &str) -> Option<(&str, &str)> {
    let trimmed = text.trim();
    if !trimmed.starts_with('@') {
        return None;
    }
    // Find the end of the agent name (first whitespace or end of string)
    let name_end = trimmed[1..]
        .find(char::is_whitespace)
        .map(|i| i + 1)
        .unwrap_or(trimmed.len());
    let name = &trimmed[1..name_end];
    if name.is_empty() {
        return None;
    }
    let rest = trimmed[name_end..].trim();
    Some((name, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_mention_with_message() {
        let result = parse_at_mention("@reviewer check this code");
        assert_eq!(result, Some(("reviewer", "check this code")));
    }

    #[test]
    fn mention_without_message() {
        let result = parse_at_mention("@writer");
        assert_eq!(result, Some(("writer", "")));
    }

    #[test]
    fn mention_not_at_start() {
        assert_eq!(parse_at_mention("hello @reviewer"), None);
    }

    #[test]
    fn at_sign_followed_by_space() {
        assert_eq!(parse_at_mention("@ no name"), None);
    }

    #[test]
    fn hyphenated_agent_name() {
        let result = parse_at_mention("@my-agent do stuff");
        assert_eq!(result, Some(("my-agent", "do stuff")));
    }

    #[test]
    fn underscored_agent_name() {
        let result = parse_at_mention("@agent_with_underscores task");
        assert_eq!(result, Some(("agent_with_underscores", "task")));
    }

    #[test]
    fn empty_string() {
        assert_eq!(parse_at_mention(""), None);
    }

    #[test]
    fn just_at_sign() {
        assert_eq!(parse_at_mention("@"), None);
    }

    #[test]
    fn leading_whitespace_is_trimmed() {
        let result = parse_at_mention("  @reviewer check this");
        assert_eq!(result, Some(("reviewer", "check this")));
    }

    #[test]
    fn extra_whitespace_between_name_and_message() {
        let result = parse_at_mention("@reviewer    lots of space");
        assert_eq!(result, Some(("reviewer", "lots of space")));
    }

    #[test]
    fn trailing_whitespace_only_after_name() {
        let result = parse_at_mention("@writer   ");
        assert_eq!(result, Some(("writer", "")));
    }
}
