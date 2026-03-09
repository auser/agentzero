//! Tool-loop detection with tiered escalation.
//!
//! Three detectors run after each tool call to prevent agents from getting stuck
//! in repetitive loops:
//!
//! 1. **Exact repeat** — Same tool + same arguments N times in a row.
//! 2. **Similarity** — Tool calls with high argument similarity over a sliding window.
//! 3. **Cost runaway** — Token spend exceeds budget threshold.

use crate::LoopAction;
use serde_json::Value;
use std::collections::VecDeque;

/// Configuration for loop detection thresholds.
#[derive(Debug, Clone)]
pub struct LoopDetectionConfig {
    /// Number of identical consecutive calls before escalation (default: 3).
    pub exact_repeat_threshold: usize,
    /// Similarity threshold (0.0–1.0) for the similarity detector (default: 0.9).
    pub similarity_threshold: f64,
    /// Sliding window size for similarity detection (default: 5).
    pub similarity_window: usize,
    /// Maximum tokens per run before cost runaway triggers (0 = disabled).
    pub max_tokens_per_run: u64,
    /// Maximum cost in microdollars per run before cost runaway triggers (0 = disabled).
    pub max_cost_microdollars_per_run: u64,
}

impl Default for LoopDetectionConfig {
    fn default() -> Self {
        Self {
            exact_repeat_threshold: 3,
            similarity_threshold: 0.9,
            similarity_window: 5,
            max_tokens_per_run: 0,
            max_cost_microdollars_per_run: 0,
        }
    }
}

/// A recorded tool invocation for loop analysis.
#[derive(Debug, Clone)]
struct ToolCall {
    name: String,
    args_str: String,
}

/// Stateful detector that tracks tool calls and checks for loops.
#[derive(Debug)]
pub struct ToolLoopDetector {
    config: LoopDetectionConfig,
    history: VecDeque<ToolCall>,
}

impl ToolLoopDetector {
    pub fn new(config: LoopDetectionConfig) -> Self {
        Self {
            config,
            history: VecDeque::new(),
        }
    }

    /// Check a tool call for loop patterns. Returns the highest-severity action.
    pub fn check(
        &mut self,
        tool_name: &str,
        args: &Value,
        tokens_used: u64,
        cost_microdollars: u64,
    ) -> LoopAction {
        let args_str = serde_json::to_string(args).unwrap_or_default();
        let call = ToolCall {
            name: tool_name.to_string(),
            args_str,
        };

        // Add to history (bounded by window size).
        self.history.push_back(call);
        let max_window = self
            .config
            .similarity_window
            .max(self.config.exact_repeat_threshold);
        while self.history.len() > max_window {
            self.history.pop_front();
        }

        // Check detectors in escalation order (lowest severity first).
        // Return the highest severity action found.
        let mut worst = LoopAction::Continue;

        // 1. Exact repeat detector.
        if let Some(action) = self.check_exact_repeat() {
            worst = action;
        }

        // 2. Similarity detector.
        if let Some(action) = self.check_similarity() {
            if severity(&action) > severity(&worst) {
                worst = action;
            }
        }

        // 3. Cost runaway detector.
        if let Some(action) = self.check_cost_runaway(tokens_used, cost_microdollars) {
            if severity(&action) > severity(&worst) {
                worst = action;
            }
        }

        worst
    }

    fn check_exact_repeat(&self) -> Option<LoopAction> {
        let threshold = self.config.exact_repeat_threshold;
        if threshold == 0 || self.history.len() < threshold {
            return None;
        }

        let recent: Vec<_> = self.history.iter().rev().take(threshold).collect();
        let first = &recent[0];
        let all_same = recent
            .iter()
            .all(|c| c.name == first.name && c.args_str == first.args_str);

        if all_same {
            Some(LoopAction::InjectMessage(format!(
                "You have called '{}' with identical arguments {} times in a row. \
                 Try a different approach or different parameters.",
                first.name, threshold
            )))
        } else {
            None
        }
    }

