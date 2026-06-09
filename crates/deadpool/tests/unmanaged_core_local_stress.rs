#![cfg(all(feature = "unmanaged", feature = "core-local"))]

use std::time::{Duration, Instant};

use deadpool::{PoolMode, unmanaged::Pool};

#[derive(Debug)]
struct Measurement {
    p99: Duration,
    throughput_per_second: f64,
    local_waits: usize,
    shared_fallbacks: usize,
}

fn p99(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() * 99 / 100]
}

async fn measure_local(workers: usize, iterations: usize) -> Measurement {
    let pool = Pool::new_with_mode(workers, PoolMode::CoreLocal);
    let locals = (0..workers).map(|_| pool.local()).collect::<Vec<_>>();
    for local in &locals {
        local.add(()).await.unwrap();
    }
    let fallback_after_warmup: usize = locals
        .iter()
        .map(|local| local.local_shared_fallback_count())
        .sum();

    let started = Instant::now();
    let mut handles = Vec::with_capacity(workers);
    for local in locals {
        handles.push(tokio::spawn(async move {
            let mut samples = Vec::with_capacity(iterations);
            for _ in 0..iterations {
                let before = Instant::now();
                drop(local.get().await.unwrap());
                samples.push(before.elapsed());
            }
            (
                samples,
                local.local_wait_count(),
                local.local_shared_fallback_count(),
            )
        }));
    }

    let mut samples = Vec::new();
    let mut local_waits = 0;
    let mut shared_fallbacks = 0;
    for handle in handles {
        let (handle_samples, waits, fallbacks) = handle.await.unwrap();
        samples.extend(handle_samples);
        local_waits += waits;
        shared_fallbacks += fallbacks;
    }
    let elapsed = started.elapsed();

    Measurement {
        p99: p99(samples.clone()),
        throughput_per_second: samples.len() as f64 / elapsed.as_secs_f64(),
        local_waits,
        shared_fallbacks: shared_fallbacks - fallback_after_warmup,
    }
}

async fn measure_shared(workers: usize, iterations: usize) -> Measurement {
    let pool = Pool::new(workers);
    for _ in 0..workers {
        pool.add(()).await.unwrap();
    }

    let started = Instant::now();
    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            let mut samples = Vec::with_capacity(iterations);
            for _ in 0..iterations {
                let before = Instant::now();
                drop(pool.get().await.unwrap());
                samples.push(before.elapsed());
            }
            samples
        }));
    }
    let mut samples = Vec::new();
    for handle in handles {
        samples.extend(handle.await.unwrap());
    }
    let elapsed = started.elapsed();

    Measurement {
        p99: p99(samples.clone()),
        throughput_per_second: samples.len() as f64 / elapsed.as_secs_f64(),
        local_waits: 0,
        shared_fallbacks: 0,
    }
}

async fn capacity_pressure_reuses_returned_local_object() {
    let pool = Pool::new_with_mode(1, PoolMode::CoreLocal);
    let local = pool.local();
    local.add(()).await.unwrap();
    let checked_out = local.get().await.unwrap();
    let waiter = {
        let local = local.clone();
        tokio::spawn(async move {
            let object = local.get().await.unwrap();
            (local.local_wait_count(), object)
        })
    };
    wait_until(|| local.local_wait_count() > 0).await;
    drop(checked_out);
    let (waits, object) = waiter.await.unwrap();
    drop(object);

    assert!(waits > 0);
    assert_eq!(pool.status().size, 1);
    assert_eq!(pool.status().available, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 32)]
#[ignore = "stress gate for opt-in core-local unmanaged pool mode"]
async fn unmanaged_core_local_stress_gate() {
    capacity_pressure_reuses_returned_local_object().await;
    for workers in [1, 8, 16, 32] {
        let shared = measure_shared(workers, 512).await;
        let local = measure_local(workers, 512).await;
        assert_eq!(local.local_waits, 0);
        assert_eq!(local.shared_fallbacks, 0);
        assert!(local.throughput_per_second.is_finite() && local.throughput_per_second > 0.0);
        assert!(local.p99 <= shared.p99);
        assert!(local.throughput_per_second >= shared.throughput_per_second);
        eprintln!("workers={workers} shared={shared:?} local={local:?}");
    }
}

async fn wait_until(predicate: impl Fn() -> bool) {
    for _ in 0..100 {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(predicate());
}
