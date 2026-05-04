//! Retry logic for model provider calls.
//!
//! Implements exponential backoff with jitter for transient failures.
//! Only retries on unavailability — never retries denied or policy-blocked calls.

use std::time::Duration;

use agentzero_tracing::{debug, warn};

use crate::provider::ModelProviderError;

/// Retry configuration.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Initial backoff duration.
    pub initial_backoff: Duration,
    /// Maximum backoff duration.
    pub max_backoff: Duration,
    /// Backoff multiplier.
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(10),
            multiplier: 2.0,
        }
    }
}

/// Determine if an error is retryable.
pub fn is_retryable(error: &ModelProviderError) -> bool {
    matches!(error, ModelProviderError::Unavailable(_))
}

/// Calculate backoff duration for a given attempt.
pub fn backoff_duration(config: &RetryConfig, attempt: u32) -> Duration {
    let base = config.initial_backoff.as_millis() as f64;
    let backoff_ms = base * config.multiplier.powi(attempt as i32);
    let capped = backoff_ms.min(config.max_backoff.as_millis() as f64);

    // Add jitter (±25%)
    let jitter_factor = 0.75 + (attempt as f64 * 0.1 % 0.5);
    let final_ms = (capped * jitter_factor) as u64;

    Duration::from_millis(final_ms)
}

/// Execute a future with retry logic.
pub async fn with_retry<F, Fut, T>(
    config: &RetryConfig,
    operation_name: &str,
    mut f: F,
) -> Result<T, ModelProviderError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, ModelProviderError>>,
{
    let mut last_error = ModelProviderError::Unavailable("no attempts made".into());

    for attempt in 0..=config.max_retries {
        match f().await {
            Ok(result) => {
                if attempt > 0 {
                    debug!(
                        operation = operation_name,
                        attempt = attempt,
                        "succeeded after retry"
                    );
                }
                return Ok(result);
            }
            Err(e) => {
                if !is_retryable(&e) || attempt == config.max_retries {
                    if attempt > 0 {
                        warn!(
                            operation = operation_name,
                            attempt = attempt,
                            error = %e,
                            "all retries exhausted"
                        );
                    }
                    return Err(e);
                }

                let backoff = backoff_duration(config, attempt);
                debug!(
                    operation = operation_name,
                    attempt = attempt,
                    backoff_ms = backoff.as_millis(),
                    error = %e,
                    "retrying after backoff"
                );
                tokio::time::sleep(backoff).await;
                last_error = e;
            }
        }
    }

    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_backoff, Duration::from_millis(500));
    }

    #[test]
    fn retryable_errors() {
        assert!(is_retryable(&ModelProviderError::Unavailable(
            "timeout".into()
        )));
        assert!(!is_retryable(&ModelProviderError::Denied("policy".into())));
        assert!(!is_retryable(&ModelProviderError::Failed(
            "bad request".into()
        )));
    }

    #[test]
    fn backoff_increases() {
        let config = RetryConfig::default();
        let d0 = backoff_duration(&config, 0);
        let d1 = backoff_duration(&config, 1);
        let d2 = backoff_duration(&config, 2);
        // Each should be larger than previous (with jitter variance)
        assert!(d1.as_millis() > d0.as_millis() / 2);
        assert!(d2.as_millis() > d1.as_millis() / 2);
    }

    #[test]
    fn backoff_capped() {
        let config = RetryConfig {
            max_backoff: Duration::from_secs(1),
            ..Default::default()
        };
        let d = backoff_duration(&config, 10);
        assert!(d <= Duration::from_millis(1250)); // cap + jitter
    }

    #[tokio::test]
    async fn retry_succeeds_eventually() {
        let config = RetryConfig {
            max_retries: 3,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(10),
            multiplier: 1.0,
        };

        let attempt = std::sync::atomic::AtomicU32::new(0);
        let result = with_retry(&config, "test", || {
            let count = attempt.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                if count < 2 {
                    Err(ModelProviderError::Unavailable("not ready".into()))
                } else {
                    Ok("success")
                }
            }
        })
        .await;

        assert_eq!(result.expect("should succeed"), "success");
        assert_eq!(attempt.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn no_retry_on_denied() {
        let config = RetryConfig::default();
        let attempt = std::sync::atomic::AtomicU32::new(0);

        let result: Result<&str, _> = with_retry(&config, "test", || {
            attempt.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async { Err(ModelProviderError::Denied("policy".into())) }
        })
        .await;

        assert!(result.is_err());
        // Should not retry denied errors
        assert_eq!(attempt.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
