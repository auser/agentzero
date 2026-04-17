//! Persistent daily/monthly cost tracking.
//!
//! Tracks accumulated API costs via `agentzero-storage` encrypted-at-rest
//! persistence. Used to enforce the `[cost]` config limits
//! (`daily_limit_usd`, `monthly_limit_usd`, `warn_at_percent`).

use agentzero_storage::EncryptedJsonStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::warn;

/// Persisted cost usage record.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostUsageRecord {
    /// Daily usage in microdollars, keyed by "YYYY-MM-DD".
    #[serde(default)]
    pub daily: HashMap<String, u64>,
    /// Monthly usage in microdollars, keyed by "YYYY-MM".
    #[serde(default)]
    pub monthly: HashMap<String, u64>,
}

/// Persistent cost tracker backed by `EncryptedJsonStore` (encrypted at rest).
pub struct CostTracker {
    store: EncryptedJsonStore,
    record: CostUsageRecord,
}

fn today_key() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_date_from_epoch(now)
}

fn month_key() -> String {
    let key = today_key();
    // "YYYY-MM-DD" -> "YYYY-MM"
    key[..7].to_string()
}

/// Format a UNIX timestamp into "YYYY-MM-DD".
fn format_date_from_epoch(secs: u64) -> String {
    // Civil date computation from UNIX timestamp (UTC).
    let days = (secs / 86400) as i64;
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era [0, 399]
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

impl CostTracker {
    /// Load the cost tracker from `<data_dir>/cost_usage.json` via encrypted storage.
    ///
    /// Automatically migrates legacy plaintext JSON files to encrypted format.
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir, "cost_usage.json")?;
        let mut record: CostUsageRecord = store.load_or_default()?;
        prune_old_entries(&mut record);
        Ok(Self { store, record })
    }

    /// Record cost (in microdollars) for today and the current month, then persist.
    pub fn record_cost(&mut self, microdollars: u64) -> anyhow::Result<()> {
        if microdollars == 0 {
            return Ok(());
        }
        let day = today_key();
        let month = month_key();

        *self.record.daily.entry(day).or_insert(0) += microdollars;
        *self.record.monthly.entry(month).or_insert(0) += microdollars;

        self.save()
    }

    /// Get today's accumulated cost in microdollars.
    pub fn today_usage(&self) -> u64 {
        let key = today_key();
        self.record.daily.get(&key).copied().unwrap_or(0)
    }

    /// Get the current month's accumulated cost in microdollars.
    pub fn month_usage(&self) -> u64 {
        let key = month_key();
        self.record.monthly.get(&key).copied().unwrap_or(0)
    }

    /// Check if daily or monthly limits have been exceeded.
    /// Returns an error message if exceeded, `None` otherwise.
    pub fn check_limits(&self, config: &agentzero_config::CostConfig) -> Option<String> {
        if !config.enabled {
            return None;
        }
        let daily = self.today_usage();
        let daily_limit = usd_to_microdollars(config.daily_limit_usd);
        if daily_limit > 0 && daily >= daily_limit {
            return Some(format!(
                "daily cost limit exceeded: ${:.4} >= ${:.2}",
                daily as f64 / 1_000_000.0,
                config.daily_limit_usd,
            ));
        }
        let monthly = self.month_usage();
        let monthly_limit = usd_to_microdollars(config.monthly_limit_usd);
        if monthly_limit > 0 && monthly >= monthly_limit {
            return Some(format!(
                "monthly cost limit exceeded: ${:.4} >= ${:.2}",
                monthly as f64 / 1_000_000.0,
                config.monthly_limit_usd,
            ));
        }
        None
    }

    /// Check if usage is approaching limits (at or above `warn_at_percent`).
    /// Returns a warning message if approaching, `None` otherwise.
    pub fn check_warnings(&self, config: &agentzero_config::CostConfig) -> Option<String> {
        if !config.enabled || config.warn_at_percent == 0 {
            return None;
        }
        let threshold = config.warn_at_percent as u64;

        let daily = self.today_usage();
        let daily_limit = usd_to_microdollars(config.daily_limit_usd);
        if let Some(pct) = daily
            .checked_mul(100)
            .and_then(|value| value.checked_div(daily_limit))
        {
            if pct >= threshold {
                return Some(format!(
                    "daily cost at {}% of limit (${:.4} / ${:.2})",
                    pct,
                    daily as f64 / 1_000_000.0,
                    config.daily_limit_usd,
                ));
            }
        }

        let monthly = self.month_usage();
        let monthly_limit = usd_to_microdollars(config.monthly_limit_usd);
        if let Some(pct) = monthly
            .checked_mul(100)
            .and_then(|value| value.checked_div(monthly_limit))
        {
            if pct >= threshold {
                return Some(format!(
                    "monthly cost at {}% of limit (${:.4} / ${:.2})",
                    pct,
                    monthly as f64 / 1_000_000.0,
                    config.monthly_limit_usd,
                ));
            }
        }

        None
    }

    fn save(&self) -> anyhow::Result<()> {
        self.store.save(&self.record)
    }
}

