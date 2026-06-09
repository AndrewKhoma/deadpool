use std::{
    collections::VecDeque,
    sync::{Mutex, MutexGuard},
};

use tokio::sync::Semaphore;

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

#[derive(Debug)]
pub(crate) enum PoolStorage<T> {
    Shared(SharedStorage<T>),
    #[cfg(feature = "core-local")]
    CoreLocal(SharedStorage<T>),
}

impl<T> PoolStorage<T> {
    pub(crate) fn new(mode: PoolMode, max_size: usize) -> Self {
        let storage = SharedStorage::new(max_size);
        match mode {
            PoolMode::Shared => Self::Shared(storage),
            #[cfg(feature = "core-local")]
            PoolMode::CoreLocal => Self::CoreLocal(storage),
        }
    }

    pub(crate) const fn mode(&self) -> StorageMode {
        match self {
            Self::Shared(_) => StorageMode::Shared,
            #[cfg(feature = "core-local")]
            Self::CoreLocal(_) => StorageMode::CoreLocal,
        }
    }

    pub(crate) const fn pool_mode(&self) -> PoolMode {
        self.mode().pool_mode()
    }

    pub(crate) fn slots(&self) -> MutexGuard<'_, Slots<T>> {
        self.shared().slots.lock().unwrap()
    }

    pub(crate) const fn semaphore(&self) -> &Semaphore {
        &self.shared().semaphore
    }

    const fn shared(&self) -> &SharedStorage<T> {
        match self {
            Self::Shared(storage) => storage,
            #[cfg(feature = "core-local")]
            Self::CoreLocal(storage) => storage,
        }
    }
}

#[derive(Debug)]
pub(crate) struct SharedStorage<T> {
    slots: Mutex<Slots<T>>,
    semaphore: Semaphore,
}

impl<T> SharedStorage<T> {
    fn new(max_size: usize) -> Self {
        Self {
            slots: Mutex::new(Slots {
                vec: VecDeque::with_capacity(max_size),
                size: 0,
                max_size,
            }),
            semaphore: Semaphore::new(max_size),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Slots<T> {
    pub(crate) vec: VecDeque<T>,
    pub(crate) size: usize,
    pub(crate) max_size: usize,
}