    fn check_similarity(&self) -> Option<LoopAction> {
        let window = self.config.similarity_window;
        if window < 2 || self.history.len() < window {
            return None;
        }

        let recent: Vec<_> = self.history.iter().rev().take(window).collect();

        // Check if all calls in the window are to the same tool with similar args.
        let first = &recent[0];
        let all_same_tool = recent.iter().all(|c| c.name == first.name);
        if !all_same_tool {
            return None;
        }

        // Compute pairwise similarity using Jaccard on character bigrams.
        let mut high_similarity_count = 0;
        let total_pairs = recent.len() - 1;

        for i in 0..total_pairs {
            let sim = jaccard_bigram_similarity(&recent[i].args_str, &recent[i + 1].args_str);
            if sim >= self.config.similarity_threshold {
                high_similarity_count += 1;
            }
        }

        // If most pairs are similar, escalate.
        if high_similarity_count > total_pairs / 2 {
            Some(LoopAction::RestrictTools(vec![first.name.clone()]))
        } else {
            None
        }
    }

    fn check_cost_runaway(&self, tokens_used: u64, cost_microdollars: u64) -> Option<LoopAction> {
        if self.config.max_tokens_per_run > 0 && tokens_used > self.config.max_tokens_per_run {
            return Some(LoopAction::ForceComplete(format!(
                "Token budget exceeded: {} tokens used (limit: {})",
                tokens_used, self.config.max_tokens_per_run
            )));
        }
        if self.config.max_cost_microdollars_per_run > 0
            && cost_microdollars > self.config.max_cost_microdollars_per_run
        {
            return Some(LoopAction::ForceComplete(format!(
                "Cost budget exceeded: {} microdollars (limit: {})",
                cost_microdollars, self.config.max_cost_microdollars_per_run
            )));
        }
        None
    }
}

/// Severity ordering for LoopAction (higher = more severe).
pub fn severity(action: &LoopAction) -> u8 {
    match action {
        LoopAction::Continue => 0,
        LoopAction::InjectMessage(_) => 1,
        LoopAction::RestrictTools(_) => 2,
        LoopAction::ForceComplete(_) => 3,
    }
}

