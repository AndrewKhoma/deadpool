#![cfg(feature = "core-local")]

#[cfg(feature = "managed")]
mod managed_api {
    use std::convert::Infallible;

    use deadpool::{
        PoolMode,
        managed::{self, Metrics, RecycleResult},
    };

    struct Manager;

    impl managed::Manager for Manager {
        type Type = usize;
        type Error = Infallible;

        async fn create(&self) -> Result<usize, Infallible> {
            Ok(1)
        }

        async fn recycle(&self, _: &mut usize, _: &Metrics) -> RecycleResult<Infallible> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn builder_selects_core_local_mode_and_creates_local_handle() {
        let pool: managed::Pool<Manager> = managed::Pool::builder(Manager)
            .pool_mode(PoolMode::CoreLocal)
            .build()
            .unwrap();
        assert_eq!(pool.pool_mode(), PoolMode::CoreLocal);

        let local = pool.local();
        assert_eq!(local.pool_mode(), PoolMode::CoreLocal);
        assert_eq!(*local.get().await.unwrap(), 1);
    }

    #[test]
    fn default_builder_uses_shared_mode() {
        let pool = managed::Pool::<Manager>::builder(Manager).build().unwrap();
        assert_eq!(pool.pool_mode(), PoolMode::Shared);
    }

    #[test]
    fn public_config_struct_literal_does_not_require_mode() {
        let config = managed::PoolConfig {
            max_size: 1,
            timeouts: managed::Timeouts::default(),
            queue_mode: managed::QueueMode::default(),
        };
        let pool = managed::Pool::<Manager>::builder(Manager)
            .config(config)
            .build()
            .unwrap();
        assert_eq!(pool.pool_mode(), PoolMode::Shared);
    }
}

#[cfg(feature = "unmanaged")]
mod unmanaged_api {
    use deadpool::{PoolMode, unmanaged};

    #[tokio::test]
    async fn constructors_select_core_local_mode_and_create_local_handle() {
        let pool = unmanaged::Pool::from_iter_with_mode([1_usize], PoolMode::CoreLocal);
        assert_eq!(pool.pool_mode(), PoolMode::CoreLocal);

        let local = pool.local();
        assert_eq!(local.pool_mode(), PoolMode::CoreLocal);
        assert_eq!(*local.get().await.unwrap(), 1);
    }

    #[test]
    fn default_constructors_use_shared_mode() {
        let pool = unmanaged::Pool::<usize>::new(1);
        assert_eq!(pool.pool_mode(), PoolMode::Shared);

        let pool = unmanaged::Pool::from([1_usize]);
        assert_eq!(pool.pool_mode(), PoolMode::Shared);
    }

    #[test]
    fn public_config_struct_literal_does_not_require_mode() {
        let config = unmanaged::PoolConfig {
            max_size: 1,
            timeout: None,
            runtime: None,
        };
        let pool = unmanaged::Pool::<usize>::from_config_with_mode(&config, PoolMode::CoreLocal);
        assert_eq!(pool.pool_mode(), PoolMode::CoreLocal);
    }
}
