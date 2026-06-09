use criterion::{Criterion, criterion_group, criterion_main};

#[cfg(feature = "core-local")]
use deadpool::PoolMode;
use deadpool::unmanaged::Pool;

const ITERATIONS: usize = 1_000_000;

#[tokio::main]
async fn use_pool() {
    let pool = Pool::new(16);
    pool.add(()).await.unwrap();
    for _ in 0..ITERATIONS {
        let _ = pool.get().await.unwrap();
    }
}

#[cfg(feature = "core-local")]
#[tokio::main]
async fn use_core_local_pool() {
    let pool = Pool::new_with_mode(16, PoolMode::CoreLocal);
    let local = pool.local();
    local.add(()).await.unwrap();
    for _ in 0..ITERATIONS {
        let _ = local.get().await.unwrap();
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("use_pool", |b| b.iter(use_pool));
    #[cfg(feature = "core-local")]
    c.bench_function("use_core_local_pool", |b| b.iter(use_core_local_pool));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
