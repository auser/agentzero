use std::time::Instant;

const ITERATIONS: usize = 500;

fn main() {
    let runtime = tokio::runtime::Runtime::new()
        .expect("tokio runtime should be created for offline benchmark harness");

    let single_turn_ms = runtime.block_on(async {
        let started = Instant::now();
        for _ in 0..ITERATIONS {
            let response = agentzero_bench::run_core_loop_iteration("hello benchmark")
                .await
                .expect("single-turn benchmark iteration should succeed");
            std::hint::black_box(response);
        }
        started.elapsed().as_secs_f64() * 1000.0
    });

    let tool_turn_ms = runtime.block_on(async {
        let started = Instant::now();
        for _ in 0..ITERATIONS {
            let response = agentzero_bench::run_core_loop_iteration("tool:echo ping")
                .await
                .expect("tool benchmark iteration should succeed");
            std::hint::black_box(response);
        }
        started.elapsed().as_secs_f64() * 1000.0
    });

    println!("agentzero-bench core_loop");
    println!("iterations: {}", ITERATIONS);
    println!("single_turn_total_ms: {:.3}", single_turn_ms);
    println!("tool_turn_total_ms: {:.3}", tool_turn_ms);
}
