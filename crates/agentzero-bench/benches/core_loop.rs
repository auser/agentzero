use criterion::{criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;

fn bench_core_loop(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime should be created for benchmarks");

    c.bench_function("core_loop_single_turn", |b| {
        b.to_async(&rt).iter(|| async {
            let response = agentzero_bench::run_core_loop_iteration("hello benchmark")
                .await
                .expect("benchmark iteration should succeed");
            criterion::black_box(response);
        });
    });

    c.bench_function("core_loop_tool_turn", |b| {
        b.to_async(&rt).iter(|| async {
            let response = agentzero_bench::run_core_loop_iteration("tool:echo ping")
                .await
                .expect("tool benchmark iteration should succeed");
            criterion::black_box(response);
        });
    });
}

criterion_group!(benches, bench_core_loop);
criterion_main!(benches);
