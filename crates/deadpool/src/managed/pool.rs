#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use std::{
    collections::VecDeque,
    fmt,
    future::Future,
    marker::PhantomData,
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use deadpool_runtime::{Runtime, timeout};
use tokio::sync::TryAcquireError;

use crate::{
    PoolMode, Status,
    managed::{
        Manager, Metrics, Object, PoolBuilder, PoolConfig, PoolError, QueueMode, TimeoutType,
        Timeouts, dropguard::DropGuard, hooks::Hooks, object::ObjectInner, storage::PoolStorage,
    },
};

#[cfg(feature = "core-local")]
use crate::managed::storage::LocalStorage;
#[cfg(feature = "core-local")]
use std::sync::Weak as StdWeak;

/// Generic object and connection pool.
///
/// This struct can be cloned and transferred across thread boundaries and uses
/// reference counting for its internal state.
pub struct Pool<M: Manager, W: From<Object<M>> = Object<M>> {
    pub(crate) inner: Arc<PoolInner<M>>,
    pub(crate) _wrapper: PhantomData<fn() -> W>,
}

// Implemented manually to avoid unnecessary trait bound on `W` type parameter.
impl<M, W> fmt::Debug for Pool<M, W>
where
    M: fmt::Debug + Manager,
    M::Type: fmt::Debug,
    W: From<Object<M>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pool")
            .field("inner", &self.inner)
            .field("wrapper", &self._wrapper)
            .finish()
    }
}

impl<M: Manager, W: From<Object<M>>> Clone for Pool<M, W> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _wrapper: PhantomData,
        }
    }
}

impl<M: Manager, W: From<Object<M>>> Pool<M, W> {
    /// Instantiates a builder for a new [`Pool`].
    ///
    /// This is the only way to create a [`Pool`] instance.
    pub fn builder(manager: M) -> PoolBuilder<M, W> {
        PoolBuilder::new(manager)
    }

    pub(crate) fn from_builder(builder: PoolBuilder<M, W>) -> Self {
        Self {
            inner: Arc::new(PoolInner {
                manager: builder.manager,
                next_id: AtomicUsize::new(0),
                users: AtomicUsize::new(0),
                storage: PoolStorage::new(builder.mode, builder.config.max_size),
                config: builder.config,
                hooks: builder.hooks,
                runtime: builder.runtime,
            }),
            _wrapper: PhantomData,
        }
    }

    /// Returns the configured [`PoolMode`].
    #[must_use]
    pub fn pool_mode(&self) -> PoolMode {
        self.inner.storage.pool_mode()
    }

    /// Creates a local handle for this pool.
    ///
    /// In [`PoolMode::Shared`], the handle delegates to the shared pool. In
    /// [`PoolMode::CoreLocal`], the handle owns a local FIFO queue for
    /// same-handle returns while sharing global capacity with the backing pool.
    /// The configured [`QueueMode`] only applies to objects retrieved from the
    /// shared fallback queue.
    #[cfg(feature = "core-local")]
    #[cfg_attr(docsrs, doc(cfg(feature = "core-local")))]
    #[must_use]
    pub fn local(&self) -> LocalPool<M, W> {
        LocalPool {
            pool: self.clone(),
            local: self.inner.storage.register_local(),
        }
    }

    /// Retrieves an [`Object`] from this [`Pool`] or waits for one to
    /// become available.
    ///
    /// # Errors
    ///
    /// See [`PoolError`] for details.
    pub async fn get(&self) -> Result<W, PoolError<M::Error>> {
        self.timeout_get(&self.timeouts()).await
    }

