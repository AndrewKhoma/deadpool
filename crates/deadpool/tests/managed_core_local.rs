#![cfg(all(feature = "managed", feature = "core-local"))]

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use deadpool::{
    PoolMode,
    managed::{self, Hook, Metrics, Object, PoolError, RecycleError, RecycleResult, Timeouts},
};

type Pool = managed::Pool<TestManager>;

#[derive(Clone, Debug, Default)]
struct TestManager {
    state: Arc<State>,
}

#[derive(Debug, Default)]
struct State {
    next_id: AtomicUsize,
    creates: AtomicUsize,
    recycles: AtomicUsize,
    detaches: AtomicUsize,
    fail_create: AtomicBool,
    fail_recycle: AtomicBool,
}

impl managed::Manager for TestManager {
    type Type = usize;
    type Error = &'static str;

    async fn create(&self) -> Result<usize, &'static str> {
        if self.state.fail_create.load(Ordering::Relaxed) {
            return Err("create failed");
        }
        self.state.creates.fetch_add(1, Ordering::Relaxed);
        Ok(self.state.next_id.fetch_add(1, Ordering::Relaxed))
    }

    async fn recycle(&self, _: &mut usize, _: &Metrics) -> RecycleResult<&'static str> {
        self.state.recycles.fetch_add(1, Ordering::Relaxed);
        if self.state.fail_recycle.load(Ordering::Relaxed) {
            Err(RecycleError::Backend("recycle failed"))
        } else {
            Ok(())
        }
    }

    fn detach(&self, _: &mut usize) {
        self.state.detaches.fetch_add(1, Ordering::Relaxed);
    }
}

fn core_local_pool(max_size: usize) -> (Pool, Arc<State>) {
    let manager = TestManager::default();
    let state = manager.state.clone();
    let pool = Pool::builder(manager)
        .max_size(max_size)
        .pool_mode(PoolMode::CoreLocal)
        .build()
        .unwrap();
    (pool, state)
}

fn zero_wait() -> Timeouts {
    Timeouts {
        wait: Some(Duration::from_millis(0)),
        create: None,
        recycle: None,
    }
}

