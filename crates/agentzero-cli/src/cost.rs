use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CostSummary {
    pub total_tokens: u64,
    pub total_usd: f64,
}

impl Default for CostSummary {
    fn default() -> Self {
        Self {
            total_tokens: 0,
            total_usd: 0.0,
        }
    }
}

impl CostSummary {
    pub fn record(&mut self, tokens: u64, usd: f64) {
        self.total_tokens += tokens;
        self.total_usd += usd;
    }
}

#[cfg(test)]
mod tests {
    use super::CostSummary;

    #[test]
    fn record_accumulates_cost_success_path() {
        let mut summary = CostSummary::default();
        summary.record(100, 0.02);
        summary.record(50, 0.01);
        assert_eq!(summary.total_tokens, 150);
        assert!((summary.total_usd - 0.03).abs() < 1e-12);
    }

    #[test]
    fn default_is_zero_negative_path() {
        let summary = CostSummary::default();
        assert_eq!(summary.total_tokens, 0);
        assert_eq!(summary.total_usd, 0.0);
    }

    #[test]
    fn cost_summary_default_zero() {
        let summary = CostSummary::default();
        assert_eq!(summary.total_tokens, 0);
        assert!((summary.total_usd - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_summary_record_accumulates() {
        let mut summary = CostSummary::default();
        summary.record(100, 0.01);
        summary.record(100, 0.01);
        assert_eq!(summary.total_tokens, 200);
        assert!((summary.total_usd - 0.02).abs() < 1e-12);
    }

    #[test]
    fn cost_summary_serialization_roundtrip() {
        let mut summary = CostSummary::default();
        summary.record(500, 0.05);

        let json = serde_json::to_string(&summary).unwrap();
        let parsed: CostSummary = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.total_tokens, summary.total_tokens);
        assert!((parsed.total_usd - summary.total_usd).abs() < 1e-12);
    }
}
