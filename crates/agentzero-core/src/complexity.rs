//! Message complexity scoring for cost-aware model routing.
//!
//! Evaluates a user query across multiple signals (length, code presence,
//! keyword complexity, tool hints) to produce a [`ComplexityTier`] that
//! the model router can use to select cheaper models for simple queries.

use serde::{Deserialize, Serialize};

/// Complexity tier for routing decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComplexityTier {
    /// Simple queries: greetings, factual lookups, short questions.
    /// Route to cheapest available model.
    Simple,
    /// Medium complexity: multi-step instructions, moderate code.
    /// Route to mid-tier model.
    Medium,
    /// Complex: code generation, analysis, multi-tool tasks.
    /// Route to premium model.
    Complex,
}

/// Individual scoring signals (0.0–1.0 each).
#[derive(Debug, Clone)]
pub struct ComplexityScore {
    pub char_count_score: f32,
    pub word_count_score: f32,
    pub code_presence_score: f32,
    pub keyword_score: f32,
    pub composite: f32,
    pub tier: ComplexityTier,
}

/// Configurable thresholds for tier boundaries.
#[derive(Debug, Clone)]
pub struct ComplexityConfig {
    /// Composite score below this → Simple.
    pub simple_threshold: f32,
    /// Composite score above this → Complex. Between thresholds → Medium.
    pub complex_threshold: f32,
}

impl Default for ComplexityConfig {
    fn default() -> Self {
        Self {
            simple_threshold: 0.15,
            complex_threshold: 0.35,
        }
    }
}

/// Score the complexity of a user query.
pub fn score(query: &str, config: &ComplexityConfig) -> ComplexityScore {
    let char_count_score = score_char_count(query);
    let word_count_score = score_word_count(query);
    let code_presence_score = score_code_presence(query);
    let keyword_score = score_keywords(query);

    // Weighted composite. Keywords and code are the dominant signals;
    // length provides minor adjustment.
    let composite = char_count_score * 0.10
        + word_count_score * 0.10
        + code_presence_score * 0.35
        + keyword_score * 0.45;

    let tier = if composite < config.simple_threshold {
        ComplexityTier::Simple
    } else if composite > config.complex_threshold {
        ComplexityTier::Complex
    } else {
        ComplexityTier::Medium
    };

    ComplexityScore {
        char_count_score,
        word_count_score,
        code_presence_score,
        keyword_score,
        composite,
        tier,
    }
}

/// Normalized character count (0.0 for short, 1.0 for long).
fn score_char_count(query: &str) -> f32 {
    let len = query.len() as f32;
    // Under 50 chars → 0, over 2000 chars → 1.
    ((len - 50.0) / 1950.0).clamp(0.0, 1.0)
}

/// Normalized word count.
fn score_word_count(query: &str) -> f32 {
    let words = query.split_whitespace().count() as f32;
    // Under 10 words → 0, over 200 words → 1.
    ((words - 10.0) / 190.0).clamp(0.0, 1.0)
}

/// Detect code presence: markdown code blocks, import statements, braces.
fn score_code_presence(query: &str) -> f32 {
    let mut signals = 0u32;
    let total_signals = 6u32;

    if query.contains("```") {
        signals += 2; // strong signal
    }
    if query.contains("import ") || query.contains("from ") || query.contains("use ") {
        signals += 1;
    }
    if query.contains("fn ") || query.contains("def ") || query.contains("function ") {
        signals += 1;
    }
    if query.contains("class ") || query.contains("struct ") || query.contains("interface ") {
        signals += 1;
    }
    // Curly braces suggest code structure.
    if query.contains('{') && query.contains('}') {
        signals += 1;
    }

    signals as f32 / total_signals as f32
}

