//! Centralized tracing and logging for AgentZero.
//!
//! All crates should use `tracing` macros (info!, warn!, error!, debug!, trace!)
//! for structured logging. This crate handles subscriber initialization and
//! provides the shared configuration.
//!
//! Re-exports `tracing` so dependents don't need a direct dependency.

pub use tracing;
pub use tracing::{debug, error, info, instrument, trace, warn};

use tracing_subscriber::EnvFilter;

/// Initialize the global tracing subscriber.
///
/// Respects `RUST_LOG` for filtering. Defaults to `info` level.
/// Call this once at application startup (typically in main).
pub fn init() {
    init_with_default("info")
}

/// Initialize the global tracing subscriber with a custom default filter.
///
/// The `default` parameter is used when `RUST_LOG` is not set.
/// Examples: `"debug"`, `"agentzero=debug,warn"`, `"trace"`.
pub fn init_with_default(default: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
}

/// Initialize a JSON-formatted subscriber for structured log output.
///
/// Useful for production or when logs are consumed by log aggregators.
pub fn init_json() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_macros_compile() {
        // Just verify the re-exported macros compile — we can't easily test
        // subscriber output without capturing, but the macros must resolve.
        info!("test info message");
        warn!("test warn message");
        debug!("test debug message");
        error!("test error message");
        trace!("test trace message");
    }
}
