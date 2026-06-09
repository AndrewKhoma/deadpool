use std::{
    convert::TryInto,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicIsize, AtomicUsize},
    },
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
    pub(crate) fn empty(mode: PoolMode, max_size: usize) -> Self {
        Self::from_queue(mode, Vec::with_capacity(max_size), max_size)
    }

    pub(crate) fn from_queue(mode: PoolMode, queue: Vec<T>, max_size: usize) -> Self {
        let storage = SharedStorage::from_queue(queue, max_size);
        match mode {
            PoolMode::Shared => Self::Shared(storage),
            #[cfg(feature = "core-local")]
            PoolMode::CoreLocal => Self::CoreLocal(storage),
        }
    }

    pub(crate) const fn pool_mode(&self) -> PoolMode {
        self.mode().pool_mode()
    }

    pub(crate) const fn mode(&self) -> StorageMode {
        match self {
            Self::Shared(_) => StorageMode::Shared,
            #[cfg(feature = "core-local")]
            Self::CoreLocal(_) => StorageMode::CoreLocal,
        }
    }

    pub(crate) fn queue(&self) -> MutexGuard<'_, Vec<T>> {
        self.shared().queue.lock().unwrap()
    }

    pub(crate) const fn size(&self) -> &AtomicUsize {
        &self.shared().size
    }

    pub(crate) const fn size_semaphore(&self) -> &Semaphore {
        &self.shared().size_semaphore
    }

    pub(crate) const fn available(&self) -> &AtomicIsize {
        &self.shared().available
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
    queue: Mutex<Vec<T>>,
    size: AtomicUsize,
    size_semaphore: Semaphore,
    available: AtomicIsize,
    semaphore: Semaphore,
}

impl<T> SharedStorage<T> {
    fn from_queue(queue: Vec<T>, max_size: usize) -> Self {
        let len = queue.len();
        Self {
            queue: Mutex::new(queue),
            size: AtomicUsize::new(len),
            size_semaphore: Semaphore::new(max_size.saturating_sub(len)),
            available: AtomicIsize::new(len.try_into().unwrap()),
            semaphore: Semaphore::new(len),
        }
    }
}
