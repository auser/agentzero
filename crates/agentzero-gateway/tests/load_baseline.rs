//! Pure-Rust load harness for the AgentZero gateway.
//!
//! This is **not** a unit test — it's a benchmarking harness gated behind
//! `#[ignore]` so `cargo test` ignores it by default. To run:
//!
//! ```bash
//! cargo test --release -p agentzero-gateway --test load_baseline -- --ignored --nocapture
//! ```
//!
//! Configuration via env vars (all optional, defaults shown):
//!   AZ_LOAD_DURATION_SECS=10     — how long each endpoint is hammered
//!   AZ_LOAD_CONCURRENCY=64       — number of concurrent client tasks
//!   AZ_LOAD_PORT=18800           — gateway listen port for the run
//!
//! What it measures, per endpoint:
//!   - total requests issued
//!   - effective requests/second
//!   - error count (any non-2xx response or transport error)
//!   - p50 / p95 / p99 / max latency in milliseconds
//!
//! The gateway is spawned in-process via `agentzero_gateway::run()` with
//! `--no-auth` so we don't need any test fixtures, API keys, or pairing.
//! Rate limiting is disabled in the middleware config so the harness can
//! actually saturate the server.

use agentzero_gateway::{run, GatewayMiddlewareConfig as MiddlewareConfig, GatewayRunOptions};
use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_DURATION_SECS: u64 = 10;
const DEFAULT_CONCURRENCY: usize = 64;
const DEFAULT_PORT: u16 = 18800;

#[derive(Debug, Clone, Copy)]
struct LoadConfig {
    duration: Duration,
    concurrency: usize,
    port: u16,
}

