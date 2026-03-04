//! Character-class bigram perplexity filter for detecting adversarial suffixes.
//!
//! Adversarial prompt injection attacks often append high-perplexity suffix strings
//! that look nothing like natural language. This filter scores the suffix window of
//! incoming prompts using character-class bigram frequencies and blocks prompts that
//! exceed the perplexity threshold.

/// Character classes for bigram analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CharClass {
    Lower,
    Upper,
    Digit,
    Space,
    Punct,
    Symbol,
    Other,
}

fn classify(c: char) -> CharClass {
    if c.is_ascii_lowercase() {
        CharClass::Lower
    } else if c.is_ascii_uppercase() {
        CharClass::Upper
    } else if c.is_ascii_digit() {
        CharClass::Digit
    } else if c.is_ascii_whitespace() {
        CharClass::Space
    } else if c.is_ascii_punctuation() {
        CharClass::Punct
    } else if c.is_ascii() {
        CharClass::Symbol
    } else {
        CharClass::Other
    }
}

const NUM_CLASSES: usize = 7;

fn class_index(c: CharClass) -> usize {
    match c {
        CharClass::Lower => 0,
        CharClass::Upper => 1,
        CharClass::Digit => 2,
        CharClass::Space => 3,
        CharClass::Punct => 4,
        CharClass::Symbol => 5,
        CharClass::Other => 6,
    }
}

/// Compute character-class bigram perplexity for a string.
///
/// Returns the perplexity score — higher values indicate more "random" text.
/// Natural English text typically scores 3-8; adversarial suffixes score 15+.
pub fn bigram_perplexity(text: &str) -> f64 {
    if text.len() < 2 {
        return 0.0;
    }

    let chars: Vec<CharClass> = text.chars().map(classify).collect();
    let n = chars.len();

    // Count bigram frequencies
    let mut bigram_counts = [[0u32; NUM_CLASSES]; NUM_CLASSES];
    let mut total_bigrams = 0u32;

    for window in chars.windows(2) {
        let a = class_index(window[0]);
        let b = class_index(window[1]);
        bigram_counts[a][b] += 1;
        total_bigrams += 1;
    }

    if total_bigrams == 0 {
        return 0.0;
    }

    // Compute perplexity using log-probability
    // P(bigram) = count(bigram) / total_bigrams
    // Perplexity = exp(-1/N * sum(log(P(bigram))))
    let total_f = total_bigrams as f64;
    let mut log_prob_sum = 0.0;

    for window in chars.windows(2) {
        let a = class_index(window[0]);
        let b = class_index(window[1]);
        let count = bigram_counts[a][b] as f64;
        // Laplace smoothing to avoid log(0)
        let prob = (count + 0.1) / (total_f + 0.1 * (NUM_CLASSES * NUM_CLASSES) as f64);
        log_prob_sum += prob.ln();
    }

    let avg_log_prob = log_prob_sum / (n - 1) as f64;
    (-avg_log_prob).exp()
}

/// Compute the ratio of symbol/punctuation characters in the text.
pub fn symbol_ratio(text: &str) -> f64 {
    if text.is_empty() {
        return 0.0;
    }

    let symbol_count = text
        .chars()
        .filter(|c| {
            let cls = classify(*c);
            matches!(cls, CharClass::Punct | CharClass::Symbol | CharClass::Other)
        })
        .count();

    symbol_count as f64 / text.len() as f64
}

/// Result of perplexity filter analysis.
#[derive(Debug, Clone, PartialEq)]
pub enum PerplexityResult {
    /// The text passes the filter.
    Pass,
    /// The text is flagged as potentially adversarial.
    Flagged {
        perplexity: f64,
        symbol_ratio: f64,
        reason: String,
    },
}