#[tokio::test]
async fn same_handle_reuses_local_idle_object_without_extra_create() {
    let (pool, state) = core_local_pool(1);
    let local = pool.local();

    let first = local.get().await.unwrap();
    assert_eq!(*first, 0);
    drop(first);
    assert_eq!(pool.status().available, 1);

    let second = local.get().await.unwrap();
    assert_eq!(*second, 0);
    assert_eq!(state.creates.load(Ordering::Relaxed), 1);
    assert_eq!(state.recycles.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn local_idle_queue_is_fifo_within_handle() {
    let (pool, _) = core_local_pool(2);
    let local = pool.local();

    let first = local.get().await.unwrap();
    let second = local.get().await.unwrap();
    assert_eq!(*first, 0);
    assert_eq!(*second, 1);
    drop(first);
    drop(second);

    assert_eq!(*local.get().await.unwrap(), 0);
    assert_eq!(*local.get().await.unwrap(), 1);
}

#[tokio::test]
async fn local_idle_is_owned_by_origin_handle() {
    let (pool, _) = core_local_pool(1);
    let local_a = pool.local();
    let local_b = pool.local();

    drop(local_a.get().await.unwrap());

    let err = local_b.timeout_get(&zero_wait()).await.unwrap_err();
    assert!(matches!(err, PoolError::Timeout(_)));
    assert_eq!(*local_a.get().await.unwrap(), 0);
}

#[tokio::test]
async fn checked_out_object_releases_capacity_when_origin_handle_is_dropped() {
    let (pool, state) = core_local_pool(1);
    let local = pool.local();
    let obj = local.get().await.unwrap();
    drop(local);
    drop(obj);

    assert_eq!(state.detaches.load(Ordering::Relaxed), 1);
    let local = pool.local();
    assert_eq!(*local.get().await.unwrap(), 1);
    assert_eq!(state.creates.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn object_take_detaches_and_releases_capacity() {
    let (pool, state) = core_local_pool(1);
    let local = pool.local();
    let obj = local.get().await.unwrap();

    assert_eq!(Object::take(obj), 0);
    assert_eq!(state.detaches.load(Ordering::Relaxed), 1);
    assert_eq!(pool.status().size, 0);
    assert_eq!(*local.get().await.unwrap(), 1);
}

#[tokio::test]
async fn close_drains_local_idle_and_checked_out_return_after_close() {
    let (pool, state) = core_local_pool(2);
    let local = pool.local();

    drop(local.get().await.unwrap());
    assert_eq!(pool.status().size, 1);
    pool.close();
    assert_eq!(pool.status().size, 0);
    assert_eq!(pool.status().max_size, 0);
    assert_eq!(state.detaches.load(Ordering::Relaxed), 1);

    let (pool, state) = core_local_pool(1);
    let local = pool.local();
    let obj = local.get().await.unwrap();
    pool.close();
    drop(obj);
    assert_eq!(pool.status().size, 0);
    assert_eq!(state.detaches.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn resize_and_retain_apply_to_local_idle_objects() {
    let (pool, state) = core_local_pool(2);
    let local = pool.local();

    let first = local.get().await.unwrap();
    let second = local.get().await.unwrap();
    drop(first);
    drop(second);
    assert_eq!(pool.status().size, 2);
    assert_eq!(pool.status().available, 2);

    pool.resize(1);
    assert_eq!(pool.status().size, 1);
    assert_eq!(pool.status().max_size, 1);
    assert_eq!(pool.status().available, 1);
    assert_eq!(state.detaches.load(Ordering::Relaxed), 1);

    let (pool, _) = core_local_pool(2);
    let local = pool.local();
    let first = local.get().await.unwrap();
    let retained_value = *first;
    let second = local.get().await.unwrap();
    drop(first);
    drop(second);
    let result = pool.retain(|value, _| *value == retained_value);
    assert_eq!(result.retained, 1);
    assert_eq!(result.removed.len(), 1);
    assert_eq!(pool.status().size, 1);
}

#[tokio::test]
async fn local_recycle_failure_discards_and_recovers_capacity() {
    let (pool, state) = core_local_pool(1);
    let local = pool.local();

    drop(local.get().await.unwrap());
    state.fail_recycle.store(true, Ordering::Relaxed);

    let obj = local.get().await.unwrap();
    assert_eq!(*obj, 1);
    assert_eq!(state.creates.load(Ordering::Relaxed), 2);
    assert_eq!(state.detaches.load(Ordering::Relaxed), 1);
    assert_eq!(pool.status().size, 1);
}

#[tokio::test]
async fn local_create_failure_recovers_waiter_accounting() {
    let (pool, state) = core_local_pool(1);
    let local = pool.local();
    state.fail_create.store(true, Ordering::Relaxed);

    assert!(local.get().await.is_err());
    assert_eq!(pool.status().size, 0);
    assert_eq!(pool.status().available, 0);
    state.fail_create.store(false, Ordering::Relaxed);
    assert_eq!(*local.get().await.unwrap(), 0);
}

#[tokio::test]
async fn cancelled_local_waiter_does_not_change_capacity() {
    let (pool, _) = core_local_pool(0);
    let local = pool.local();
    let waiter = tokio::spawn(async move { local.get().await });

    tokio::task::yield_now().await;
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    tokio::task::yield_now().await;

    assert_eq!(pool.status().size, 0);
    assert_eq!(pool.status().available, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_is_consistent_while_local_handles_are_active() {
    let (pool, _) = core_local_pool(4);
    let locals = (0..4).map(|_| pool.local()).collect::<Vec<_>>();
    let handles = locals
        .iter()
        .cloned()
        .map(|local| {
            tokio::spawn(async move {
                for _ in 0..128 {
                    drop(local.get().await.unwrap());
                }
            })
        })
        .collect::<Vec<_>>();

    for _ in 0..64 {
        let status = pool.status();
        assert!(status.size <= status.max_size);
        assert!(status.available <= status.size);
        tokio::task::yield_now().await;
    }

    for handle in handles {
        handle.await.unwrap();
    }
    let status = pool.status();
    assert_eq!(status.size, 4);
    assert_eq!(status.available, 4);
}

#[tokio::test]
async fn recycle_hooks_run_on_local_idle_checkout() {
    let manager = TestManager::default();
    let hook_count = Arc::new(AtomicUsize::new(0));
    let hook_count_clone = hook_count.clone();
    let pool = Pool::builder(manager)
        .max_size(1)
        .pool_mode(PoolMode::CoreLocal)
        .pre_recycle(Hook::sync_fn(move |_, _| {
            hook_count_clone.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }))
        .build()
        .unwrap();
    let local = pool.local();

    drop(local.get().await.unwrap());
    drop(local.get().await.unwrap());

    assert_eq!(hook_count.load(Ordering::Relaxed), 1);
}
