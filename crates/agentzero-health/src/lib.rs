use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthSeverity {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreshnessAssessment {
    pub severity: HealthSeverity,
    pub message: String,
    pub hint: Option<String>,
}

pub fn assess_freshness(
    component: &str,
    last_seen_epoch_seconds: Option<u64>,
    stale_after_seconds: u64,
    now_epoch_seconds: u64,
) -> FreshnessAssessment {
    let Some(last_seen) = last_seen_epoch_seconds else {
        return FreshnessAssessment {
            severity: HealthSeverity::Warn,
            message: format!("{component} heartbeat is missing"),
            hint: Some(format!(
                "Start {component} runtime and ensure heartbeat writes are enabled."
            )),
        };
    };

    let age = now_epoch_seconds.saturating_sub(last_seen);
    if age > stale_after_seconds {
        return FreshnessAssessment {
            severity: HealthSeverity::Error,
            message: format!(
                "{component} heartbeat is stale (last seen {age}s ago; threshold {stale_after_seconds}s)"
            ),
            hint: Some(format!(
                "Restart {component} and inspect logs for stalled work loop."
            )),
        };
    }

    FreshnessAssessment {
        severity: HealthSeverity::Ok,
        message: format!(
            "{component} heartbeat is fresh (last seen {age}s ago; threshold {stale_after_seconds}s)"
        ),
        hint: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{assess_freshness, HealthSeverity};

    #[test]
    fn assess_freshness_is_ok_when_recent_success_path() {
        let assessment = assess_freshness("daemon", Some(900), 120, 1000);
        assert_eq!(assessment.severity, HealthSeverity::Ok);
        assert!(assessment.message.contains("fresh"));
        assert!(assessment.hint.is_none());
    }

    #[test]
    fn assess_freshness_errors_when_stale_negative_path() {
        let assessment = assess_freshness("daemon", Some(100), 60, 200);
        assert_eq!(assessment.severity, HealthSeverity::Error);
        assert!(assessment.message.contains("stale"));
        assert!(assessment.hint.is_some());
    }
}