fn usd_to_microdollars(usd: f64) -> u64 {
    (usd * 1_000_000.0) as u64
}

/// Remove entries older than 90 days to prevent unbounded file growth.
fn prune_old_entries(record: &mut CostUsageRecord) {
    let cutoff = today_key();
    // Parse cutoff into a simple comparable string — since our keys are
    // "YYYY-MM-DD" format, lexicographic comparison works for dates.
    let cutoff_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff_days = cutoff_secs / 86400;
    let cutoff_date = format_date_from_epoch(cutoff_days.saturating_sub(90) * 86400);

    let before = record.daily.len();
    record
        .daily
        .retain(|k, _| k.as_str() >= cutoff_date.as_str());
    let pruned = before - record.daily.len();
    if pruned > 0 {
        warn!(pruned, "pruned old daily cost entries");
    }

    // Keep monthly entries for at most 12 months.
    let month_cutoff = {
        let today = &cutoff;
        let year: i32 = today[..4].parse().unwrap_or(2026);
        let month: i32 = today[5..7].parse().unwrap_or(1);
        let (y, m) = if month > 12 {
            (year, month - 12)
        } else {
            (year - 1, month)
        };
        format!("{:04}-{:02}", y, m)
    };
    record
        .monthly
        .retain(|k, _| k.as_str() >= month_cutoff.as_str());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path =
            std::env::temp_dir().join(format!("agentzero_cost_test_{pid}_{nanos}_{counter}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn format_date_known_epoch() {
        assert_eq!(format_date_from_epoch(0), "1970-01-01");
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(format_date_from_epoch(1_704_067_200), "2024-01-01");
        // 2026-03-12 = 1_773_273_600
        assert_eq!(format_date_from_epoch(1_773_273_600), "2026-03-12");
    }

    #[test]
    fn load_creates_fresh_on_missing_file() {
        let dir = temp_dir();
        let tracker = CostTracker::load(&dir).unwrap();
        assert_eq!(tracker.today_usage(), 0);
        assert_eq!(tracker.month_usage(), 0);
    }

    #[test]
    fn record_and_query_cost() {
        let dir = temp_dir();
        let mut tracker = CostTracker::load(&dir).unwrap();
        tracker.record_cost(5_000_000).unwrap(); // $5
        assert_eq!(tracker.today_usage(), 5_000_000);
        assert_eq!(tracker.month_usage(), 5_000_000);

        tracker.record_cost(3_000_000).unwrap(); // $3 more
        assert_eq!(tracker.today_usage(), 8_000_000);
        assert_eq!(tracker.month_usage(), 8_000_000);
    }

    #[test]
    fn persists_across_loads() {
        let dir = temp_dir();
        {
            let mut tracker = CostTracker::load(&dir).unwrap();
            tracker.record_cost(1_000_000).unwrap();
        }
        let tracker = CostTracker::load(&dir).unwrap();
        assert_eq!(tracker.today_usage(), 1_000_000);
    }

    #[test]
    fn check_limits_daily() {
        let dir = temp_dir();
        let mut tracker = CostTracker::load(&dir).unwrap();
        tracker.record_cost(11_000_000).unwrap(); // $11

        let config = agentzero_config::CostConfig {
            enabled: true,
            daily_limit_usd: 10.0,
            monthly_limit_usd: 100.0,
            warn_at_percent: 80,
            allow_override: false,
        };
        let result = tracker.check_limits(&config);
        assert!(result.is_some());
        assert!(result.unwrap().contains("daily cost limit exceeded"));
    }

    #[test]
    fn check_limits_monthly() {
        let dir = temp_dir();
        let mut tracker = CostTracker::load(&dir).unwrap();
        tracker.record_cost(101_000_000).unwrap(); // $101

        let config = agentzero_config::CostConfig {
            enabled: true,
            daily_limit_usd: 200.0, // high daily limit
            monthly_limit_usd: 100.0,
            warn_at_percent: 80,
            allow_override: false,
        };
        let result = tracker.check_limits(&config);
        assert!(result.is_some());
        assert!(result.unwrap().contains("monthly cost limit exceeded"));
    }

    #[test]
    fn check_limits_not_exceeded() {
        let dir = temp_dir();
        let mut tracker = CostTracker::load(&dir).unwrap();
        tracker.record_cost(5_000_000).unwrap(); // $5

        let config = agentzero_config::CostConfig {
            enabled: true,
            daily_limit_usd: 10.0,
            monthly_limit_usd: 100.0,
            warn_at_percent: 80,
            allow_override: false,
        };
        assert!(tracker.check_limits(&config).is_none());
    }

    #[test]
    fn check_limits_disabled() {
        let dir = temp_dir();
        let mut tracker = CostTracker::load(&dir).unwrap();
        tracker.record_cost(100_000_000).unwrap(); // $100

        let config = agentzero_config::CostConfig {
            enabled: false,
            daily_limit_usd: 10.0,
            monthly_limit_usd: 50.0,
            warn_at_percent: 80,
            allow_override: false,
        };
        assert!(tracker.check_limits(&config).is_none());
    }

    #[test]
    fn check_warnings_fires_at_threshold() {
        let dir = temp_dir();
        let mut tracker = CostTracker::load(&dir).unwrap();
        tracker.record_cost(8_500_000).unwrap(); // $8.50 = 85% of $10

        let config = agentzero_config::CostConfig {
            enabled: true,
            daily_limit_usd: 10.0,
            monthly_limit_usd: 100.0,
            warn_at_percent: 80,
            allow_override: false,
        };
        let result = tracker.check_warnings(&config);
        assert!(result.is_some());
        assert!(result.unwrap().contains("daily cost at"));
    }

    #[test]
    fn check_warnings_silent_below_threshold() {
        let dir = temp_dir();
        let mut tracker = CostTracker::load(&dir).unwrap();
        tracker.record_cost(5_000_000).unwrap(); // $5 = 50% of $10

        let config = agentzero_config::CostConfig {
            enabled: true,
            daily_limit_usd: 10.0,
            monthly_limit_usd: 100.0,
            warn_at_percent: 80,
            allow_override: false,
        };
        assert!(tracker.check_warnings(&config).is_none());
    }

    #[test]
    fn record_cost_zero_is_noop() {
        let dir = temp_dir();
        let mut tracker = CostTracker::load(&dir).unwrap();
        tracker.record_cost(0).unwrap();
        assert_eq!(tracker.today_usage(), 0);
        // File should not have been created for zero cost.
        assert!(!dir.join("cost_usage.json").exists());
    }

    #[test]
    fn prune_removes_old_daily_entries() {
        let mut record = CostUsageRecord::default();
        record.daily.insert("2020-01-01".to_string(), 1000);
        record.daily.insert("2020-06-15".to_string(), 2000);
        record.daily.insert(today_key(), 3000);

        prune_old_entries(&mut record);
        // Old entries should be removed, today's should remain.
        assert!(!record.daily.contains_key("2020-01-01"));
        assert!(!record.daily.contains_key("2020-06-15"));
        assert!(record.daily.contains_key(&today_key()));
    }
}
