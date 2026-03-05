use crate::ChannelMessage;
use std::collections::HashMap;

/// Group reply mode — determines which messages to process in group chats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupReplyMode {
    /// Process all messages in the group.
    AllMessages,
    /// Only process messages that mention the bot by name.
    MentionOnly,
}

impl GroupReplyMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "mention_only" | "mentiononly" | "mention" => Self::MentionOnly,
            _ => Self::AllMessages,
        }
    }
}

/// Per-channel group reply policy.
#[derive(Debug, Clone)]
pub struct GroupReplyPolicy {
    pub mode: GroupReplyMode,
    pub allowed_sender_ids: Vec<String>,
    pub bot_name: Option<String>,
}

impl Default for GroupReplyPolicy {
    fn default() -> Self {
        Self {
            mode: GroupReplyMode::AllMessages,
            allowed_sender_ids: Vec::new(),
            bot_name: None,
        }
    }
}

/// Registry of group reply policies keyed by channel name.
#[derive(Debug, Clone, Default)]
pub struct GroupReplyFilter {
    policies: HashMap<String, GroupReplyPolicy>,
}

impl GroupReplyFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_policy(mut self, channel: impl Into<String>, policy: GroupReplyPolicy) -> Self {
        self.policies.insert(channel.into(), policy);
        self
    }

    pub fn set_policy(&mut self, channel: impl Into<String>, policy: GroupReplyPolicy) {
        self.policies.insert(channel.into(), policy);
    }

    /// Check if a message should be processed according to the group reply policy.
    /// Returns true if the message should be processed, false if it should be dropped.
    pub fn should_process(&self, msg: &ChannelMessage) -> bool {
        let policy = match self.policies.get(&msg.channel) {
            Some(p) => p,
            None => return true, // No policy = allow all
        };

        match policy.mode {
            GroupReplyMode::AllMessages => true,
            GroupReplyMode::MentionOnly => {
                // Always allow messages from allowed sender IDs
                if !policy.allowed_sender_ids.is_empty()
                    && policy
                        .allowed_sender_ids
                        .iter()
                        .any(|id| id == "*" || id.eq_ignore_ascii_case(&msg.sender))
                {
                    return true;
                }

                // Check if the message mentions the bot name
                if let Some(bot_name) = &policy.bot_name {
                    let content_lower = msg.content.to_lowercase();
                    let bot_lower = bot_name.to_lowercase();

                    // Check for @mention or plain name mention
                    content_lower.contains(&format!("@{bot_lower}"))
                        || content_lower.contains(&bot_lower)
                } else {
                    // No bot name configured and not in allowed_sender_ids — drop
                    false
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_msg(channel: &str, sender: &str, content: &str) -> ChannelMessage {
        ChannelMessage {
            id: "1".into(),
            sender: sender.into(),
            reply_target: sender.into(),
            content: content.into(),
            channel: channel.into(),
            timestamp: 0,
            thread_ts: None,
            privacy_boundary: String::new(),
        }
    }

    #[test]
    fn no_policy_allows_all_messages() {
        let filter = GroupReplyFilter::new();
        assert!(filter.should_process(&test_msg("telegram", "alice", "hello")));
    }

    #[test]
    fn all_messages_mode_allows_all() {
        let filter = GroupReplyFilter::new().with_policy(
            "telegram",
            GroupReplyPolicy {
                mode: GroupReplyMode::AllMessages,
                ..Default::default()
            },
        );
        assert!(filter.should_process(&test_msg("telegram", "alice", "hello")));
    }

    #[test]
    fn mention_only_drops_non_mention() {
        let filter = GroupReplyFilter::new().with_policy(
            "telegram",
            GroupReplyPolicy {
                mode: GroupReplyMode::MentionOnly,
                bot_name: Some("MyBot".into()),
                ..Default::default()
            },
        );
        assert!(!filter.should_process(&test_msg("telegram", "alice", "hello everyone")));
    }

    #[test]
    fn mention_only_allows_at_mention() {
        let filter = GroupReplyFilter::new().with_policy(
            "telegram",
            GroupReplyPolicy {
                mode: GroupReplyMode::MentionOnly,
                bot_name: Some("MyBot".into()),
                ..Default::default()
            },
        );
        assert!(filter.should_process(&test_msg("telegram", "alice", "hey @mybot help me")));
    }

    #[test]
    fn mention_only_allows_name_mention() {
        let filter = GroupReplyFilter::new().with_policy(
            "telegram",
            GroupReplyPolicy {
                mode: GroupReplyMode::MentionOnly,
                bot_name: Some("MyBot".into()),
                ..Default::default()
            },
        );
        assert!(filter.should_process(&test_msg("telegram", "alice", "MyBot can you help?")));
    }

    #[test]
    fn mention_only_allows_allowed_sender() {
        let filter = GroupReplyFilter::new().with_policy(
            "telegram",
            GroupReplyPolicy {
                mode: GroupReplyMode::MentionOnly,
                bot_name: Some("MyBot".into()),
                allowed_sender_ids: vec!["admin".into()],
            },
        );
        // Admin bypasses mention requirement
        assert!(filter.should_process(&test_msg("telegram", "admin", "do something")));
        // Non-admin without mention is dropped
        assert!(!filter.should_process(&test_msg("telegram", "alice", "do something")));
    }

    #[test]
    fn mention_only_wildcard_sender_allows_all() {
        let filter = GroupReplyFilter::new().with_policy(
            "telegram",
            GroupReplyPolicy {
                mode: GroupReplyMode::MentionOnly,
                bot_name: None,
                allowed_sender_ids: vec!["*".into()],
            },
        );
        assert!(filter.should_process(&test_msg("telegram", "anyone", "anything")));
    }

    #[test]
    fn mention_only_no_bot_name_no_allowed_senders_drops() {
        let filter = GroupReplyFilter::new().with_policy(
            "telegram",
            GroupReplyPolicy {
                mode: GroupReplyMode::MentionOnly,
                bot_name: None,
                allowed_sender_ids: Vec::new(),
            },
        );
        assert!(!filter.should_process(&test_msg("telegram", "alice", "hello")));
    }

    #[test]
    fn different_channel_not_affected() {
        let filter = GroupReplyFilter::new().with_policy(
            "telegram",
            GroupReplyPolicy {
                mode: GroupReplyMode::MentionOnly,
                bot_name: Some("MyBot".into()),
                ..Default::default()
            },
        );
        // Discord has no policy, so it allows all
        assert!(filter.should_process(&test_msg("discord", "alice", "hello")));
    }

    #[test]
    fn group_reply_mode_from_str() {
        assert_eq!(
            GroupReplyMode::parse("mention_only"),
            GroupReplyMode::MentionOnly
        );
        assert_eq!(
            GroupReplyMode::parse("MentionOnly"),
            GroupReplyMode::MentionOnly
        );
        assert_eq!(
            GroupReplyMode::parse("mention"),
            GroupReplyMode::MentionOnly
        );
        assert_eq!(
            GroupReplyMode::parse("all_messages"),
            GroupReplyMode::AllMessages
        );
        assert_eq!(
            GroupReplyMode::parse("anything_else"),
            GroupReplyMode::AllMessages
        );
    }
}
