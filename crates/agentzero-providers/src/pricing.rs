//! Model pricing lookup for cost estimation.
//!
//! Maps (provider, model) pairs to input/output token prices in microdollars
//! (1 microdollar = $0.000001). Uses integer arithmetic throughout to avoid
//! floating-point rounding issues.

use crate::find_provider;

/// Pricing rates for a model, in microdollars per 1 million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelPricing {
    /// Cost per 1M input tokens, in microdollars.
    pub input_per_mtok: u64,
    /// Cost per 1M output tokens, in microdollars.
    pub output_per_mtok: u64,
}

struct PricingEntry {
    prefix: &'static str,
    pricing: ModelPricing,
}

// Anthropic pricing (microdollars per 1M tokens).
const ANTHROPIC_PRICING: &[PricingEntry] = &[
    PricingEntry {
        prefix: "claude-opus-4",
        pricing: ModelPricing {
            input_per_mtok: 15_000_000,
            output_per_mtok: 75_000_000,
        },
    },
    PricingEntry {
        prefix: "claude-sonnet-4",
        pricing: ModelPricing {
            input_per_mtok: 3_000_000,
            output_per_mtok: 15_000_000,
        },
    },
    PricingEntry {
        prefix: "claude-haiku-4",
        pricing: ModelPricing {
            input_per_mtok: 800_000,
            output_per_mtok: 4_000_000,
        },
    },
    PricingEntry {
        prefix: "claude-3.5-sonnet",
        pricing: ModelPricing {
            input_per_mtok: 3_000_000,
            output_per_mtok: 15_000_000,
        },
    },
    PricingEntry {
        prefix: "claude-3-haiku",
        pricing: ModelPricing {
            input_per_mtok: 250_000,
            output_per_mtok: 1_250_000,
        },
    },
];

// OpenAI pricing (more specific prefixes first).
const OPENAI_PRICING: &[PricingEntry] = &[
    PricingEntry {
        prefix: "gpt-4.1-nano",
        pricing: ModelPricing {
            input_per_mtok: 100_000,
            output_per_mtok: 400_000,
        },
    },
    PricingEntry {
        prefix: "gpt-4.1-mini",
        pricing: ModelPricing {
            input_per_mtok: 400_000,
            output_per_mtok: 1_600_000,
        },
    },
    PricingEntry {
        prefix: "gpt-4.1",
        pricing: ModelPricing {
            input_per_mtok: 2_000_000,
            output_per_mtok: 8_000_000,
        },
    },
    PricingEntry {
        prefix: "gpt-4o-mini",
        pricing: ModelPricing {
            input_per_mtok: 150_000,
            output_per_mtok: 600_000,
        },
    },
    PricingEntry {
        prefix: "gpt-4o",
        pricing: ModelPricing {
            input_per_mtok: 2_500_000,
            output_per_mtok: 10_000_000,
        },
    },
    PricingEntry {
        prefix: "o3-mini",
        pricing: ModelPricing {
            input_per_mtok: 1_100_000,
            output_per_mtok: 4_400_000,
        },
    },
    PricingEntry {
        prefix: "o3",
        pricing: ModelPricing {
            input_per_mtok: 2_000_000,
            output_per_mtok: 8_000_000,
        },
    },
    PricingEntry {
        prefix: "o4-mini",
        pricing: ModelPricing {
            input_per_mtok: 1_100_000,
            output_per_mtok: 4_400_000,
        },
    },
];

// Google Gemini pricing.
const GEMINI_PRICING: &[PricingEntry] = &[
    PricingEntry {
        prefix: "gemini-2.0-flash",
        pricing: ModelPricing {
            input_per_mtok: 100_000,
            output_per_mtok: 400_000,
        },
    },
    PricingEntry {
        prefix: "gemini-1.5-flash",
        pricing: ModelPricing {
            input_per_mtok: 75_000,
            output_per_mtok: 300_000,
        },
    },
    PricingEntry {
        prefix: "gemini-1.5-pro",
        pricing: ModelPricing {
            input_per_mtok: 1_250_000,
            output_per_mtok: 5_000_000,
        },
    },
];

// OpenRouter pricing — uses underlying model prefixes.
const OPENROUTER_PRICING: &[PricingEntry] = &[
    PricingEntry {
        prefix: "anthropic/claude-3.5-sonnet",
        pricing: ModelPricing {
            input_per_mtok: 3_000_000,
            output_per_mtok: 15_000_000,
        },
    },
    PricingEntry {
        prefix: "openai/gpt-4o-mini",
        pricing: ModelPricing {
            input_per_mtok: 150_000,
            output_per_mtok: 600_000,
        },
    },
    PricingEntry {
        prefix: "google/gemini-1.5-pro",
        pricing: ModelPricing {
            input_per_mtok: 1_250_000,
            output_per_mtok: 5_000_000,
        },
    },
];

fn find_pricing(entries: &[PricingEntry], model: &str) -> Option<ModelPricing> {
    // More specific prefixes come first in the arrays, so first match wins.
    entries
        .iter()
        .find(|e| model.starts_with(e.prefix))
        .map(|e| e.pricing)
}