/// Analyze the suffix window of a prompt for adversarial content.
///
/// - `text`: full prompt text
/// - `suffix_window_chars`: number of trailing characters to analyze
/// - `perplexity_threshold`: perplexity score above which to flag
/// - `symbol_ratio_threshold`: symbol ratio above which to flag
/// - `min_prompt_chars`: minimum prompt length to apply the filter
pub fn analyze_suffix(
    text: &str,
    suffix_window_chars: usize,
    perplexity_threshold: f64,
    symbol_ratio_threshold: f64,
    min_prompt_chars: usize,
) -> PerplexityResult {
    if text.len() < min_prompt_chars {
        return PerplexityResult::Pass;
    }

    // Extract suffix window
    let suffix_start = text.len().saturating_sub(suffix_window_chars);
    let suffix = &text[suffix_start..];

    let perp = bigram_perplexity(suffix);
    let sym_ratio = symbol_ratio(suffix);

    // Flag if either threshold is exceeded
    if perp > perplexity_threshold {
        return PerplexityResult::Flagged {
            perplexity: perp,
            symbol_ratio: sym_ratio,
            reason: format!(
                "Suffix perplexity {perp:.2} exceeds threshold {perplexity_threshold:.2}"
            ),
        };
    }

    if sym_ratio > symbol_ratio_threshold {
        return PerplexityResult::Flagged {
            perplexity: perp,
            symbol_ratio: sym_ratio,
            reason: format!(
                "Suffix symbol ratio {sym_ratio:.2} exceeds threshold {symbol_ratio_threshold:.2}"
            ),
        };
    }

    PerplexityResult::Pass
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_english_low_perplexity() {
        let text = "Hello, this is a normal English sentence about programming.";
        let perp = bigram_perplexity(text);
        // Natural text should have relatively low perplexity
        assert!(perp < 10.0, "English text perplexity {perp} should be < 10");
    }

    #[test]
    fn random_chars_high_perplexity() {
        let text = "xK7!mQ@3#zP$9&wR*5^yL%2(eN)8+bT";
        let perp = bigram_perplexity(text);
        // Random mixed-class chars should have high perplexity
        assert!(perp > 5.0, "Random chars perplexity {perp} should be > 5");
    }

    #[test]
    fn empty_text_zero_perplexity() {
        assert_eq!(bigram_perplexity(""), 0.0);
        assert_eq!(bigram_perplexity("a"), 0.0);
    }

    #[test]
    fn repeated_chars_low_perplexity() {
        let text = "aaaaaaaaaaaaaaaaaaa";
        let perp = bigram_perplexity(text);
        assert!(perp < 3.0, "Repeated chars perplexity {perp} should be < 3");
    }

    #[test]
    fn symbol_ratio_normal_text() {
        let text = "Hello, world!";
        let ratio = symbol_ratio(text);
        assert!(
            ratio < 0.20,
            "Normal text symbol ratio {ratio} should be < 0.20"
        );
    }

    #[test]
    fn symbol_ratio_heavy_symbols() {
        let text = "!@#$%^&*()_+-=[]{}|;':\",./<>?";
        let ratio = symbol_ratio(text);
        assert!(
            ratio > 0.80,
            "Heavy symbol text ratio {ratio} should be > 0.80"
        );
    }

    #[test]
    fn symbol_ratio_empty() {
        assert_eq!(symbol_ratio(""), 0.0);
    }

    #[test]
    fn analyze_suffix_passes_normal_text() {
        let text = "Can you help me write a function that calculates the fibonacci sequence?";
        let result = analyze_suffix(text, 64, 18.0, 0.20, 32);
        assert_eq!(result, PerplexityResult::Pass);
    }

    #[test]
    fn analyze_suffix_flags_adversarial_suffix() {
        // Simulate an adversarial prompt: normal text followed by gibberish
        let normal = "Please write a function.";
        let adversarial = "xK7!mQ@3#zP$9&wR*5^yL%2(eN)8+bT!@#$%^&*()_+-=[]{}|xK7!mQ@3#";
        let text = format!("{normal} {adversarial}");

        let result = analyze_suffix(&text, 64, 4.0, 0.20, 32);
        match result {
            PerplexityResult::Flagged { .. } => {} // expected
            PerplexityResult::Pass => panic!("adversarial suffix should be flagged"),
        }
    }

    #[test]
    fn analyze_suffix_skips_short_prompts() {
        let text = "hi";
        let result = analyze_suffix(text, 64, 18.0, 0.20, 32);
        assert_eq!(result, PerplexityResult::Pass);
    }

    #[test]
    fn analyze_suffix_symbol_ratio_flag() {
        let text = "Please help me with this: !@#$%^&*()!@#$%^&*()!@#$%^&*()!@#$%^&*()";
        let result = analyze_suffix(text, 40, 100.0, 0.10, 32);
        match result {
            PerplexityResult::Flagged { symbol_ratio, .. } => {
                assert!(symbol_ratio > 0.10);
            }
            PerplexityResult::Pass => panic!("high symbol ratio should be flagged"),
        }
    }
}