    /// Retrieves an [`Object`] from this [`Pool`] using a different `timeout`
    /// than the configured one.
    ///
    /// # Errors
    ///
    /// See [`PoolError`] for details.
    pub async fn timeout_get(&self, timeouts: &Timeouts) -> Result<W, PoolError<M::Error>> {
        let _ = self.inner.users.fetch_add(1, Ordering::Relaxed);
        let users_guard = DropGuard(|| {
            let _ = self.inner.users.fetch_sub(1, Ordering::Relaxed);
        });

        let non_blocking = match timeouts.wait {
            Some(t) => t.as_nanos() == 0,
            None => false,
        };

        let permit = if non_blocking {
            self.inner
                .storage
                .semaphore()
                .try_acquire()
                .map_err(|e| match e {
                    TryAcquireError::Closed => PoolError::Closed,
                    TryAcquireError::NoPermits => PoolError::Timeout(TimeoutType::Wait),
                })?
        } else {
            apply_timeout(
                self.inner.runtime,
                TimeoutType::Wait,
                timeouts.wait,
                async {
                    self.inner
                        .storage
                        .semaphore()
                        .acquire()
                        .await
                        .map_err(|_| PoolError::Closed)
                },
            )
            .await?
        };

        let inner_obj = loop {
            let inner_obj = match self.inner.config.queue_mode {
                QueueMode::Fifo => self.inner.storage.slots().vec.pop_front(),
                QueueMode::Lifo => self.inner.storage.slots().vec.pop_back(),
            };
            let inner_obj = if let Some(inner_obj) = inner_obj {
                match self.try_recycle(timeouts, inner_obj).await {
                    Ok(inner_obj) => inner_obj,
                    Err(err) => {
                        self.inner.notify_local_waiters();
                        return Err(err);
                    }
                }
            } else {
                match self.try_create(timeouts).await {
                    Ok(inner_obj) => inner_obj,
                    Err(err) => {
                        self.inner.notify_local_waiters();
                        return Err(err);
                    }
                }
            };
            if let Some(inner_obj) = inner_obj {
                break inner_obj;
            }
        };

        users_guard.disarm();
        permit.forget();

        Ok(self.wrap_object(inner_obj, None).into())
    }

    #[cfg(feature = "core-local")]
    async fn timeout_get_for_local(
        &self,
        timeouts: &Timeouts,
        local: &Arc<LocalStorage<ObjectInner<M>>>,
    ) -> Result<W, PoolError<M::Error>> {
        loop {
            let Some(inner_obj) = local.pop() else {
                let mut try_timeouts = *timeouts;
                try_timeouts.wait = Some(Duration::from_millis(0));
                match self
                    .timeout_get_with_local_return(&try_timeouts, Some(Arc::downgrade(local)))
                    .await
                {
                    Ok(obj) => return Ok(obj),
                    Err(PoolError::Timeout(TimeoutType::Wait)) => {
                        if timeouts
                            .wait
                            .is_some_and(|duration| duration.as_nanos() == 0)
                        {
                            return Err(PoolError::Timeout(TimeoutType::Wait));
                        }
                        if let Some(inner_obj) = self.wait_for_local_object(timeouts, local).await?
                        {
                            if let Some(obj) = self
                                .checkout_local_inner(timeouts, local, inner_obj)
                                .await?
                            {
                                return Ok(obj);
                            }
                        }
                        continue;
                    }
                    Err(err) => return Err(err),
                }
            };
            let _ = self.inner.users.fetch_add(1, Ordering::Relaxed);
            let users_guard = DropGuard(|| {
                let _ = self.inner.users.fetch_sub(1, Ordering::Relaxed);
            });
            if let Some(inner_obj) = self.try_recycle_local(timeouts, inner_obj).await? {
                users_guard.disarm();
                return Ok(self
                    .wrap_object(inner_obj, Some(Arc::downgrade(local)))
                    .into());
            }
        }
    }

    #[cfg(feature = "core-local")]
    async fn wait_for_local_object(
        &self,
        timeouts: &Timeouts,
        local: &LocalStorage<ObjectInner<M>>,
    ) -> Result<Option<ObjectInner<M>>, PoolError<M::Error>> {
        let _ = self.inner.users.fetch_add(1, Ordering::Relaxed);
        let users_guard = DropGuard(|| {
            let _ = self.inner.users.fetch_sub(1, Ordering::Relaxed);
        });
        let result = match (self.inner.runtime, timeouts.wait) {
            (_, None) => Ok(local.pop_wait().await),
            (Some(runtime), Some(duration)) => timeout(runtime, duration, local.pop_wait())
                .await
                .ok_or(PoolError::Timeout(TimeoutType::Wait)),
            (None, Some(_)) => Err(PoolError::NoRuntimeSpecified),
        };
        drop(users_guard);
        result
    }

