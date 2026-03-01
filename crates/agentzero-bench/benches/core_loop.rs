use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_core_loop_single_turn(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new()
        .expect("tokio runtime should be created for criterion bench");
    c.bench_function("core_loop_single_turn", |b| {
        b.to_async(&runtime).iter(|| async {
            let response = agentzero_bench::run_core_loop_iteration(black_box("hello benchmark"))
                .await
                .expect("single-turn benchmark iteration should succeed");
            black_box(response);
        });
    });
}

fn bench_core_loop_tool_turn(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new()
        .expect("tokio runtime should be created for criterion bench");
    c.bench_function("core_loop_tool_turn", |b| {
        b.to_async(&runtime).iter(|| async {
            let response = agentzero_bench::run_core_loop_iteration(black_box("tool:echo ping"))
                .await
                .expect("tool benchmark iteration should succeed");
            black_box(response);
        });
    });
}

criterion_group!(
    core_loop,
    bench_core_loop_single_turn,
    bench_core_loop_tool_turn
);
criterion_main!(core_loop);