impl LoadConfig {
    fn from_env() -> Self {
        let duration_secs = env::var("AZ_LOAD_DURATION_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_DURATION_SECS);
        let concurrency = env::var("AZ_LOAD_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_CONCURRENCY);
        let port = env::var("AZ_LOAD_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        Self {
            duration: Duration::from_secs(duration_secs),
            concurrency,
            port,
        }
    }
}

#[derive(Debug, Default)]
struct EndpointStats {
    name: String,
    total_requests: u64,
    errors: u64,
    elapsed: Duration,
    /// Latencies in microseconds — collected as raw samples, sorted at report
    /// time. We use a Mutex<Vec> rather than a histogram crate so we don't add
    /// a workspace dependency for one bench.
    latencies_us: Vec<u64>,
}

impl EndpointStats {
    fn rps(&self) -> f64 {
        if self.elapsed.as_secs_f64() == 0.0 {
            0.0
        } else {
            self.total_requests as f64 / self.elapsed.as_secs_f64()
        }
    }

    /// Latency at percentile `p` in [0.0, 1.0], in milliseconds. Caller must
    /// have sorted `latencies_us` first.
    fn percentile_ms(&self, p: f64) -> f64 {
        if self.latencies_us.is_empty() {
            return 0.0;
        }
        let idx = ((self.latencies_us.len() as f64 - 1.0) * p).round() as usize;
        self.latencies_us[idx] as f64 / 1000.0
    }

    fn report(&mut self) -> String {
        self.latencies_us.sort_unstable();
        let p50 = self.percentile_ms(0.50);
        let p95 = self.percentile_ms(0.95);
        let p99 = self.percentile_ms(0.99);
        let max = self
            .latencies_us
            .last()
            .copied()
            .map(|v| v as f64 / 1000.0)
            .unwrap_or(0.0);
        format!(
            "  {:<24}  reqs={:>7}  errors={:>4}  rps={:>10.1}  p50={:>7.2}ms  p95={:>7.2}ms  p99={:>7.2}ms  max={:>7.2}ms",
            self.name,
            self.total_requests,
            self.errors,
            self.rps(),
            p50,
            p95,
            p99,
            max,
        )
    }
}

/// Spin a single endpoint at full configured concurrency for `cfg.duration`,
/// then return aggregated stats.
async fn hammer(client: &reqwest::Client, name: &str, url: &str, cfg: LoadConfig) -> EndpointStats {
    let stop_at = Instant::now() + cfg.duration;
    let total = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let latencies = Arc::new(std::sync::Mutex::new(Vec::with_capacity(
        cfg.concurrency * 1000,
    )));

    let mut handles = Vec::with_capacity(cfg.concurrency);
    for _ in 0..cfg.concurrency {
        let client = client.clone();
        let url = url.to_string();
        let total = total.clone();
        let errors = errors.clone();
        let latencies = latencies.clone();
        handles.push(tokio::spawn(async move {
            let mut local_latencies: Vec<u64> = Vec::with_capacity(1024);
            while Instant::now() < stop_at {
                let started = Instant::now();
                let result = client.get(&url).send().await;
                let latency_us = started.elapsed().as_micros() as u64;
                match result {
                    Ok(resp) if resp.status().is_success() => {
                        // Drain the body so the connection can be reused.
                        let _ = resp.bytes().await;
                        local_latencies.push(latency_us);
                        total.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) | Err(_) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        total.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            // Merge local latencies into the shared vec at the end of the run
            // so the contention point is hit O(concurrency) times instead of
            // O(total_requests) times.
            if let Ok(mut shared) = latencies.lock() {
                shared.extend(local_latencies);
            }
        }));
    }

    let started = Instant::now();
    for h in handles {
        let _ = h.await;
    }
    let elapsed = started.elapsed();

    let latencies = std::mem::take(&mut *latencies.lock().expect("latencies mutex"));
    EndpointStats {
        name: name.to_string(),
        total_requests: total.load(Ordering::Relaxed),
        errors: errors.load(Ordering::Relaxed),
        elapsed,
        latencies_us: latencies,
    }
}

/// Spawn the gateway in-process with `--no-auth` and unlimited rate limiting.
/// Returns a future that resolves when the gateway has fully started serving.
async fn spawn_gateway(port: u16) {
    let middleware = MiddlewareConfig {
        // Disable rate limiting entirely so the load test can saturate.
        rate_limit_max: 0,
        rate_limit_per_identity: 0,
        rate_limit_window_secs: 60,
        max_body_bytes: 1024 * 1024,
        cors_allowed_origins: vec![],
        tls_enabled: false,
    };

    let options = GatewayRunOptions {
        token_store_path: None,
        new_pairing: false,
        middleware,
        config_path: None,
        workspace_root: None,
        data_dir: None,
        default_privacy_mode: None,
        serve_ui: false,
        no_auth: true,
    };

    tokio::spawn(async move {
        if let Err(e) = run("127.0.0.1", port, options).await {
            eprintln!("gateway failed to run: {e}");
        }
    });

    // Poll the health endpoint until it answers, capped at 5 seconds.
    let client = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(resp) = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
        {
            if resp.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("gateway did not become ready within 5s on port {port}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "load harness — invoke explicitly: cargo test --release -- --ignored load_baseline"]
async fn load_baseline() {
    let cfg = LoadConfig::from_env();
    println!(
        "\n=== AgentZero Gateway Load Baseline ===\n\
         duration:    {:?}\n\
         concurrency: {}\n\
         port:        {}\n",
        cfg.duration, cfg.concurrency, cfg.port
    );

    spawn_gateway(cfg.port).await;
    println!("gateway up at http://127.0.0.1:{}\n", cfg.port);

    // Build a single client with connection pooling so we measure server
    // throughput, not TLS handshake or TCP open cost.
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(cfg.concurrency * 2)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client");

    let endpoints = [
        ("/health/live", "GET /health/live"),
        ("/health", "GET /health"),
        ("/metrics", "GET /metrics"),
    ];

    println!("Hammering each endpoint for {:?}...\n", cfg.duration);
    let mut all_stats = Vec::new();
    for (path, name) in endpoints {
        let url = format!("http://127.0.0.1:{}{path}", cfg.port);
        let stats = hammer(&client, name, &url, cfg).await;
        all_stats.push(stats);
    }

    println!("=== Results ===");
    for stats in &mut all_stats {
        println!("{}", stats.report());
    }
    println!();

    // Sanity assertions: we should have made at least *some* requests against
    // every endpoint, and the cheapest endpoint should not have a high error
    // rate. We deliberately don't assert specific RPS numbers — those are
    // hardware-dependent and the point is the report, not a pass/fail gate.
    for stats in &all_stats {
        assert!(
            stats.total_requests > 0,
            "endpoint {} produced zero requests — gateway may have crashed",
            stats.name
        );
    }

    let live = all_stats
        .iter()
        .find(|s| s.name == "GET /health/live")
        .expect("health/live in results");
    let error_rate = live.errors as f64 / live.total_requests.max(1) as f64;
    assert!(
        error_rate < 0.05,
        "GET /health/live error rate {:.2}% exceeds 5% — gateway is unhealthy under load",
        error_rate * 100.0
    );
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn percentile_handles_empty_input() {
        let stats = EndpointStats::default();
        assert_eq!(stats.percentile_ms(0.5), 0.0);
        assert_eq!(stats.percentile_ms(0.99), 0.0);
    }

    #[test]
    fn percentile_basic_case() {
        let mut stats = EndpointStats {
            name: "test".to_string(),
            total_requests: 100,
            errors: 0,
            elapsed: Duration::from_secs(1),
            latencies_us: (1..=100).map(|i| i * 1000).collect(),
        };
        stats.latencies_us.sort_unstable();
        // p50 of [1..100] is element index 50 → value 51000us → 51ms
        let p50 = stats.percentile_ms(0.50);
        assert!((50.0..=52.0).contains(&p50), "p50 was {p50}");
        // p99 → element index 98 → value 99000us → 99ms
        let p99 = stats.percentile_ms(0.99);
        assert!((98.0..=100.0).contains(&p99), "p99 was {p99}");
    }

    #[test]
    fn rps_zero_when_elapsed_zero() {
        let stats = EndpointStats {
            elapsed: Duration::ZERO,
            total_requests: 100,
            ..Default::default()
        };
        assert_eq!(stats.rps(), 0.0);
    }

    #[test]
    fn rps_basic_case() {
        let stats = EndpointStats {
            elapsed: Duration::from_secs(2),
            total_requests: 1000,
            ..Default::default()
        };
        assert!((stats.rps() - 500.0).abs() < 0.01);
    }
}