    #[cfg(feature = "core-local")]
    async fn checkout_local_inner(
        &self,
        timeouts: &Timeouts,
        local: &Arc<LocalStorage<ObjectInner<M>>>,
        inner_obj: ObjectInner<M>,
    ) -> Result<Option<W>, PoolError<M::Error>> {
        let _ = self.inner.users.fetch_add(1, Ordering::Relaxed);
        let users_guard = DropGuard(|| {
            let _ = self.inner.users.fetch_sub(1, Ordering::Relaxed);
        });
        if let Some(inner_obj) = self.try_recycle_local(timeouts, inner_obj).await? {
            users_guard.disarm();
            Ok(Some(
                self.wrap_object(inner_obj, Some(Arc::downgrade(local)))
                    .into(),
            ))
        } else {
            Ok(None)
        }
    }

    #[cfg(feature = "core-local")]
    async fn timeout_get_with_local_return(
        &self,
        timeouts: &Timeouts,
        local: Option<StdWeak<LocalStorage<ObjectInner<M>>>>,
    ) -> Result<W, PoolError<M::Error>> {
        let _ = self.inner.users.fetch_add(1, Ordering::Relaxed);
        let users_guard = DropGuard(|| {
            let _ = self.inner.users.fetch_sub(1, Ordering::Relaxed);
        });

        let non_blocking = match timeouts.wait {
            Some(t) => t.as_nanos() == 0,
            None => false,
        };

        let permit = if non_blocking {
            self.inner
                .storage
                .semaphore()
                .try_acquire()
                .map_err(|e| match e {
                    TryAcquireError::Closed => PoolError::Closed,
                    TryAcquireError::NoPermits => PoolError::Timeout(TimeoutType::Wait),
                })?
        } else {
            apply_timeout(
                self.inner.runtime,
                TimeoutType::Wait,
                timeouts.wait,
                async {
                    self.inner
                        .storage
                        .semaphore()
                        .acquire()
                        .await
                        .map_err(|_| PoolError::Closed)
                },
            )
            .await?
        };

        let inner_obj = loop {
            let inner_obj = match self.inner.config.queue_mode {
                QueueMode::Fifo => self.inner.storage.slots().vec.pop_front(),
                QueueMode::Lifo => self.inner.storage.slots().vec.pop_back(),
            };
            let inner_obj = if let Some(inner_obj) = inner_obj {
                self.try_recycle(timeouts, inner_obj).await?
            } else {
                self.try_create(timeouts).await?
            };
            if let Some(inner_obj) = inner_obj {
                break inner_obj;
            }
        };

        users_guard.disarm();
        permit.forget();

        Ok(self.wrap_object(inner_obj, local).into())
    }

    #[inline]
    async fn try_recycle(
        &self,
        timeouts: &Timeouts,
        inner_obj: ObjectInner<M>,
    ) -> Result<Option<ObjectInner<M>>, PoolError<M::Error>> {
        let mut unready_obj = UnreadyObject {
            inner: Some(inner_obj),
            pool: &self.inner,
        };
        let inner = unready_obj.inner();

        // Apply pre_recycle hooks
        if let Err(_e) = self.inner.hooks.pre_recycle.apply(inner).await {
            // TODO log pre_recycle error
            return Ok(None);
        }

        if apply_timeout(
            self.inner.runtime,
            TimeoutType::Recycle,
            timeouts.recycle,
            self.inner.manager.recycle(&mut inner.obj, &inner.metrics),
        )
        .await
        .is_err()
        {
            return Ok(None);
        }

        // Apply post_recycle hooks
        if let Err(_e) = self.inner.hooks.post_recycle.apply(inner).await {
            // TODO log post_recycle error
            return Ok(None);
        }

        inner.metrics.recycle_count += 1;
        #[cfg(not(target_arch = "wasm32"))]
        {
            inner.metrics.recycled = Some(Instant::now());
        }

        Ok(Some(unready_obj.ready()))
    }