/// Look up pricing for a specific model on a specific provider.
///
/// Returns `None` if the provider is unknown, the model is not in the pricing
/// table, or the provider is local (ollama, llamacpp, builtin, etc.) since
/// those have no API cost.
pub fn model_pricing(provider: &str, model: &str) -> Option<ModelPricing> {
    let resolved = find_provider(provider);
    let provider_id = resolved.map(|p| p.id).unwrap_or(provider);

    let entries = match provider_id {
        "anthropic" => ANTHROPIC_PRICING,
        "openai" | "openai-codex" | "copilot" => OPENAI_PRICING,
        "gemini" => GEMINI_PRICING,
        "openrouter" => OPENROUTER_PRICING,
        // Local providers have no API cost.
        "ollama" | "llamacpp" | "lmstudio" | "vllm" | "sglang" | "osaurus" | "builtin" => {
            return None
        }
        _ => return None,
    };

    find_pricing(entries, model)
}

/// Compute the cost in microdollars for a single LLM call.
///
/// Uses integer arithmetic: `(tokens * rate_per_mtok) / 1_000_000`.
pub fn compute_cost_microdollars(
    pricing: &ModelPricing,
    input_tokens: u64,
    output_tokens: u64,
) -> u64 {
    let input_cost = input_tokens.saturating_mul(pricing.input_per_mtok) / 1_000_000;
    let output_cost = output_tokens.saturating_mul(pricing.output_per_mtok) / 1_000_000;
    input_cost.saturating_add(output_cost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_sonnet_pricing_found() {
        let p = model_pricing("anthropic", "claude-sonnet-4-20250514").expect("should find");
        assert_eq!(p.input_per_mtok, 3_000_000);
        assert_eq!(p.output_per_mtok, 15_000_000);
    }

    #[test]
    fn anthropic_opus_pricing_found() {
        let p = model_pricing("anthropic", "claude-opus-4-20250514").expect("should find");
        assert_eq!(p.input_per_mtok, 15_000_000);
        assert_eq!(p.output_per_mtok, 75_000_000);
    }

    #[test]
    fn anthropic_haiku_pricing_found() {
        let p = model_pricing("anthropic", "claude-haiku-4-20250414").expect("should find");
        assert_eq!(p.input_per_mtok, 800_000);
    }

    #[test]
    fn openai_gpt4o_mini_pricing_found() {
        let p = model_pricing("openai", "gpt-4o-mini").expect("should find");
        assert_eq!(p.input_per_mtok, 150_000);
        assert_eq!(p.output_per_mtok, 600_000);
    }

    #[test]
    fn openai_gpt41_pricing_found() {
        let p = model_pricing("openai", "gpt-4.1").expect("should find");
        assert_eq!(p.input_per_mtok, 2_000_000);
    }

    #[test]
    fn openai_gpt41_mini_pricing_found() {
        let p = model_pricing("openai", "gpt-4.1-mini").expect("should find");
        assert_eq!(p.input_per_mtok, 400_000);
    }

    #[test]
    fn gemini_flash_pricing_found() {
        let p = model_pricing("gemini", "gemini-2.0-flash-exp").expect("should find");
        assert_eq!(p.input_per_mtok, 100_000);
    }

    #[test]
    fn local_provider_returns_none() {
        assert!(model_pricing("ollama", "llama3.1:8b").is_none());
        assert!(model_pricing("llamacpp", "anything").is_none());
        assert!(model_pricing("builtin", "qwen2.5-coder-3b").is_none());
    }

    #[test]
    fn unknown_provider_returns_none() {
        assert!(model_pricing("unknown", "some-model").is_none());
    }

    #[test]
    fn unknown_model_returns_none() {
        assert!(model_pricing("anthropic", "totally-fake-model").is_none());
    }

    #[test]
    fn compute_cost_basic_arithmetic() {
        let p = ModelPricing {
            input_per_mtok: 3_000_000,   // $3/Mtok
            output_per_mtok: 15_000_000, // $15/Mtok
        };
        // 1000 input + 500 output tokens
        let cost = compute_cost_microdollars(&p, 1_000, 500);
        // input: 1000 * 3_000_000 / 1_000_000 = 3000 microdollars = $0.003
        // output: 500 * 15_000_000 / 1_000_000 = 7500 microdollars = $0.0075
        assert_eq!(cost, 10_500);
    }

    #[test]
    fn compute_cost_zero_tokens() {
        let p = ModelPricing {
            input_per_mtok: 3_000_000,
            output_per_mtok: 15_000_000,
        };
        assert_eq!(compute_cost_microdollars(&p, 0, 0), 0);
    }

    #[test]
    fn compute_cost_large_token_counts_no_overflow() {
        let p = ModelPricing {
            input_per_mtok: 15_000_000,
            output_per_mtok: 75_000_000,
        };
        // 10M tokens — saturating_mul should prevent overflow
        let cost = compute_cost_microdollars(&p, 10_000_000, 10_000_000);
        // input: 10M * 15M / 1M = 150_000_000 microdollars = $150
        // output: 10M * 75M / 1M = 750_000_000 microdollars = $750
        assert_eq!(cost, 900_000_000);
    }

    #[test]
    fn prefix_matching_more_specific_wins() {
        // gpt-4.1-mini should match the mini entry, not the gpt-4.1 entry
        let p = model_pricing("openai", "gpt-4.1-mini-2025-04-14").expect("should find");
        assert_eq!(p.input_per_mtok, 400_000); // mini price, not full gpt-4.1
    }

    #[test]
    fn openrouter_anthropic_model_found() {
        let p = model_pricing("openrouter", "anthropic/claude-3.5-sonnet").expect("should find");
        assert_eq!(p.input_per_mtok, 3_000_000);
    }
}