/// Keyword-based complexity scoring. Returns 0.0–1.0.
fn score_keywords(query: &str) -> f32 {
    let lower = query.to_lowercase();

    let complex_keywords = [
        "implement",
        "refactor",
        "architect",
        "optimize",
        "debug",
        "analyze",
        "design",
        "migrate",
        "deploy",
        "benchmark",
    ];
    let medium_keywords = [
        "explain",
        "compare",
        "summarize",
        "create",
        "modify",
        "update",
        "fix",
        "change",
        "add",
        "build",
    ];
    let simple_keywords = [
        "what is", "how do", "when", "where", "who", "hello", "hi", "thanks", "yes", "no",
    ];

    let complex_hits = complex_keywords
        .iter()
        .filter(|kw| lower.contains(**kw))
        .count();
    let medium_hits = medium_keywords
        .iter()
        .filter(|kw| lower.contains(**kw))
        .count();
    let is_simple = simple_keywords.iter().any(|kw| lower.contains(kw));

    // Base score from keyword category.
    let mut base = if complex_hits > 0 {
        0.7 + (complex_hits as f32 - 1.0) * 0.1 // 0.7 for first, +0.1 per additional
    } else if medium_hits > 0 {
        0.35 + (medium_hits as f32 - 1.0) * 0.1 // 0.35 for first, +0.1 per additional
    } else {
        0.0
    };

    // Bonus signals.
    if lower.contains("step") || lower.contains("first") || lower.contains("then") {
        base += 0.1;
    }
    if lower.contains("file") || lower.contains("database") || lower.contains("api") {
        base += 0.05;
    }
    if lower.contains("must") || lower.contains("should") || lower.contains("requirement") {
        base += 0.1;
    }
    if lower.chars().filter(|c| *c == '?').count() > 1 {
        base += 0.05;
    }

    // Discount for simple patterns.
    if is_simple && complex_hits == 0 && medium_hits <= 1 {
        base *= 0.3;
    }

    base.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_greeting() {
        let s = score("hello", &ComplexityConfig::default());
        assert_eq!(s.tier, ComplexityTier::Simple);
        assert!(s.composite < 0.3);
    }

    #[test]
    fn simple_question() {
        let s = score(
            "what is the capital of France?",
            &ComplexityConfig::default(),
        );
        assert_eq!(s.tier, ComplexityTier::Simple);
    }

    #[test]
    fn medium_task() {
        let s = score(
            "explain how the authentication system works and compare it with OAuth",
            &ComplexityConfig::default(),
        );
        assert!(
            s.tier == ComplexityTier::Medium || s.tier == ComplexityTier::Complex,
            "explain + compare should be at least medium, got {:?}",
            s.tier
        );
    }

    #[test]
    fn complex_code_generation() {
        let query = "implement a REST API endpoint that handles user authentication \
                     with JWT tokens. Must support refresh tokens and rate limiting. \
                     ```rust\nfn handle_auth() {}\n```";
        let s = score(query, &ComplexityConfig::default());
        assert_eq!(
            s.tier,
            ComplexityTier::Complex,
            "code generation should be complex"
        );
    }

    #[test]
    fn complex_refactoring() {
        let s = score(
            "refactor the database module to use connection pooling. \
             First update the config, then migrate the queries, then add tests.",
            &ComplexityConfig::default(),
        );
        assert_eq!(s.tier, ComplexityTier::Complex);
    }

    #[test]
    fn code_block_boosts_score() {
        let without = score("write a function", &ComplexityConfig::default());
        let with = score(
            "write a function\n```python\ndef foo():\n    pass\n```",
            &ComplexityConfig::default(),
        );
        assert!(
            with.code_presence_score > without.code_presence_score,
            "code block should increase code presence score"
        );
    }

    #[test]
    fn custom_thresholds() {
        let config = ComplexityConfig {
            simple_threshold: 0.1,
            complex_threshold: 0.2,
        };
        let s = score("explain authentication", &config);
        // With lower thresholds, more things are complex.
        assert!(s.tier != ComplexityTier::Simple);
    }

    #[test]
    fn empty_query_is_simple() {
        let s = score("", &ComplexityConfig::default());
        assert_eq!(s.tier, ComplexityTier::Simple);
    }
}