    #[cfg(feature = "core-local")]
    #[inline]
    async fn try_recycle_local(
        &self,
        timeouts: &Timeouts,
        inner_obj: ObjectInner<M>,
    ) -> Result<Option<ObjectInner<M>>, PoolError<M::Error>> {
        let mut unready_obj = UnreadyLocalObject {
            inner: Some(inner_obj),
            pool: &self.inner,
        };
        let inner = unready_obj.inner();

        if self.inner.hooks.pre_recycle.apply(inner).await.is_err() {
            return Ok(None);
        }

        if apply_timeout(
            self.inner.runtime,
            TimeoutType::Recycle,
            timeouts.recycle,
            self.inner.manager.recycle(&mut inner.obj, &inner.metrics),
        )
        .await
        .is_err()
        {
            return Ok(None);
        }

        if self.inner.hooks.post_recycle.apply(inner).await.is_err() {
            return Ok(None);
        }

        inner.metrics.recycle_count += 1;
        #[cfg(not(target_arch = "wasm32"))]
        {
            inner.metrics.recycled = Some(Instant::now());
        }

        Ok(Some(unready_obj.ready()))
    }

    #[inline]
    async fn try_create(
        &self,
        timeouts: &Timeouts,
    ) -> Result<Option<ObjectInner<M>>, PoolError<M::Error>> {
        let mut unready_obj = UnreadyObject {
            inner: Some(ObjectInner {
                obj: apply_timeout(
                    self.inner.runtime,
                    TimeoutType::Create,
                    timeouts.create,
                    self.inner.manager.create(),
                )
                .await?,
                id: self.inner.next_id.fetch_add(1, Ordering::Relaxed),
                metrics: Metrics::default(),
            }),
            pool: &self.inner,
        };

        self.inner.storage.slots().size += 1;

        // Apply post_create hooks
        if let Err(e) = self
            .inner
            .hooks
            .post_create
            .apply(unready_obj.inner())
            .await
        {
            return Err(PoolError::PostCreateHook(e));
        }

        Ok(Some(unready_obj.ready()))
    }

    /**
     * Resize the pool. This change the `max_size` of the pool dropping
     * excess objects and/or making space for new ones.
     *
     * If the pool is closed this method does nothing. The [`Pool::status()`] method
     * always reports a `max_size` of 0 for closed pools.
     */
    pub fn resize(&self, max_size: usize) {
        if self.inner.storage.semaphore().is_closed() {
            return;
        }
        let mut slots = self.inner.storage.slots();
        let old_max_size = slots.max_size;
        slots.max_size = max_size;
        drop(slots);
        #[cfg(feature = "core-local")]
        self.inner.discard_local_objects_over_max_size();
        let mut slots = self.inner.storage.slots();
        // shrink pool
        if max_size < old_max_size {
            while slots.size > slots.max_size {
                if let Ok(permit) = self.inner.storage.semaphore().try_acquire() {
                    permit.forget();
                    if slots.vec.pop_front().is_some() {
                        slots.size -= 1;
                    }
                } else {
                    break;
                }
            }
            // Create a new VecDeque with a smaller capacity
            let mut vec = VecDeque::with_capacity(max_size);
            for obj in slots.vec.drain(..) {
                vec.push_back(obj);
            }
            slots.vec = vec;
        }
        // grow pool
        if max_size > old_max_size {
            let additional = slots.max_size - old_max_size;
            slots.vec.reserve_exact(additional);
            self.inner.storage.semaphore().add_permits(additional);
            drop(slots);
            #[cfg(feature = "core-local")]
            self.inner.notify_local_waiters();
        }
    }