/// Compute Jaccard similarity on character bigrams of two strings.
fn jaccard_bigram_similarity(a: &str, b: &str) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let bigrams_a: std::collections::HashSet<(char, char)> =
        a.chars().zip(a.chars().skip(1)).collect();
    let bigrams_b: std::collections::HashSet<(char, char)> =
        b.chars().zip(b.chars().skip(1)).collect();

    if bigrams_a.is_empty() && bigrams_b.is_empty() {
        return 1.0;
    }

    let intersection = bigrams_a.intersection(&bigrams_b).count();
    let union = bigrams_a.union(&bigrams_b).count();

    if union == 0 {
        return 1.0;
    }

    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_repeat_triggers_after_threshold() {
        let config = LoopDetectionConfig {
            exact_repeat_threshold: 3,
            ..Default::default()
        };
        let mut detector = ToolLoopDetector::new(config);
        let args = json!({"path": "/tmp/foo"});

        assert_eq!(
            detector.check("read_file", &args, 0, 0),
            LoopAction::Continue
        );
        assert_eq!(
            detector.check("read_file", &args, 0, 0),
            LoopAction::Continue
        );
        match detector.check("read_file", &args, 0, 0) {
            LoopAction::InjectMessage(msg) => {
                assert!(msg.contains("read_file"));
                assert!(msg.contains("3 times"));
            }
            other => panic!("expected InjectMessage, got {other:?}"),
        }
    }

    #[test]
    fn exact_repeat_resets_on_different_call() {
        let config = LoopDetectionConfig {
            exact_repeat_threshold: 3,
            ..Default::default()
        };
        let mut detector = ToolLoopDetector::new(config);
        let args = json!({"path": "/tmp/foo"});

        detector.check("read_file", &args, 0, 0);
        detector.check("read_file", &args, 0, 0);
        // Different tool breaks the streak.
        detector.check("write_file", &json!({}), 0, 0);
        assert_eq!(
            detector.check("read_file", &args, 0, 0),
            LoopAction::Continue
        );
    }

    #[test]
    fn exact_repeat_different_args_no_trigger() {
        let config = LoopDetectionConfig {
            exact_repeat_threshold: 3,
            ..Default::default()
        };
        let mut detector = ToolLoopDetector::new(config);

        detector.check("read_file", &json!({"path": "/a"}), 0, 0);
        detector.check("read_file", &json!({"path": "/b"}), 0, 0);
        assert_eq!(
            detector.check("read_file", &json!({"path": "/c"}), 0, 0),
            LoopAction::Continue
        );
    }

    #[test]
    fn similarity_detector_triggers_on_similar_args() {
        let config = LoopDetectionConfig {
            exact_repeat_threshold: 10, // disable exact repeat
            similarity_threshold: 0.8,
            similarity_window: 4,
            ..Default::default()
        };
        let mut detector = ToolLoopDetector::new(config);

        // Very similar args with minor variations.
        detector.check(
            "search",
            &json!({"query": "rust async programming guide"}),
            0,
            0,
        );
        detector.check(
            "search",
            &json!({"query": "rust async programming guide 2"}),
            0,
            0,
        );
        detector.check(
            "search",
            &json!({"query": "rust async programming guide 3"}),
            0,
            0,
        );
        let action = detector.check(
            "search",
            &json!({"query": "rust async programming guide 4"}),
            0,
            0,
        );

        match action {
            LoopAction::RestrictTools(tools) => assert!(tools.contains(&"search".to_string())),
            other => panic!("expected RestrictTools, got {other:?}"),
        }
    }

    #[test]
    fn similarity_detector_no_trigger_on_different_args() {
        let config = LoopDetectionConfig {
            exact_repeat_threshold: 10,
            similarity_threshold: 0.9,
            similarity_window: 3,
            ..Default::default()
        };
        let mut detector = ToolLoopDetector::new(config);

        detector.check("search", &json!({"query": "rust"}), 0, 0);
        detector.check("search", &json!({"query": "python machine learning"}), 0, 0);
        assert_eq!(
            detector.check("search", &json!({"query": "go concurrency patterns"}), 0, 0),
            LoopAction::Continue
        );
    }

    #[test]
    fn cost_runaway_triggers_on_token_limit() {
        let config = LoopDetectionConfig {
            max_tokens_per_run: 1000,
            ..Default::default()
        };
        let mut detector = ToolLoopDetector::new(config);

        assert_eq!(
            detector.check("tool", &json!({}), 500, 0),
            LoopAction::Continue
        );
        match detector.check("tool", &json!({}), 1500, 0) {
            LoopAction::ForceComplete(msg) => assert!(msg.contains("Token budget")),
            other => panic!("expected ForceComplete, got {other:?}"),
        }
    }

    #[test]
    fn cost_runaway_triggers_on_cost_limit() {
        let config = LoopDetectionConfig {
            max_cost_microdollars_per_run: 5000,
            ..Default::default()
        };
        let mut detector = ToolLoopDetector::new(config);

        assert_eq!(
            detector.check("tool", &json!({}), 0, 3000),
            LoopAction::Continue
        );
        match detector.check("tool", &json!({}), 0, 6000) {
            LoopAction::ForceComplete(msg) => assert!(msg.contains("Cost budget")),
            other => panic!("expected ForceComplete, got {other:?}"),
        }
    }

    #[test]
    fn cost_runaway_disabled_when_zero() {
        let config = LoopDetectionConfig {
            max_tokens_per_run: 0,
            max_cost_microdollars_per_run: 0,
            ..Default::default()
        };
        let mut detector = ToolLoopDetector::new(config);

        // Even very high usage shouldn't trigger when limits are 0 (disabled).
        assert_eq!(
            detector.check("tool", &json!({}), 999_999, 999_999),
            LoopAction::Continue
        );
    }

    #[test]
    fn highest_severity_wins() {
        // Trigger both exact repeat (InjectMessage) and cost runaway (ForceComplete).
        let config = LoopDetectionConfig {
            exact_repeat_threshold: 2,
            max_tokens_per_run: 100,
            ..Default::default()
        };
        let mut detector = ToolLoopDetector::new(config);
        let args = json!({"x": 1});

        detector.check("tool", &args, 50, 0);
        // Second identical call + over token limit.
        match detector.check("tool", &args, 200, 0) {
            LoopAction::ForceComplete(_) => {} // ForceComplete > InjectMessage
            other => panic!("expected ForceComplete (highest severity), got {other:?}"),
        }
    }

    #[test]
    fn jaccard_similarity_identical_strings() {
        assert!((jaccard_bigram_similarity("hello", "hello") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_similarity_completely_different() {
        let sim = jaccard_bigram_similarity("abc", "xyz");
        assert!(sim < 0.1);
    }

    #[test]
    fn jaccard_similarity_empty_strings() {
        assert!((jaccard_bigram_similarity("", "") - 1.0).abs() < f64::EPSILON);
        assert!((jaccard_bigram_similarity("abc", "") - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_similarity_single_char_strings() {
        // Single chars have no bigrams.
        assert!((jaccard_bigram_similarity("a", "b") - 1.0).abs() < f64::EPSILON);
    }
}
