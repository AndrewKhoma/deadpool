use std::{
    convert::TryInto,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicIsize, AtomicUsize},
    },
};

#[cfg(feature = "core-local")]
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
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
    pub(crate) fn empty(mode: PoolMode, max_size: usize) -> Self {
        Self::from_queue(mode, Vec::with_capacity(max_size), max_size)
    }

    pub(crate) fn from_queue(mode: PoolMode, queue: Vec<T>, max_size: usize) -> Self {
        match mode {
            PoolMode::Shared => Self::Shared(SharedStorage::from_queue(queue, max_size)),
            #[cfg(feature = "core-local")]
            PoolMode::CoreLocal => Self::CoreLocal(CoreLocalStorage::from_queue(queue, max_size)),
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

    pub(crate) fn size(&self) -> &AtomicUsize {
        &self.shared().size
    }

    pub(crate) fn size_semaphore(&self) -> &Semaphore {
        &self.shared().size_semaphore
    }

    pub(crate) fn available(&self) -> &AtomicIsize {
        &self.shared().available
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

    #[cfg(feature = "core-local")]
    pub(crate) fn local_available(&self) -> usize {
        self.local_storages()
            .iter()
            .map(|local| local.available())
            .sum()
    }

    #[cfg(feature = "core-local")]
    pub(crate) fn local_waiting(&self) -> usize {
        self.local_storages()
            .iter()
            .map(|local| local.waiting())
            .sum()
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
    fn from_queue(queue: Vec<T>, max_size: usize) -> Self {
        Self {
            shared: SharedStorage::from_queue(queue, max_size),
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
    queue: Mutex<Vec<T>>,
    size: AtomicUsize,
    size_semaphore: Semaphore,
    available: AtomicIsize,
    semaphore: Semaphore,
}

#[cfg(feature = "core-local")]
#[derive(Debug)]
pub(crate) struct LocalStorage<T> {
    queue: SegQueue<T>,
    available: AtomicUsize,
    waits: AtomicUsize,
    waiters: AtomicUsize,
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
            waiters: AtomicUsize::new(0),
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
        let _ = self.waiters.fetch_add(1, Ordering::Relaxed);
        let permit = match self.semaphore.acquire().await {
            Ok(permit) => permit,
            Err(_) => {
                let _ = self.waiters.fetch_sub(1, Ordering::Relaxed);
                return None;
            }
        };
        let _ = self.waiters.fetch_sub(1, Ordering::Relaxed);
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

    pub(crate) fn available(&self) -> usize {
        self.available.load(Ordering::Relaxed)
    }

    pub(crate) fn waiting(&self) -> usize {
        self.waiters.load(Ordering::Relaxed)
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
