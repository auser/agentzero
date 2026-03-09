//! OpenTelemetry initialization and span utilities.
//!
//! Behind the `telemetry` feature flag. Provides `init_telemetry()` that
//! sets up an OTLP exporter with batch span processing, and a guard that
//! flushes pending spans on drop.

use agentzero_config::ObservabilityConfig;
use anyhow::Context;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::SubscriberExt;

/// Guard returned by [`init_telemetry`]. Flushes and shuts down the tracer
/// provider when dropped.
pub struct TelemetryGuard {
    provider: SdkTracerProvider,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Err(e) = self.provider.shutdown() {
            tracing::warn!(error = %e, "failed to shut down telemetry provider");
        }
    }
}

/// Initialize OpenTelemetry tracing with OTLP export.
///
/// Returns `None` if `backend` is not `"otlp"`, so callers can simply hold
/// the guard without conditional logic.
///
/// The returned [`tracing::subscriber::DefaultGuard`] installs the subscriber
/// as the thread-local default. Hold both guards for the application lifetime.
pub fn init_telemetry(
    config: &ObservabilityConfig,
) -> anyhow::Result<Option<(TelemetryGuard, tracing::subscriber::DefaultGuard)>> {
    if config.backend != "otlp" {
        return Ok(None);
    }

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&config.otel_endpoint)
        .build()
        .context("failed to build OTLP span exporter")?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name(config.otel_service_name.clone())
                .build(),
        )
        .build();

    let tracer = provider.tracer("agentzero");
    let otel_layer = OpenTelemetryLayer::new(tracer);

    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(otel_layer);

    let default_guard = tracing::subscriber::set_default(subscriber);

    tracing::info!(
        endpoint = %config.otel_endpoint,
        service_name = %config.otel_service_name,
        "OpenTelemetry OTLP tracing initialized"
    );

    Ok(Some((TelemetryGuard { provider }, default_guard)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_telemetry_none_backend_returns_none() {
        let config = ObservabilityConfig::default(); // backend = "none"
        let result = init_telemetry(&config).expect("should not error");
        assert!(result.is_none());
    }
}