    /// Retains only the objects specified by the given function.
    ///
    /// This function is typically used to remove objects from
    /// the pool based on their current state or metrics.
    ///
    /// **Caution:** This function blocks the entire pool while
    /// it is running. Therefore the given function should not
    /// block.
    ///
    /// The following example starts a background task that
    /// runs every 30 seconds and removes objects from the pool
    /// that haven't been used for more than one minute.
    ///
    /// ```rust,ignore
    /// let interval = Duration::from_secs(30);
    /// let max_age = Duration::from_secs(60);
    /// tokio::spawn(async move {
    ///     loop {
    ///         tokio::time::sleep(interval).await;
    ///         pool.retain(|_, metrics| metrics.last_used() < max_age);
    ///     }
    /// });
    /// ```
    pub fn retain(
        &self,
        mut predicate: impl FnMut(&M::Type, Metrics) -> bool,
    ) -> RetainResult<M::Type> {
        let mut removed = Vec::with_capacity(self.status().size);
        let mut guard = self.inner.storage.slots();
        let mut i = 0;
        // This code can be simplified once `Vec::extract_if` lands in stable Rust.
        // https://doc.rust-lang.org/std/vec/struct.Vec.html#method.extract_if
        while i < guard.vec.len() {
            let obj = &mut guard.vec[i];
            if predicate(&mut obj.obj, obj.metrics) {
                i += 1;
            } else {
                let mut obj = guard.vec.remove(i).unwrap();
                self.manager().detach(&mut obj.obj);
                removed.push(obj.obj);
            }
        }
        let retained = i;
        guard.size -= removed.len();
        drop(guard);
        #[cfg(feature = "core-local")]
        let local_retained = self
            .inner
            .retain_local_objects(&mut predicate, &mut removed);
        #[cfg(not(feature = "core-local"))]
        let local_retained = 0;
        RetainResult {
            retained: retained + local_retained,
            removed,
        }
    }

    /// Get current timeout configuration
    pub fn timeouts(&self) -> Timeouts {
        self.inner.config.timeouts
    }

    /// Closes this [`Pool`].
    ///
    /// All current and future tasks waiting for [`Object`]s will return
    /// [`PoolError::Closed`] immediately.
    ///
    /// This operation resizes the pool to 0.
    pub fn close(&self) {
        self.resize(0);
        self.inner.storage.semaphore().close();
        #[cfg(feature = "core-local")]
        self.inner.notify_local_waiters();
        #[cfg(feature = "core-local")]
        self.inner.drain_local_objects();
    }

    /// Indicates whether this [`Pool`] has been closed.
    pub fn is_closed(&self) -> bool {
        self.inner.storage.semaphore().is_closed()
    }

    /// Retrieves [`Status`] of this [`Pool`].
    #[must_use]
    pub fn status(&self) -> Status {
        let slots = self.inner.storage.slots();
        let users = self.inner.users.load(Ordering::Relaxed);
        let (available, waiting) = if users < slots.size {
            (slots.size - users, 0)
        } else {
            (0, users - slots.size)
        };
        Status {
            max_size: slots.max_size,
            size: slots.size,
            available,
            waiting,
        }
    }

    /// Returns [`Manager`] of this [`Pool`].
    #[must_use]
    pub fn manager(&self) -> &M {
        &self.inner.manager
    }

    /// Returns a [`WeakPool<T>`] of this [`Pool`].
    pub fn weak(&self) -> WeakPool<M> {
        WeakPool {
            inner: Arc::downgrade(&self.inner),
            _wrapper: PhantomData,
        }
    }

    fn wrap_object(
        &self,
        inner_obj: ObjectInner<M>,
        #[cfg(feature = "core-local")] local: Option<StdWeak<LocalStorage<ObjectInner<M>>>>,
        #[cfg(not(feature = "core-local"))] _local: Option<()>,
    ) -> Object<M> {
        Object {
            inner: Some(inner_obj),
            pool: self.weak(),
            #[cfg(feature = "core-local")]
            local,
        }
    }
}

/// A weak reference to a [`Pool<T>`], used to avoid keeping the pool alive.
///
/// `WeakPool<T>` is analogous to [`std::sync::Weak<T>`] for [`Pool<T>`], and
/// is typically used in situations where you need a non-owning reference to a pool,
/// such as in background tasks, managers, or callbacks that should not extend
/// the lifetime of the pool.
///
/// This allows components to retain a reference to the pool while avoiding
/// reference cycles or prolonging its lifetime unnecessarily.
///
/// To access the pool, use [`WeakPool::upgrade`] to attempt to get a strong reference.
#[derive(Debug)]
pub struct WeakPool<M: Manager, W: From<Object<M>> = Object<M>> {
    inner: Weak<PoolInner<M>>,
    _wrapper: PhantomData<fn() -> W>,
}

