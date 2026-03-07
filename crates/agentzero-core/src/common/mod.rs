pub mod local_providers;
pub mod paths;
pub mod privacy_helpers;
pub mod url_policy;
pub mod util;

use std::collections::HashMap;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Log output format.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum LogFormat {
    /// Human-readable text output (default).
    #[default]
    Text,
    /// Structured JSON — one JSON object per line.
    Json,
}

/// Options for initialising the tracing subscriber.
#[derive(Debug, Clone, Default)]
pub struct TracingOptions {
    /// Output format (text or json).
    pub format: LogFormat,
    /// Base log level (overridden by `RUST_LOG` env var).
    pub level: Option<String>,
    /// Per-module log level overrides.
    pub modules: HashMap<String, String>,
}

/// Initialise tracing from CLI verbosity flag (backward-compatible).
///
/// Also checks `AGENTZERO__LOGGING__FORMAT` env var (values: "text", "json")
/// and `AGENTZERO__LOGGING__LEVEL` env var so container deployments can set
/// format via environment without a config file.
pub fn init_tracing(verbosity: u8) {
    let format = match std::env::var("AGENTZERO__LOGGING__FORMAT")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "json" => LogFormat::Json,
        _ => LogFormat::Text,
    };

    let level = std::env::var("AGENTZERO__LOGGING__LEVEL")
        .ok()
        .unwrap_or_else(|| verbosity_to_level(verbosity).to_string());

    init_tracing_with_options(&TracingOptions {
        format,
        level: Some(level),
        ..Default::default()
    });
}

/// Initialise tracing with full options (format, level, per-module overrides).
pub fn init_tracing_with_options(opts: &TracingOptions) {
    let base_level = opts.level.as_deref().unwrap_or("error");
    let filter = build_env_filter(base_level, &opts.modules);

    let registry = tracing_subscriber::registry().with(filter);

    match opts.format {
        LogFormat::Json => {
            registry
                .with(tracing_subscriber::fmt::layer().json().with_target(true))
                .try_init()
                .ok();
        }
        LogFormat::Text => {
            registry
                .with(tracing_subscriber::fmt::layer().with_target(false))
                .try_init()
                .ok();
        }
    }
}

fn build_env_filter(base_level: &str, modules: &HashMap<String, String>) -> EnvFilter {
    // RUST_LOG takes precedence when set.
    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        return EnvFilter::new(rust_log);
    }

    let mut directives = base_level.to_string();
    for (module, level) in modules {
        directives.push_str(&format!(",{module}={level}"));
    }
    EnvFilter::new(directives)
}

fn verbosity_to_level(verbosity: u8) -> &'static str {
    match verbosity {
        0 | 1 => "error",
        2 => "info",
        3 => "debug",
        _ => "trace",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbosity_level_one_maps_to_error() {
        assert_eq!(verbosity_to_level(1), "error");
    }

    #[test]
    fn verbosity_level_two_maps_to_info() {
        assert_eq!(verbosity_to_level(2), "info");
    }

    #[test]
    fn verbosity_level_three_maps_to_debug() {
        assert_eq!(verbosity_to_level(3), "debug");
    }

    #[test]
    fn verbosity_level_four_or_more_maps_to_trace() {
        assert_eq!(verbosity_to_level(4), "trace");
        assert_eq!(verbosity_to_level(8), "trace");
    }

    #[test]
    fn log_format_default_is_text() {
        assert_eq!(LogFormat::default(), LogFormat::Text);
    }

    #[test]
    fn tracing_options_default_uses_text_format() {
        let opts = TracingOptions::default();
        assert_eq!(opts.format, LogFormat::Text);
        assert!(opts.level.is_none());
        assert!(opts.modules.is_empty());
    }

    #[test]
    fn build_env_filter_with_module_overrides() {
        let mut modules = HashMap::new();
        modules.insert("agentzero_gateway".to_string(), "debug".to_string());
        // Should not panic — just verifies the directive string is valid.
        let _ = build_env_filter("info", &modules);
    }
}
