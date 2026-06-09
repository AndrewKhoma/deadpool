use crate::PoolMode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StorageMode {
    Shared,
    #[cfg(feature = "core-local")]
    CoreLocal,
}

impl StorageMode {
    pub(crate) const fn pool_mode(self) -> PoolMode {
        match self {
            Self::Shared => PoolMode::Shared,
            #[cfg(feature = "core-local")]
            Self::CoreLocal => PoolMode::CoreLocal,
        }
    }
}

impl From<PoolMode> for StorageMode {
    fn from(value: PoolMode) -> Self {
        match value {
            PoolMode::Shared => Self::Shared,
            #[cfg(feature = "core-local")]
            PoolMode::CoreLocal => Self::CoreLocal,
        }
    }
}