impl<M: Manager, W: From<Object<M>>> WeakPool<M, W> {
    /// Attempts to upgrade the `WeakPool` to a strong [`Pool<T>`] reference.
    ///
    /// If the pool has already been dropped (i.e., no strong references remain),
    /// this returns `None`.
    pub fn upgrade(&self) -> Option<Pool<M, W>> {
        Some(Pool {
            inner: self.inner.upgrade()?,
            _wrapper: PhantomData,
        })
    }
}

pub(crate) struct PoolInner<M: Manager> {
    manager: M,
    next_id: AtomicUsize,
    /// Number of [`Pool`] users. A user is both a future which is waiting for an [`Object`] or one
    /// with an [`Object`] which hasn't been returned, yet.
    users: AtomicUsize,
    storage: PoolStorage<ObjectInner<M>>,
    config: PoolConfig,
    runtime: Option<Runtime>,
    hooks: Hooks<M>,
}

// Implemented manually to avoid unnecessary trait bound on the struct.
impl<M> fmt::Debug for PoolInner<M>
where
    M: fmt::Debug + Manager,
    M::Type: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoolInner")
            .field("manager", &self.manager)
            .field("used", &self.users)
            .field("storage", &self.storage)
            .field("config", &self.config)
            .field("runtime", &self.runtime)
            .field("hooks", &self.hooks)
            .finish()
    }
}

/// Local handle for a managed [`Pool`].
///
/// Local handles are the opt-in API surface for core-local storage. Successful
/// same-handle returns are stored in the handle's local FIFO queue. The
/// configured [`QueueMode`] continues to apply to objects retrieved from the
/// shared fallback queue.
#[cfg(feature = "core-local")]
#[cfg_attr(docsrs, doc(cfg(feature = "core-local")))]
pub struct LocalPool<M: Manager, W: From<Object<M>> = Object<M>> {
    pool: Pool<M, W>,
    local: Option<Arc<LocalStorage<ObjectInner<M>>>>,
}

#[cfg(feature = "core-local")]
impl<M: Manager, W: From<Object<M>>> Clone for LocalPool<M, W> {
    fn clone(&self) -> Self {
        if let Some(local) = &self.local {
            local.clone_owner();
        }
        Self {
            pool: self.pool.clone(),
            local: self.local.clone(),
        }
    }
}

#[cfg(feature = "core-local")]
impl<M: Manager, W: From<Object<M>>> Drop for LocalPool<M, W> {
    fn drop(&mut self) {
        let Some(local) = &self.local else {
            return;
        };
        if local.release_owner() {
            local.close();
            self.pool.inner.drain_local_storage(local);
        }
    }
}

#[cfg(feature = "core-local")]
impl<M, W> fmt::Debug for LocalPool<M, W>
where
    M: fmt::Debug + Manager,
    M::Type: fmt::Debug,
    W: From<Object<M>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalPool")
            .field("pool", &self.pool)
            .field("local", &self.local)
            .finish()
    }
}

#[cfg(feature = "core-local")]
impl<M: Manager, W: From<Object<M>>> LocalPool<M, W> {
    /// Returns the shared pool backing this local handle.
    #[must_use]
    pub fn pool(&self) -> &Pool<M, W> {
        &self.pool
    }

    /// Returns the configured [`PoolMode`].
    #[must_use]
    pub fn pool_mode(&self) -> PoolMode {
        self.pool.pool_mode()
    }

    /// Retrieves an [`Object`] from this local handle.
    ///
    /// In [`PoolMode::CoreLocal`], this first checks the handle's local FIFO
    /// queue and otherwise uses shared pool capacity. In [`PoolMode::Shared`],
    /// this delegates to [`Pool::get`].
    ///
    /// # Errors
    ///
    /// See [`PoolError`] for details.
    pub async fn get(&self) -> Result<W, PoolError<M::Error>> {
        self.timeout_get(&self.pool.timeouts()).await
    }

