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
    creates_during_measurement: usize,
    local_waits: usize,
    shared_fallbacks: usize,
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
        let (handle_samples, handle_waits, handle_fallbacks) = handle.await.unwrap();
        samples.extend(handle_samples);
        local_waits += handle_waits;
        shared_fallbacks += handle_fallbacks;
    }
    let elapsed = started.elapsed();
    let throughput_per_second = samples.len() as f64 / elapsed.as_secs_f64();

    let creates_during_measurement = creates.load(Ordering::Relaxed) - creates_after_warmup;
    assert_eq!(
        creates_during_measurement, 0,
        "same-handle local hot path created additional objects"
    );

    Measurement {
        p99: p99(samples),
        throughput_per_second,
        creates_after_warmup,
        creates_during_measurement,
        local_waits,
        shared_fallbacks: shared_fallbacks - fallback_after_warmup,
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
        creates_during_measurement: creates.load(Ordering::Relaxed) - creates_after_warmup,
        local_waits: 0,
        shared_fallbacks: 0,
    }
}

async fn capacity_pressure_reuses_returned_local_object() {
    let manager = Manager::default();
    let creates = manager.creates.clone();
    let pool: managed::Pool<Manager> = managed::Pool::builder(manager)
        .max_size(1)
        .pool_mode(PoolMode::CoreLocal)
        .build()
        .unwrap();
    let local = pool.local();
    let checked_out = local.get().await.unwrap();
    let waiter = {
        let local = local.clone();
        tokio::spawn(async move {
            let object = local.get().await.unwrap();
            (local.local_wait_count(), object)
        })
    };

    for _ in 0..100 {
        if local.local_wait_count() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(local.local_wait_count() > 0);
    drop(checked_out);
    let (waits, object) = waiter.await.unwrap();
    drop(object);

    assert!(waits > 0);
    assert_eq!(creates.load(Ordering::Relaxed), 1);
    assert_eq!(pool.status().size, 1);
    assert_eq!(pool.status().available, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 32)]
#[ignore = "stress gate for opt-in core-local managed pool mode"]
async fn managed_core_local_stress_gate() {
    capacity_pressure_reuses_returned_local_object().await;
    for workers in [1, 8, 16, 32] {
        let shared = measure_shared(workers, 512).await;
        let local = measure_local(workers, 512).await;
        assert_eq!(local.creates_after_warmup, workers);
        assert_eq!(local.creates_during_measurement, 0);
        assert_eq!(local.local_waits, 0);
        assert_eq!(local.shared_fallbacks, 0);
        assert!(local.throughput_per_second.is_finite() && local.throughput_per_second > 0.0);
        assert!(local.p99 <= shared.p99);
        assert!(
            local.throughput_per_second >= shared.throughput_per_second,
            "local throughput {} was below shared throughput {} tolerance",
            local.throughput_per_second,
            shared.throughput_per_second
        );
        eprintln!("workers={workers} shared={shared:?} local={local:?}");
    }
}
