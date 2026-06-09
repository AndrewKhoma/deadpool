#![cfg(feature = "core-local")]

#[test]
fn postgres_reexports_core_local_api_surface() {
    let mode = deadpool_postgres::PoolMode::CoreLocal;
    assert_eq!(mode, deadpool_postgres::PoolMode::CoreLocal);

    fn assert_local_pool_alias(_: Option<deadpool_postgres::LocalPool>) {}
    assert_local_pool_alias(None);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn config_builder_can_opt_into_core_local_without_changing_config_shape() {
    let mut cfg = deadpool_postgres::Config::new();
    cfg.dbname = Some("deadpool".to_string());
    cfg.pool = Some(deadpool_postgres::PoolConfig::new(2));

    let pool = cfg
        .builder(deadpool_postgres::tokio_postgres::NoTls)
        .unwrap()
        .pool_mode(deadpool_postgres::PoolMode::CoreLocal)
        .build()
        .unwrap();

    assert_eq!(pool.pool_mode(), deadpool_postgres::PoolMode::CoreLocal);
    assert_eq!(
        pool.local().pool_mode(),
        deadpool_postgres::PoolMode::CoreLocal
    );
    assert_eq!(pool.status().max_size, 2);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn gateway_shaped_dual_pool_builds_with_fast_and_clean_recycling() {
    fn pool_with(recycling_method: deadpool_postgres::RecyclingMethod) -> deadpool_postgres::Pool {
        let mut cfg = deadpool_postgres::Config::new();
        cfg.dbname = Some("deadpool".to_string());
        cfg.manager = Some(deadpool_postgres::ManagerConfig { recycling_method });
        cfg.pool = Some(deadpool_postgres::PoolConfig::new(2));
        cfg.builder(deadpool_postgres::tokio_postgres::NoTls)
            .unwrap()
            .pool_mode(deadpool_postgres::PoolMode::CoreLocal)
            .build()
            .unwrap()
    }

    let primary = pool_with(deadpool_postgres::RecyclingMethod::Fast);
    let timeout = pool_with(deadpool_postgres::RecyclingMethod::Clean);

    assert_eq!(primary.pool_mode(), deadpool_postgres::PoolMode::CoreLocal);
    assert_eq!(timeout.pool_mode(), deadpool_postgres::PoolMode::CoreLocal);
    assert_eq!(primary.status().max_size + timeout.status().max_size, 4);
    primary.retain(|_, _| true);
    timeout.retain(|_, _| true);
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn gateway_shaped_timeout_error_uses_core_local_pool() {
    let mut cfg = deadpool_postgres::Config::new();
    cfg.dbname = Some("deadpool".to_string());
    let pool = cfg
        .builder(deadpool_postgres::tokio_postgres::NoTls)
        .unwrap()
        .max_size(0)
        .wait_timeout(Some(std::time::Duration::ZERO))
        .runtime(deadpool_postgres::Runtime::Tokio1)
        .pool_mode(deadpool_postgres::PoolMode::CoreLocal)
        .build()
        .unwrap();

    let err = pool.local().get().await.unwrap_err();
    assert!(matches!(
        err,
        deadpool_postgres::PoolError::Timeout(deadpool_postgres::TimeoutType::Wait)
    ));
    assert_eq!(pool.status().waiting, 0);
}
