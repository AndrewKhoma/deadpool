#![cfg(all(feature = "unmanaged", feature = "core-local"))]

use std::time::Duration;

use deadpool::{
    PoolMode, Runtime,
    unmanaged::{Pool, PoolError},
};

#[tokio::test]
async fn same_handle_add_get_and_return_reuses_local_idle() {
    let pool = Pool::new_with_mode(1, PoolMode::CoreLocal);
    let local = pool.local();

    local.add(1).await.unwrap();
    assert_eq!(pool.status().size, 1);
    assert_eq!(pool.status().available, 1);

    let obj = local.get().await.unwrap();
    assert_eq!(*obj, 1);
    assert_eq!(pool.status().available, 0);
    drop(obj);
    assert_eq!(pool.status().available, 1);
    assert_eq!(*local.get().await.unwrap(), 1);
}

#[tokio::test]
async fn local_idle_is_owned_by_origin_handle() {
    let pool = Pool::new_with_mode(1, PoolMode::CoreLocal);
    let local_a = pool.local();
    let local_b = pool.local();
    local_a.add(1).await.unwrap();

    assert!(matches!(local_b.try_get(), Err(PoolError::Timeout)));
    assert_eq!(*local_a.try_get().unwrap(), 1);
}

#[tokio::test]
async fn local_waiter_is_woken_by_local_return_shared_add_and_close() {
    let pool = Pool::new_with_mode(1, PoolMode::CoreLocal);
    let local = pool.local();
    local.add(1).await.unwrap();
    let obj = local.get().await.unwrap();
    let waiter = {
        let local = local.clone();
        tokio::spawn(async move { local.get().await })
    };
    wait_until(|| pool.status().waiting == 1).await;
    drop(obj);
    assert_eq!(*waiter.await.unwrap().unwrap(), 1);

    let pool = Pool::new_with_mode(1, PoolMode::CoreLocal);
    let local = pool.local();
    let waiter = {
        let local = local.clone();
        tokio::spawn(async move { local.get().await })
    };
    wait_until(|| pool.status().waiting == 1).await;
    pool.add(2).await.unwrap();
    assert_eq!(*waiter.await.unwrap().unwrap(), 2);

    let pool = Pool::<usize>::new_with_mode(1, PoolMode::CoreLocal);
    let local = pool.local();
    let waiter = tokio::spawn(async move { local.get().await });
    wait_until(|| pool.status().waiting == 1).await;
    pool.close();
    assert!(matches!(waiter.await.unwrap(), Err(PoolError::Closed)));
}

#[tokio::test]
async fn local_timeout_and_no_runtime_behaviour_matches_pool() {
    let pool = Pool::<usize>::new_with_mode(1, PoolMode::CoreLocal);
    let local = pool.local();
    assert!(matches!(
        local.timeout_get(Some(Duration::from_millis(1))).await,
        Err(PoolError::NoRuntimeSpecified)
    ));
    assert!(matches!(
        local.timeout_get(Some(Duration::ZERO)).await,
        Err(PoolError::Timeout)
    ));

    let pool = Pool::from_config_with_mode(
        &deadpool::unmanaged::PoolConfig {
            max_size: 1,
            timeout: Some(Duration::from_millis(50)),
            runtime: Some(Runtime::Tokio1),
        },
        PoolMode::CoreLocal,
    );
    let local = pool.local();
    let waiter = {
        let local = local.clone();
        tokio::spawn(async move { local.get().await })
    };
    wait_until(|| pool.status().waiting == 1).await;
    local.add(3).await.unwrap();
    assert_eq!(*waiter.await.unwrap().unwrap(), 3);
    let pool = Pool::from_config_with_mode(
        &deadpool::unmanaged::PoolConfig {
            max_size: 1,
            timeout: Some(Duration::from_millis(50)),
            runtime: Some(Runtime::Tokio1),
        },
        PoolMode::CoreLocal,
    );
    let local_a = pool.local();
    let local_b = pool.local();
    local_a.add(4).await.unwrap();
    drop(local_a.get().await.unwrap());
    let started = std::time::Instant::now();
    assert!(matches!(
        local_b.timeout_get(Some(Duration::from_millis(20))).await,
        Err(PoolError::Timeout)
    ));
    assert!(started.elapsed() < Duration::from_millis(250));
    assert_eq!(pool.status().waiting, 0);
}

#[tokio::test]
async fn take_remove_close_and_local_drop_release_capacity() {
    let pool = Pool::new_with_mode(1, PoolMode::CoreLocal);
    let local = pool.local();
    local.add(1).await.unwrap();
    assert_eq!(
        deadpool::unmanaged::Object::take(local.get().await.unwrap()),
        1
    );
    assert_eq!(pool.status().size, 0);
    assert!(local.try_add(2).is_ok());
    assert_eq!(local.try_remove().unwrap(), 2);
    assert_eq!(pool.status().size, 0);

    local.add(3).await.unwrap();
    drop(local);
    assert_eq!(pool.status().size, 0);
    let local = pool.local();
    assert!(local.try_add(4).is_ok());
    pool.close();
    assert_eq!(pool.status().size, 0);
    assert!(matches!(local.try_get(), Err(PoolError::Closed)));
}

#[tokio::test]
async fn boundary_capacity_cases_preserve_accounting() {
    let pool = Pool::<usize>::new_with_mode(0, PoolMode::CoreLocal);
    let local = pool.local();
    assert!(matches!(local.try_add(1), Err((1, PoolError::Timeout))));
    assert!(matches!(local.try_get(), Err(PoolError::Timeout)));
    assert_eq!(pool.status().size, 0);

    let pool = Pool::new_with_mode(1, PoolMode::CoreLocal);
    let local = pool.local();
    local.add(1).await.unwrap();
    let checked_out = local.get().await.unwrap();
    let waiter = {
        let local = local.clone();
        tokio::spawn(async move { local.get().await })
    };
    wait_until(|| local.local_wait_count() > 0).await;
    assert_eq!(deadpool::unmanaged::Object::take(checked_out), 1);
    local.add(2).await.unwrap();
    assert_eq!(*waiter.await.unwrap().unwrap(), 2);
    assert_eq!(pool.status().size, 1);
    assert_eq!(pool.status().available, 1);

    let pool = Pool::new_with_mode(1024, PoolMode::CoreLocal);
    let local = pool.local();
    for i in 0..1024 {
        local.try_add(i).unwrap();
    }
    assert!(matches!(
        local.try_add(1025),
        Err((1025, PoolError::Timeout))
    ));
    assert_eq!(pool.status().size, 1024);
    assert_eq!(pool.status().available, 1024);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_is_consistent_while_local_handles_are_active() {
    let pool = Pool::new_with_mode(4, PoolMode::CoreLocal);
    let locals = (0..4).map(|_| pool.local()).collect::<Vec<_>>();
    for (idx, local) in locals.iter().enumerate() {
        local.add(idx).await.unwrap();
    }
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
    assert_eq!(pool.status().size, 4);
    assert_eq!(pool.status().available, 4);
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
