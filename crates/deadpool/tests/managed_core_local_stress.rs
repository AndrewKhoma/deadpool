#![cfg(all(feature = "managed", feature = "core-local"))]

use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use deadpool::{
    PoolMode,
    managed::{self, Metrics, RecycleResult},
};

#[derive(Clone, Default)]
struct Manager {
    creates: Arc<AtomicUsize>,
}

impl managed::Manager for Manager {
    type Type = ();
    type Error = Infallible;

    async fn create(&self) -> Result<(), Infallible> {
        self.creates.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn recycle(&self, _: &mut (), _: &Metrics) -> RecycleResult<Infallible> {
        Ok(())
    }
}

#[derive(Debug)]
struct Measurement {
    p99: Duration,
    throughput_per_second: f64,
    creates_after_warmup: usize,
}

fn p99(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() * 99 / 100]
}

async fn measure_local(workers: usize, iterations: usize) -> Measurement {
    let manager = Manager::default();
    let creates = manager.creates.clone();
    let pool: managed::Pool<Manager> = managed::Pool::builder(manager)
        .max_size(workers)
        .pool_mode(PoolMode::CoreLocal)
        .build()
        .unwrap();
    let locals = (0..workers).map(|_| pool.local()).collect::<Vec<_>>();

    for local in &locals {
        drop(local.get().await.unwrap());
    }
    let creates_after_warmup = creates.load(Ordering::Relaxed);

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
            samples
        }));
    }
    let mut samples = Vec::new();
    for handle in handles {
        samples.extend(handle.await.unwrap());
    }
    let elapsed = started.elapsed();
    let throughput_per_second = samples.len() as f64 / elapsed.as_secs_f64();

    assert_eq!(
        creates.load(Ordering::Relaxed),
        creates_after_warmup,
        "same-handle local hot path created additional objects"
    );

    Measurement {
        p99: p99(samples),
        throughput_per_second,
        creates_after_warmup,
    }
}

async fn measure_shared(workers: usize, iterations: usize) -> Measurement {
    let manager = Manager::default();
    let creates = manager.creates.clone();
    let pool: managed::Pool<Manager> = managed::Pool::builder(manager)
        .max_size(workers)
        .build()
        .unwrap();

    for _ in 0..workers {
        drop(pool.get().await.unwrap());
    }
    let creates_after_warmup = creates.load(Ordering::Relaxed);

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
    let throughput_per_second = samples.len() as f64 / elapsed.as_secs_f64();

    Measurement {
        p99: p99(samples),
        throughput_per_second,
        creates_after_warmup,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 32)]
#[ignore = "stress gate for opt-in core-local managed pool mode"]
async fn managed_core_local_stress_gate() {
    for workers in [1, 8, 16, 32] {
        let shared = measure_shared(workers, 512).await;
        let local = measure_local(workers, 512).await;
        assert_eq!(local.creates_after_warmup, workers);
        assert!(local.throughput_per_second.is_finite() && local.throughput_per_second > 0.0);
        assert!(local.p99 <= shared.p99);
        assert!(local.throughput_per_second >= shared.throughput_per_second);
        eprintln!("workers={workers} shared={shared:?} local={local:?}");
    }
}
