#![cfg(feature = "core-local")]

#[test]
fn postgres_reexports_core_local_api_surface() {
    let mode = deadpool_postgres::PoolMode::CoreLocal;
    assert_eq!(mode, deadpool_postgres::PoolMode::CoreLocal);

    fn assert_local_pool_alias(_: Option<deadpool_postgres::LocalPool>) {}
    assert_local_pool_alias(None);
}
