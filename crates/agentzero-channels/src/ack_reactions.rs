use crate::ChannelMessage;
use std::collections::HashMap;

/// Strategy for selecting an emoji from the pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmojiStrategy {
    /// Pick a random emoji from the pool.
    Random,
    /// Always use the first emoji.
    First,
}

impl EmojiStrategy {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "first" => Self::First,
            _ => Self::Random,
        }
    }
}

/// A conditional rule for ACK reactions.
#[derive(Debug, Clone, Default)]
pub struct AckRule {
    pub contains_any: Vec<String>,
    pub contains_all: Vec<String>,
    pub contains_none: Vec<String>,
    pub regex_pattern: Option<String>,
    pub sender_ids: Vec<String>,
    pub chat_ids: Vec<String>,
    pub emoji_override: Vec<String>,
}

impl AckRule {
    /// Check if this rule matches the given message.
    pub fn matches(&self, msg: &ChannelMessage) -> bool {
        // sender_ids filter
        if !self.sender_ids.is_empty()
            && !self
                .sender_ids
                .iter()
                .any(|id| id == "*" || id == &msg.sender)
        {
            return false;
        }

        // chat_ids filter
        if !self.chat_ids.is_empty()
            && !self
                .chat_ids
                .iter()
                .any(|id| id == "*" || id == &msg.reply_target)
        {
            return false;
        }

        let content_lower = msg.content.to_lowercase();

        // contains_any: at least one keyword must be present
        if !self.contains_any.is_empty()
            && !self
                .contains_any
                .iter()
                .any(|kw| content_lower.contains(&kw.to_lowercase()))
        {
            return false;
        }

        // contains_all: all keywords must be present
        if !self.contains_all.is_empty()
            && !self
                .contains_all
                .iter()
                .all(|kw| content_lower.contains(&kw.to_lowercase()))
        {
            return false;
        }

        // contains_none: none of these keywords should be present
        if !self.contains_none.is_empty()
            && self
                .contains_none
                .iter()
                .any(|kw| content_lower.contains(&kw.to_lowercase()))
        {
            return false;
        }

        // regex filter
        if let Some(pattern) = &self.regex_pattern {
            if let Ok(re) = regex::Regex::new(pattern) {
                if !re.is_match(&msg.content) {
                    return false;
                }
            }
        }

        true
    }
}

/// Per-channel ACK reaction policy.
#[derive(Debug, Clone)]
pub struct AckReactionPolicy {
    pub enabled: bool,
    pub emoji_pool: Vec<String>,
    pub strategy: EmojiStrategy,
    pub sample_rate: f64,
    pub rules: Vec<AckRule>,
}

impl Default for AckReactionPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            emoji_pool: vec!["👍".into(), "👀".into(), "🤔".into()],
            strategy: EmojiStrategy::Random,
            sample_rate: 1.0,
            rules: Vec::new(),
        }
    }
}

/// Registry of ACK reaction policies keyed by channel name.
#[derive(Debug, Clone, Default)]
pub struct AckReactionEngine {
    policies: HashMap<String, AckReactionPolicy>,
}

impl AckReactionEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_policy(mut self, channel: impl Into<String>, policy: AckReactionPolicy) -> Self {
        self.policies.insert(channel.into(), policy);
        self
    }

    pub fn set_policy(&mut self, channel: impl Into<String>, policy: AckReactionPolicy) {
        self.policies.insert(channel.into(), policy);
    }

    /// Determine which emoji (if any) to react with for a given message.
    /// Returns `None` if no reaction should be sent.
    pub fn select_emoji(&self, msg: &ChannelMessage) -> Option<String> {
        let policy = self.policies.get(&msg.channel)?;
        if !policy.enabled || policy.emoji_pool.is_empty() {
            return None;
        }

        // Check sample rate (deterministic based on message ID for testability)
        if policy.sample_rate < 1.0 {
            let hash = simple_hash(&msg.id);
            let threshold = (policy.sample_rate * u32::MAX as f64) as u32;
            if hash > threshold {
                return None;
            }
        }

        // Check rules — if rules exist, at least one must match
        if !policy.rules.is_empty() {
            let matching_rule = policy.rules.iter().find(|r| r.matches(msg));
            if let Some(rule) = matching_rule {
                // Use emoji override if specified
                if !rule.emoji_override.is_empty() {
                    return Some(select_from_pool(
                        &rule.emoji_override,
                        &policy.strategy,
                        &msg.id,
                    ));
                }
            } else {
                return None; // No rule matched
            }
        }

        Some(select_from_pool(
            &policy.emoji_pool,
            &policy.strategy,
            &msg.id,
        ))
    }
}

fn select_from_pool(pool: &[String], strategy: &EmojiStrategy, seed: &str) -> String {
    match strategy {
        EmojiStrategy::First => pool[0].clone(),
        EmojiStrategy::Random => {
            let idx = simple_hash(seed) as usize % pool.len();
            pool[idx].clone()
        }
    }
}

