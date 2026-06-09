use std::{
    collections::VecDeque,
    sync::{Mutex, MutexGuard},
};

#[cfg(feature = "core-local")]
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[cfg(feature = "core-local")]
use crossbeam_queue::SegQueue;
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
    CoreLocal(CoreLocalStorage<T>),
}

impl<T> PoolStorage<T> {
    pub(crate) fn new(mode: PoolMode, max_size: usize) -> Self {
        match mode {
            PoolMode::Shared => Self::Shared(SharedStorage::new(max_size)),
            #[cfg(feature = "core-local")]
            PoolMode::CoreLocal => Self::CoreLocal(CoreLocalStorage::new(max_size)),
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

    pub(crate) fn semaphore(&self) -> &Semaphore {
        &self.shared().semaphore
    }

    #[cfg(feature = "core-local")]
    pub(crate) fn register_local(&self) -> Option<Arc<LocalStorage<T>>> {
        match self {
            Self::Shared(_) => None,
            Self::CoreLocal(storage) => Some(storage.register_local()),
        }
    }

    #[cfg(feature = "core-local")]
    pub(crate) fn local_storages(&self) -> Vec<Arc<LocalStorage<T>>> {
        match self {
            Self::Shared(_) => Vec::new(),
            Self::CoreLocal(storage) => storage.local_storages(),
        }
    }

    fn shared(&self) -> &SharedStorage<T> {
        match self {
            Self::Shared(storage) => storage,
            #[cfg(feature = "core-local")]
            Self::CoreLocal(storage) => &storage.shared,
        }
    }
}

#[cfg(feature = "core-local")]
#[derive(Debug)]
pub(crate) struct CoreLocalStorage<T> {
    shared: SharedStorage<T>,
    locals: Mutex<Vec<Weak<LocalStorage<T>>>>,
}

#[cfg(feature = "core-local")]
impl<T> CoreLocalStorage<T> {
    fn new(max_size: usize) -> Self {
        Self {
            shared: SharedStorage::new(max_size),
            locals: Mutex::new(Vec::new()),
        }
    }

    fn register_local(&self) -> Arc<LocalStorage<T>> {
        let local = Arc::new(LocalStorage::default());
        self.locals.lock().unwrap().push(Arc::downgrade(&local));
        local
    }

    fn local_storages(&self) -> Vec<Arc<LocalStorage<T>>> {
        let mut locals = self.locals.lock().unwrap();
        let mut live = Vec::with_capacity(locals.len());
        locals.retain(|local| {
            if let Some(local) = local.upgrade() {
                live.push(local);
                true
            } else {
                false
            }
        });
        live
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

#[cfg(feature = "core-local")]
#[derive(Debug)]
pub(crate) struct LocalStorage<T> {
    queue: SegQueue<T>,
    available: AtomicUsize,
    waits: AtomicUsize,
    owners: AtomicUsize,
    active: AtomicBool,
    semaphore: Semaphore,
}

#[cfg(feature = "core-local")]
impl<T> Default for LocalStorage<T> {
    fn default() -> Self {
        Self {
            queue: SegQueue::new(),
            available: AtomicUsize::new(0),
            waits: AtomicUsize::new(0),
            owners: AtomicUsize::new(1),
            active: AtomicBool::new(true),
            semaphore: Semaphore::new(0),
        }
    }
}

#[cfg(feature = "core-local")]
impl<T> LocalStorage<T> {
    pub(crate) fn push(&self, value: T) -> Result<(), T> {
        if !self.is_active() {
            return Err(value);
        }
        self.queue.push(value);
        let _ = self.available.fetch_add(1, Ordering::Relaxed);
        self.semaphore.add_permits(1);
        Ok(())
    }

    pub(crate) fn pop(&self) -> Option<T> {
        let permit = self.semaphore.try_acquire().ok()?;
        permit.forget();
        let value = self.queue.pop();
        if value.is_some() {
            let _ = self.available.fetch_sub(1, Ordering::Relaxed);
        }
        value
    }

    pub(crate) async fn pop_wait(&self) -> Option<T> {
        let _ = self.waits.fetch_add(1, Ordering::Relaxed);
        let permit = self.semaphore.acquire().await.ok()?;
        permit.forget();
        let value = self.queue.pop();
        if value.is_some() {
            let _ = self.available.fetch_sub(1, Ordering::Relaxed);
        }
        value
    }

    pub(crate) fn signal(&self) {
        if self.is_active() {
            self.semaphore.add_permits(1);
        }
    }

    pub(crate) fn close(&self) {
        self.active.store(false, Ordering::Relaxed);
        self.semaphore.close();
    }

    pub(crate) fn clone_owner(&self) {
        let _ = self.owners.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn release_owner(&self) -> bool {
        self.owners.fetch_sub(1, Ordering::AcqRel) == 1
    }

    pub(crate) fn waits(&self) -> usize {
        self.waits.load(Ordering::Relaxed)
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub(crate) fn drain(&self) -> Vec<T> {
        let mut drained = Vec::new();
        while let Some(value) = self.queue.pop() {
            let _ = self.available.fetch_sub(1, Ordering::Relaxed);
            drained.push(value);
        }
        drained
    }
}
