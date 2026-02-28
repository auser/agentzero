use crate::types::MetricsSink;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default, Clone)]
pub struct RuntimeMetrics {
    inner: Arc<Mutex<RuntimeMetricsInner>>,
}

#[derive(Debug, Default)]
struct RuntimeMetricsInner {
    counters: HashMap<&'static str, u64>,
    histograms: HashMap<&'static str, HistogramState>,
}

#[derive(Debug, Default, Clone)]
struct HistogramState {
    count: u64,
    sum: f64,
    min: f64,
    max: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistogramSnapshot {
    pub count: u64,
    pub avg: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeMetricsSnapshot {
    pub counters: HashMap<String, u64>,
    pub histograms: HashMap<String, HistogramSnapshot>,
}

impl RuntimeMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> RuntimeMetricsSnapshot {
        let inner = self.inner.lock().expect("metrics lock poisoned");
        let counters = inner
            .counters
            .iter()
            .map(|(k, v)| ((*k).to_string(), *v))
            .collect::<HashMap<_, _>>();

        let histograms = inner
            .histograms
            .iter()
            .map(|(k, state)| {
                let avg = if state.count == 0 {
                    0.0
                } else {
                    state.sum / state.count as f64
                };
                (
                    (*k).to_string(),
                    HistogramSnapshot {
                        count: state.count,
                        avg,
                        min: state.min,
                        max: state.max,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        RuntimeMetricsSnapshot {
            counters,
            histograms,
        }
    }

    pub fn export_json(&self) -> Value {
        let snapshot = self.snapshot();
        let counters = snapshot
            .counters
            .iter()
            .map(|(k, v)| (k.clone(), json!(v)))
            .collect::<Map<_, _>>();
        let histograms = snapshot
            .histograms
            .iter()
            .map(|(k, h)| {
                (
                    k.clone(),
                    json!({
                        "count": h.count,
                        "avg": h.avg,
                        "min": h.min,
                        "max": h.max
                    }),
                )
            })
            .collect::<Map<_, _>>();

        json!({
            "counters": counters,
            "histograms": histograms,
        })
    }
}

impl MetricsSink for RuntimeMetrics {
    fn increment_counter(&self, name: &'static str, value: u64) {
        let mut inner = self.inner.lock().expect("metrics lock poisoned");
        *inner.counters.entry(name).or_insert(0) += value;
    }

    fn observe_histogram(&self, name: &'static str, value: f64) {
        let mut inner = self.inner.lock().expect("metrics lock poisoned");
        let entry = inner.histograms.entry(name).or_default();
        if entry.count == 0 {
            entry.min = value;
            entry.max = value;
        } else {
            entry.min = entry.min.min(value);
            entry.max = entry.max.max(value);
        }
        entry.count += 1;
        entry.sum += value;
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeMetrics;
    use crate::types::MetricsSink;

    #[test]
    fn runtime_metrics_collects_counters_and_histograms() {
        let metrics = RuntimeMetrics::new();
        metrics.increment_counter("requests_total", 1);
        metrics.increment_counter("requests_total", 2);
        metrics.observe_histogram("provider_latency_ms", 10.0);
        metrics.observe_histogram("provider_latency_ms", 30.0);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.counters.get("requests_total").copied(), Some(3));
        let hist = snapshot
            .histograms
            .get("provider_latency_ms")
            .expect("provider histogram should exist");
        assert_eq!(hist.count, 2);
        assert_eq!(hist.min, 10.0);
        assert_eq!(hist.max, 30.0);
        assert_eq!(hist.avg, 20.0);
    }

    #[test]
    fn runtime_metrics_export_handles_empty_histograms() {
        let metrics = RuntimeMetrics::new();
        metrics.increment_counter("tool_errors_total", 0);

        let exported = metrics.export_json();
        assert_eq!(exported["counters"]["tool_errors_total"], 0);
        assert!(exported["histograms"]
            .as_object()
            .expect("histograms should be an object")
            .is_empty());
    }
}