fn simple_hash(s: &str) -> u32 {
    let mut h: u32 = 0;
    for b in s.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_msg(channel: &str, sender: &str, content: &str) -> ChannelMessage {
        ChannelMessage {
            id: "msg-42".into(),
            sender: sender.into(),
            reply_target: "group-1".into(),
            content: content.into(),
            channel: channel.into(),
            timestamp: 0,
            thread_ts: None,
            privacy_boundary: String::new(),
            attachments: Vec::new(),
        }
    }

    #[test]
    fn no_policy_returns_none() {
        let engine = AckReactionEngine::new();
        assert!(engine
            .select_emoji(&test_msg("telegram", "alice", "hello"))
            .is_none());
    }

    #[test]
    fn disabled_policy_returns_none() {
        let engine = AckReactionEngine::new().with_policy(
            "telegram",
            AckReactionPolicy {
                enabled: false,
                ..Default::default()
            },
        );
        assert!(engine
            .select_emoji(&test_msg("telegram", "alice", "hello"))
            .is_none());
    }

    #[test]
    fn enabled_policy_no_rules_returns_emoji() {
        let engine = AckReactionEngine::new().with_policy(
            "telegram",
            AckReactionPolicy {
                enabled: true,
                ..Default::default()
            },
        );
        let emoji = engine.select_emoji(&test_msg("telegram", "alice", "hello"));
        assert!(emoji.is_some());
    }

    #[test]
    fn first_strategy_always_picks_first() {
        let engine = AckReactionEngine::new().with_policy(
            "telegram",
            AckReactionPolicy {
                enabled: true,
                strategy: EmojiStrategy::First,
                emoji_pool: vec!["🚀".into(), "💯".into()],
                ..Default::default()
            },
        );
        let emoji = engine
            .select_emoji(&test_msg("telegram", "alice", "hello"))
            .unwrap();
        assert_eq!(emoji, "🚀");
    }

    #[test]
    fn rule_contains_any_matches() {
        let rule = AckRule {
            contains_any: vec!["help".into(), "please".into()],
            ..Default::default()
        };
        let msg = test_msg("telegram", "alice", "can you help me?");
        assert!(rule.matches(&msg));
    }

    #[test]
    fn rule_contains_any_no_match() {
        let rule = AckRule {
            contains_any: vec!["help".into(), "please".into()],
            ..Default::default()
        };
        let msg = test_msg("telegram", "alice", "hello world");
        assert!(!rule.matches(&msg));
    }

    #[test]
    fn rule_contains_all_requires_all() {
        let rule = AckRule {
            contains_all: vec!["help".into(), "urgent".into()],
            ..Default::default()
        };
        assert!(rule.matches(&test_msg("telegram", "alice", "help it's urgent")));
        assert!(!rule.matches(&test_msg("telegram", "alice", "help with something")));
    }

    #[test]
    fn rule_contains_none_excludes() {
        let rule = AckRule {
            contains_none: vec!["spam".into()],
            ..Default::default()
        };
        assert!(rule.matches(&test_msg("telegram", "alice", "hello")));
        assert!(!rule.matches(&test_msg("telegram", "alice", "this is spam")));
    }

    #[test]
    fn rule_sender_filter() {
        let rule = AckRule {
            sender_ids: vec!["alice".into()],
            ..Default::default()
        };
        assert!(rule.matches(&test_msg("telegram", "alice", "hello")));
        assert!(!rule.matches(&test_msg("telegram", "bob", "hello")));
    }

    #[test]
    fn rule_emoji_override() {
        let engine = AckReactionEngine::new().with_policy(
            "telegram",
            AckReactionPolicy {
                enabled: true,
                strategy: EmojiStrategy::First,
                rules: vec![AckRule {
                    contains_any: vec!["urgent".into()],
                    emoji_override: vec!["🚨".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let emoji = engine
            .select_emoji(&test_msg("telegram", "alice", "urgent!"))
            .unwrap();
        assert_eq!(emoji, "🚨");
    }

    #[test]
    fn no_matching_rule_returns_none() {
        let engine = AckReactionEngine::new().with_policy(
            "telegram",
            AckReactionPolicy {
                enabled: true,
                rules: vec![AckRule {
                    contains_any: vec!["urgent".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        assert!(engine
            .select_emoji(&test_msg("telegram", "alice", "hello"))
            .is_none());
    }

    #[test]
    fn empty_emoji_pool_returns_none() {
        let engine = AckReactionEngine::new().with_policy(
            "telegram",
            AckReactionPolicy {
                enabled: true,
                emoji_pool: Vec::new(),
                ..Default::default()
            },
        );
        assert!(engine
            .select_emoji(&test_msg("telegram", "alice", "hello"))
            .is_none());
    }

    #[test]
    fn emoji_strategy_from_str() {
        assert_eq!(EmojiStrategy::parse("first"), EmojiStrategy::First);
        assert_eq!(EmojiStrategy::parse("random"), EmojiStrategy::Random);
        assert_eq!(EmojiStrategy::parse("anything"), EmojiStrategy::Random);
    }
}