    /// Retrieves an [`Object`] from this local handle using custom timeouts.
    ///
    /// In [`PoolMode::CoreLocal`], this first checks the handle's local FIFO
    /// queue and otherwise uses shared pool capacity with the supplied
    /// `timeouts`. In [`PoolMode::Shared`], this delegates to
    /// [`Pool::timeout_get`].
    ///
    /// # Errors
    ///
    /// See [`PoolError`] for details.
    pub async fn timeout_get(&self, timeouts: &Timeouts) -> Result<W, PoolError<M::Error>> {
        match &self.local {
            Some(local) => self.pool.timeout_get_for_local(timeouts, local).await,
            None => self.pool.timeout_get(timeouts).await,
        }
    }

    /// Retrieves [`Status`] of the backing pool.
    #[must_use]
    pub fn status(&self) -> Status {
        self.pool.status()
    }

    /// Returns how often this local handle had to wait for local availability.
    #[must_use]
    #[doc(hidden)]
    pub fn local_wait_count(&self) -> usize {
        self.local.as_ref().map_or(0, |local| local.waits())
    }
}

impl<M: Manager> PoolInner<M> {
    pub(crate) fn return_object(&self, mut inner: ObjectInner<M>) {
        let _ = self.users.fetch_sub(1, Ordering::Relaxed);
        let mut slots = self.storage.slots();
        if slots.size <= slots.max_size {
            slots.vec.push_back(inner);
            drop(slots);
            self.storage.semaphore().add_permits(1);
            #[cfg(feature = "core-local")]
            self.notify_local_waiters();
        } else {
            slots.size -= 1;
            drop(slots);
            self.manager.detach(&mut inner.obj);
        }
    }
    pub(crate) fn detach_object(&self, obj: &mut M::Type) {
        let _ = self.users.fetch_sub(1, Ordering::Relaxed);
        let mut slots = self.storage.slots();
        let add_permits = slots.size <= slots.max_size;
        slots.size -= 1;
        drop(slots);
        if add_permits {
            self.storage.semaphore().add_permits(1);
            #[cfg(feature = "core-local")]
            self.notify_local_waiters();
        }
        self.manager.detach(obj);
    }

    #[cfg(feature = "core-local")]
    pub(crate) fn return_object_to_local(
        &self,
        local: &LocalStorage<ObjectInner<M>>,
        inner: ObjectInner<M>,
    ) {
        let _ = self.users.fetch_sub(1, Ordering::Relaxed);
        if self.storage.semaphore().is_closed() {
            self.discard_idle_object(inner);
            return;
        }
        let slots = self.storage.slots();
        let keep = slots.size <= slots.max_size;
        drop(slots);
        if keep && local.is_active() {
            if let Err(inner) = local.push(inner) {
                self.discard_idle_object(inner);
            }
        } else {
            self.discard_idle_object(inner);
        }
    }

    #[cfg(feature = "core-local")]
    pub(crate) fn detach_object_inner(&self, inner: ObjectInner<M>) {
        let _ = self.users.fetch_sub(1, Ordering::Relaxed);
        self.discard_idle_object(inner);
    }

    #[cfg(feature = "core-local")]
    fn drain_local_objects(&self) {
        for local in self.storage.local_storages() {
            local.close();
            self.drain_local_storage(&local);
        }
    }

    #[cfg(feature = "core-local")]
    fn drain_local_storage(&self, local: &LocalStorage<ObjectInner<M>>) {
        for inner in local.drain() {
            self.discard_idle_object(inner);
        }
    }

    #[cfg(feature = "core-local")]
    fn discard_local_objects_over_max_size(&self) {
        for local in self.storage.local_storages() {
            loop {
                let over_max_size = {
                    let slots = self.storage.slots();
                    slots.size > slots.max_size
                };
                if !over_max_size {
                    break;
                }
                let Some(inner) = local.pop() else {
                    break;
                };
                self.discard_idle_object(inner);
            }
        }
    }

    #[cfg(feature = "core-local")]
    fn retain_local_objects(
        &self,
        predicate: &mut impl FnMut(&M::Type, Metrics) -> bool,
        removed: &mut Vec<M::Type>,
    ) -> usize {
        let mut retained = 0;
        for local in self.storage.local_storages() {
            let mut keep = Vec::new();
            for mut inner in local.drain() {
                if predicate(&inner.obj, inner.metrics) {
                    retained += 1;
                    keep.push(inner);
                } else {
                    let mut slots = self.storage.slots();
                    let add_permit =
                        !self.storage.semaphore().is_closed() && slots.size <= slots.max_size;
                    slots.size -= 1;
                    drop(slots);
                    if add_permit {
                        self.storage.semaphore().add_permits(1);
                        self.notify_local_waiters();
                    }
                    self.manager.detach(&mut inner.obj);
                    removed.push(inner.obj);
                }
            }
            for inner in keep {
                if let Err(inner) = local.push(inner) {
                    self.discard_idle_object(inner);
                }
            }
        }
        retained
    }

    #[cfg(feature = "core-local")]
    fn discard_idle_object(&self, mut inner: ObjectInner<M>) {
        let mut slots = self.storage.slots();
        let add_permit = !self.storage.semaphore().is_closed() && slots.size <= slots.max_size;
        slots.size -= 1;
        drop(slots);
        if add_permit {
            self.storage.semaphore().add_permits(1);
            self.notify_local_waiters();
        }
        self.manager.detach(&mut inner.obj);
    }

    #[cfg(feature = "core-local")]
    fn notify_local_waiters(&self) {
        for local in self.storage.local_storages() {
            local.signal();
        }
    }
}

struct UnreadyObject<'a, M: Manager> {
    inner: Option<ObjectInner<M>>,
    pool: &'a PoolInner<M>,
}

impl<M: Manager> UnreadyObject<'_, M> {
    fn ready(mut self) -> ObjectInner<M> {
        self.inner.take().unwrap()
    }
    fn inner(&mut self) -> &mut ObjectInner<M> {
        self.inner.as_mut().unwrap()
    }
}

impl<M: Manager> Drop for UnreadyObject<'_, M> {
    fn drop(&mut self) {
        if let Some(mut inner) = self.inner.take() {
            self.pool.storage.slots().size -= 1;
            self.pool.manager.detach(&mut inner.obj);
        }
    }
}

#[cfg(feature = "core-local")]
struct UnreadyLocalObject<'a, M: Manager> {
    inner: Option<ObjectInner<M>>,
    pool: &'a PoolInner<M>,
}

#[cfg(feature = "core-local")]
impl<M: Manager> UnreadyLocalObject<'_, M> {
    fn ready(mut self) -> ObjectInner<M> {
        self.inner.take().unwrap()
    }
    fn inner(&mut self) -> &mut ObjectInner<M> {
        self.inner.as_mut().unwrap()
    }
}

#[cfg(feature = "core-local")]
impl<M: Manager> Drop for UnreadyLocalObject<'_, M> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            self.pool.discard_idle_object(inner);
        }
    }
}

async fn apply_timeout<O, E>(
    runtime: Option<Runtime>,
    timeout_type: TimeoutType,
    duration: Option<Duration>,
    future: impl Future<Output = Result<O, impl Into<PoolError<E>>>>,
) -> Result<O, PoolError<E>> {
    match (runtime, duration) {
        (_, None) => future.await.map_err(Into::into),
        (Some(runtime), Some(duration)) => timeout(runtime, duration, future)
            .await
            .ok_or(PoolError::Timeout(timeout_type))?
            .map_err(Into::into),
        (None, Some(_)) => Err(PoolError::NoRuntimeSpecified),
    }
}

#[derive(Debug)]
/// This is the result returned by `Pool::retain`
pub struct RetainResult<T> {
    /// Number of retained objects
    pub retained: usize,
    /// Objects that were removed from the pool
    pub removed: Vec<T>,
}

impl<T> Default for RetainResult<T> {
    fn default() -> Self {
        Self {
            retained: Default::default(),
            removed: Default::default(),
        }
    }
}
