---
date: "2026-06-09T02:15:49.889+00:00"
git_commit: "9c436bd7dc14ffddaf0b585e0850ac76b1380680"
branch: "users/andrewkhoma/lockfree"
repository: "andrewkhoma/deadpool"
topic: "Lockfree Deadpool code research"
tags: [research, codebase, deadpool, managed-pool, unmanaged-pool, deadpool-postgres, core-local-pool, gateway-reference]
status: complete
last_updated: "2026-06-09"
---

# Research: Lockfree Deadpool

## Research Question

Where and how do the current Deadpool managed pool, unmanaged pool, `deadpool-postgres` integration, feature flags, verification workflows, benchmarks, examples, local core-local pool reference, and gateway reference implement behaviors relevant to opt-in lockfree/thread-per-core pool behavior while preserving existing compatibility? (`.paw/work/lockfree-deadpool/Spec.md:80-94`, `.paw/work/lockfree-deadpool/ResearchQuestions.md:17-26`)

## Summary

- The `deadpool` crate exposes feature-gated managed and unmanaged modules, reexports `deadpool_runtime::Runtime`, and documents `Status` as eventually consistent under load (`crates/deadpool/src/lib.rs:24-32`, `crates/deadpool/src/lib.rs:34-56`).
- Managed pools are `Arc`-backed, use a `tokio::sync::Semaphore` for checkout capacity/backpressure, and store idle objects in a `Mutex<VecDeque<...>>` governed by `QueueMode` (`crates/deadpool/src/managed/pool.rs:67-82`, `crates/deadpool/src/managed/pool.rs:103-158`, `crates/deadpool/src/managed/config.rs:106-118`).
- Managed checked-out objects return through `Drop`, detach through `Object::take`, and call pool internals that update user counts, slot size, idle queue, permits, and manager detach hooks (`crates/deadpool/src/managed/object.rs:50-60`, `crates/deadpool/src/managed/object.rs:85-93`, `crates/deadpool/src/managed/pool.rs:454-479`).
- Unmanaged pools use separate semaphores for object availability and size capacity, store objects in a `Mutex<Vec<T>>`, and expose `get`, `try_get`, `timeout_get`, `add`, `try_add`, `remove`, close, status, and object take/return paths (`crates/deadpool/src/unmanaged/mod.rs:152-247`, `crates/deadpool/src/unmanaged/mod.rs:249-353`, `crates/deadpool/src/unmanaged/mod.rs:355-425`).
- `deadpool-postgres` depends on the managed pool, reexports the standard managed aliases, wraps `tokio_postgres::Client` in `ClientWrapper`, tracks statement caches, and delegates connection creation/recycling through a `Manager` (`crates/deadpool-postgres/src/lib.rs:39-70`, `crates/deadpool-postgres/src/lib.rs:80-177`, `crates/deadpool-postgres/src/lib.rs:233-406`, `crates/deadpool-postgres/src/lib.rs:408-490`).
- The reference `.paw/documentdb_core_local_pool` crate models one `CoreLocalPool` per core with shared `GlobalPermits`, local idle state, acquire/release/discard/status/prune APIs, permit accounting, and PostgreSQL session-reset behavior (`.paw/documentdb_core_local_pool/src/lib.rs:15-33`, `.paw/documentdb_core_local_pool/src/pool.rs:47-137`, `.paw/documentdb_core_local_pool/src/pool.rs:168-395`, `.paw/documentdb_core_local_pool/src/connection.rs:80-132`).
- The gateway reference uses Deadpool-backed PostgreSQL pools through `ConnectionPool`, builds primary `Fast` and timeout `Clean` pools, records acquisition metrics, combines status from both pools, prunes by `retain`, and stores logical pools in `PoolManager` maps (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:122-184`, `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:194-327`, `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/pool_manager.rs:40-52`, `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/pool_manager.rs:87-153`).

## Documentation System

- **Framework**: Plain Markdown crate documentation plus rustdoc; each relevant crate includes its README as crate docs with `#![doc = include_str!("../README.md")]` (`crates/deadpool/src/lib.rs:1`, `crates/deadpool-postgres/src/lib.rs:1`, `crates/deadpool-runtime/src/lib.rs:1`).
- **Docs Directory**: No dedicated `docs/` navigation tree is present in the repository root listing; documentation surfaces observed for this work are crate READMEs and CHANGELOGs (`crates/deadpool/README.md:1-205`, `crates/deadpool/CHANGELOG.md:1-14`, `crates/deadpool-postgres/README.md:1-195`, `crates/deadpool-postgres/CHANGELOG.md:1-14`).
- **Navigation Config**: No MkDocs/Docusaurus/Sphinx navigation config is used for the mapped crates; docs.rs metadata is configured per crate in Cargo manifests (`crates/deadpool/Cargo.toml:13-15`, `crates/deadpool-postgres/Cargo.toml:13-15`, `crates/deadpool-runtime/Cargo.toml:15-17`).
- **Style Conventions**: READMEs use badge headers, feature tables, Rust examples, FAQ sections, and license sections (`crates/deadpool/README.md:1-35`, `crates/deadpool/README.md:36-83`, `crates/deadpool/README.md:185-205`, `crates/deadpool-postgres/README.md:1-23`, `crates/deadpool-postgres/README.md:24-128`, `crates/deadpool-postgres/README.md:130-195`). CHANGELOGs use Keep a Changelog and Semantic Versioning headings (`crates/deadpool/CHANGELOG.md:1-14`).
- **Build Command**: `cargo doc --no-deps --all-features` for `crates/deadpool`; `cargo doc --no-deps --features serde,rt_tokio_1,rt_async-std_1` for `crates/deadpool-postgres` (`.github/workflows/deadpool.yml:59-70`, `.github/workflows/deadpool-postgres.yml:92-103`).
- **Standard Files**: Core files for this scope are `crates/deadpool/README.md`, `crates/deadpool/CHANGELOG.md`, `crates/deadpool/Cargo.toml`, `crates/deadpool-postgres/README.md`, `crates/deadpool-postgres/CHANGELOG.md`, `crates/deadpool-postgres/Cargo.toml`, and release replacement config in `release.toml` (`crates/deadpool/README.md:1-205`, `crates/deadpool/CHANGELOG.md:1-14`, `crates/deadpool/Cargo.toml:1-55`, `crates/deadpool-postgres/README.md:1-195`, `crates/deadpool-postgres/Cargo.toml:1-70`, `release.toml:1-6`).

## Verification Commands

- **Core test command**: `cd crates/deadpool && cargo test --all-features` (`.github/workflows/deadpool.yml:83-96`).
- **Core lint commands**: `cd crates/deadpool && cargo clippy --no-deps --all-features -- -D warnings`; `cd crates/deadpool && cargo fmt --check` (`.github/workflows/deadpool.yml:30-41`, `.github/workflows/deadpool.yml:71-82`).
- **Core build/type-check commands**: `cd crates/deadpool && cargo check --no-default-features --features managed,rt_tokio_1` and matrix variants for `managed|unmanaged` crossed with `rt_tokio_1|rt_async-std_1|serde`; `cd crates/deadpool && cargo check --all-features` for MSRV (`.github/workflows/deadpool.yml:8-29`, `.github/workflows/deadpool.yml:42-58`).
- **Core docs command**: `cd crates/deadpool && cargo doc --no-deps --all-features` (`.github/workflows/deadpool.yml:59-70`).
- **PostgreSQL test command**: `cd crates/deadpool-postgres && cargo test --features serde,rt_tokio_1,rt_async-std_1` with PostgreSQL 17 service and `PG__*` environment (`.github/workflows/deadpool-postgres.yml:116-143`).
- **PostgreSQL lint commands**: `cd crates/deadpool-postgres && cargo clippy --no-deps --features serde,rt_tokio_1,rt_async-std_1 -- -D warnings`; `cd crates/deadpool-postgres && cargo fmt --check` (`.github/workflows/deadpool-postgres.yml:63-74`, `.github/workflows/deadpool-postgres.yml:104-115`).
- **PostgreSQL build/type-check commands**: `cd crates/deadpool-postgres && cargo check --features serde|rt_tokio_1|rt_async-std_1` on Ubuntu/Windows; `cd crates/deadpool-postgres && cargo check --no-default-features --features rt_tokio_1 --target wasm32-unknown-unknown`; `cd crates/deadpool-postgres && ../../tools/check-reexported-features.sh` (`.github/workflows/deadpool-postgres.yml:8-29`, `.github/workflows/deadpool-postgres.yml:30-48`, `.github/workflows/deadpool-postgres.yml:49-62`).
- **PostgreSQL docs command**: `cd crates/deadpool-postgres && cargo doc --no-deps --features serde,rt_tokio_1,rt_async-std_1` (`.github/workflows/deadpool-postgres.yml:92-103`).
- **Runtime crate commands**: `cd crates/deadpool-runtime && cargo clippy --no-deps --all-features -- -D warnings`, `cargo check --all-features`, `cargo doc --no-deps --all-features`, `cargo fmt --check`, and `cargo test --all-features` are defined in the runtime workflow (`.github/workflows/deadpool-runtime.yml:8-74`).

## Detailed Findings

### Crate Boundaries, Runtime, and Feature Flags

- `deadpool` gates `managed` and `unmanaged` modules behind `managed` and `unmanaged` features, respectively, and reexports `deadpool_runtime::Runtime` at crate root (`crates/deadpool/src/lib.rs:24-32`).
- `deadpool::Status` contains `max_size`, `size`, `available`, and `waiting`; its docs state that reported numbers are not guaranteed consistent under heavy load and are meant for overall insight (`crates/deadpool/src/lib.rs:34-56`).
- `crates/deadpool/Cargo.toml` sets default features to `managed` and `unmanaged`, declares runtime timeout features `rt_tokio_1`, `rt_async-std_1`, and `rt_smol_2`, and lists `serde` as an optional dependency (`crates/deadpool/Cargo.toml:17-27`).
- `crates/deadpool/Cargo.toml` documents that `tokio::sync::Semaphore` is a non-optional dependency via `tokio` with the `sync` feature, while no other Tokio features are enabled unless the Tokio runtime feature is selected (`crates/deadpool/Cargo.toml:29-33`).
- Default pool maximum size is computed as cached logical CPU count times two (`crates/deadpool/src/util.rs:1-16`), and both managed and unmanaged configs use this default (`crates/deadpool/src/managed/config.rs:50-55`, `crates/deadpool/src/unmanaged/config.rs:47-52`).
- `deadpool-runtime` exposes `Runtime::{Tokio1, AsyncStd1, Smol2}` behind runtime-specific features and implements `timeout` by dispatching to Tokio, async-std, or smol/futures-lite timeout behavior (`crates/deadpool-runtime/src/lib.rs:26-77`, `crates/deadpool-runtime/Cargo.toml:19-41`).
- `deadpool-postgres` defaults to `rt_tokio_1`, maps `rt_tokio_1`, `rt_async-std_1`, and `serde` to `deadpool` features, and reexports tokio-postgres feature names such as `with-chrono-0_4`, `with-serde_json-1`, and `with-uuid-1` (`crates/deadpool-postgres/Cargo.toml:17-44`).
- `deadpool-postgres` depends on `deadpool` with `default-features = false` and `features = ["managed"]`, plus `tokio`, `tracing`, `tokio-postgres`, and `async-trait` (`crates/deadpool-postgres/Cargo.toml:47-70`).

### Managed Pool Public API and Module Layout

- The managed module describes a manager-created/recycled pool and reexports `BuildError`, `PoolBuilder`, `CreatePoolError`, `PoolConfig`, `QueueMode`, `Timeouts`, `PoolError`, `RecycleError`, `TimeoutType`, hooks, `Manager`, `RecycleResult`, `Metrics`, `Object`, `ObjectId`, `Pool`, `RetainResult`, and `WeakPool` (`crates/deadpool/src/managed/mod.rs:1-73`).
- `Manager` requires `Sync + Send`, `Type: Send`, `Error: Send`, async `create`, async `recycle`, and optional `detach` invoked when an object is permanently removed from a pool (`crates/deadpool/src/managed/manager.rs:5-37`).
- `managed_reexports!` generates standard backend aliases for `Pool`, `WeakPool`, `PoolBuilder`, `BuildError`, `CreatePoolError`, `PoolError`, `Object`, `Hook`, `HookError`, and `QueueMode` (`crates/deadpool/src/managed/reexports.rs:22-60`).

### Managed Pool Builder and Configuration

- `PoolConfig` stores `max_size`, `Timeouts`, and `QueueMode`; defaults are no timeouts, default max size, and FIFO queue mode (`crates/deadpool/src/managed/config.rs:5-56`, `crates/deadpool/src/managed/config.rs:58-118`).
- `Timeouts` has separate `wait`, `create`, and `recycle` durations; `Timeouts::new` sets all to `None`, and `wait_millis` sets only the wait timeout (`crates/deadpool/src/managed/config.rs:58-104`).
- `QueueMode::Fifo` dequeues the least recently added object and `QueueMode::Lifo` dequeues the most recently added object (`crates/deadpool/src/managed/config.rs:106-118`).
- `PoolBuilder` stores manager, config, optional runtime, hooks, and wrapper marker; `PoolBuilder::build` returns `BuildError::NoRuntimeSpecified` when any timeout is configured without a runtime (`crates/deadpool/src/managed/builder.rs:37-98`).
- Builder methods set full config, max size, timeouts, individual wait/create/recycle timeouts, queue mode, hooks, and runtime (`crates/deadpool/src/managed/builder.rs:100-187`).
- `BuildError::NoRuntimeSpecified` is the managed builder error for timeout configuration without runtime (`crates/deadpool/src/managed/builder.rs:10-35`).

### Managed Pool Checkout, Queue, Semaphore, and Permit Flow

- `Pool::from_builder` constructs `PoolInner` inside an `Arc` with manager, `AtomicUsize` IDs, `Mutex<Slots<VecDeque<ObjectInner>>>`, `users` counter, a Tokio `Semaphore` initialized to `max_size`, config, hooks, and runtime (`crates/deadpool/src/managed/pool.rs:67-82`, `crates/deadpool/src/managed/pool.rs:415-433`).
- `Pool::get` delegates to `timeout_get` using the configured timeouts (`crates/deadpool/src/managed/pool.rs:87-95`).
- `timeout_get` increments `users`, installs a drop guard to decrement it on early exit, treats a zero wait timeout as non-blocking, and maps semaphore `try_acquire` errors to `Closed` or `Timeout(Wait)` (`crates/deadpool/src/managed/pool.rs:103-119`).
- For non-zero/absent wait timeout, `timeout_get` wraps `semaphore.acquire().await` in `apply_timeout`, mapping semaphore closure to `PoolError::Closed` (`crates/deadpool/src/managed/pool.rs:119-133`).
- After acquiring a permit, `timeout_get` pops an idle object from `slots.vec` using FIFO or LIFO under the slots mutex, recycles popped objects, creates new objects when no idle object is present, and loops until it has an object (`crates/deadpool/src/managed/pool.rs:135-148`).
- On successful checkout, `timeout_get` disarms the users guard, calls `permit.forget()`, and returns an `Object` containing the inner object and weak pool reference (`crates/deadpool/src/managed/pool.rs:150-158`).
- `apply_timeout` runs a future directly when no duration is configured, dispatches to `deadpool_runtime::timeout` when both runtime and duration exist, maps elapsed timeout to `PoolError::Timeout(timeout_type)`, and returns `PoolError::NoRuntimeSpecified` when a duration has no runtime (`crates/deadpool/src/managed/pool.rs:504-518`).

### Managed Manager, Create, Recycle, Hooks, and Metrics

- `try_recycle` wraps the object in `UnreadyObject`, applies `pre_recycle` hooks, runs `manager.recycle` under the configured recycle timeout, applies `post_recycle` hooks, increments `metrics.recycle_count`, updates `metrics.recycled` on non-wasm targets, and returns the object when all steps complete (`crates/deadpool/src/managed/pool.rs:160-203`).
- `try_recycle` returns `Ok(None)` when pre-recycle hook application fails, manager recycle times out/errors, or post-recycle hook application fails; `UnreadyObject` cleanup handles the unrecovered inner object (`crates/deadpool/src/managed/pool.rs:172-194`, `crates/deadpool/src/managed/pool.rs:481-501`).
- `try_create` constructs `ObjectInner` from `manager.create` under the create timeout, assigns a monotonically increasing ID, initializes metrics, increments `slots.size`, applies post-create hooks, and returns `PoolError::PostCreateHook` if post-create hook application fails (`crates/deadpool/src/managed/pool.rs:205-239`).
- `UnreadyObject::drop` decrements `slots.size` and calls `manager.detach` for an inner object that did not become ready (`crates/deadpool/src/managed/pool.rs:481-501`).
- `Metrics` stores creation instant, last recycled instant, and recycle count on non-wasm targets, and exposes `age` and `last_used` helpers (`crates/deadpool/src/managed/metrics.rs:1-41`).
- Hooks support sync and async callbacks over mutable manager object plus metrics, return `HookResult`, and are stored in `HookVec` collections for post-create, pre-recycle, and post-recycle application (`crates/deadpool/src/managed/hooks.rs:9-48`, `crates/deadpool/src/managed/hooks.rs:102-169`).
- Managed errors distinguish recycle failures (`RecycleError::Message` or `Backend`), timeout phase (`TimeoutType::{Wait, Create, Recycle}`), pool errors (`Timeout`, `Backend`, `Closed`, `NoRuntimeSpecified`, `PostCreateHook`), and display/source mappings (`crates/deadpool/src/managed/errors.rs:5-123`).

### Managed Object Drop, Take, Return, Detach, Close, Status, Resize, and Retain

- `Object` wraps an optional `ObjectInner` and a `WeakPool`, implements `Deref`, `DerefMut`, `AsRef`, and `AsMut`, and its docs state `Drop` returns it to the pool when it leaves scope (`crates/deadpool/src/managed/object.rs:8-20`, `crates/deadpool/src/managed/object.rs:95-118`).
- `Object::take` removes the inner object permanently, upgrades the pool if possible, calls `detach_object`, and returns the manager object (`crates/deadpool/src/managed/object.rs:50-60`).
- `Drop for Object` takes the inner object and, if the weak pool upgrades, calls `pool.inner.return_object(inner)` (`crates/deadpool/src/managed/object.rs:85-93`).
- `return_object` decrements users, locks slots, pushes the object to the idle `VecDeque` and adds one semaphore permit when `slots.size <= slots.max_size`, or decrements size and calls `manager.detach` when the object exceeds current max size (`crates/deadpool/src/managed/pool.rs:454-467`).
- `detach_object` decrements users, decrements slots size, conditionally adds a permit when size is within max, and calls `manager.detach` (`crates/deadpool/src/managed/pool.rs:468-479`).
- `resize` returns early for closed pools, locks slots, updates max size, shrinks by acquiring permits and popping idle front objects while size exceeds max, rebuilds `VecDeque` capacity, and grows by reserving additional capacity and adding semaphore permits (`crates/deadpool/src/managed/pool.rs:241-280`).
- `retain` locks the slots queue, evaluates a predicate over object and metrics, removes non-retained idle objects, calls manager detach for removed objects, decrements size, and returns `RetainResult` with retained count and removed objects (`crates/deadpool/src/managed/pool.rs:282-329`, `crates/deadpool/src/managed/pool.rs:520-536`).
- `close` resizes to zero then closes the semaphore; `is_closed` returns semaphore closure status; `status` locks slots, reads users, computes available/waiting from users and size, and returns `Status` (`crates/deadpool/src/managed/pool.rs:331-368`).
- `Pool::manager` exposes the manager reference, and `Pool::weak` returns a weak reference wrapper that can later be upgraded (`crates/deadpool/src/managed/pool.rs:370-412`).

### Managed Tests

- `crates/deadpool/tests/managed.rs` covers basic status growth and drop-return availability, close behavior for waiters and future gets, multi-threaded concurrent checkout/checkin, `Object::take`, and `retain`/`FnMut` predicate behavior (`crates/deadpool/tests/managed.rs:26-228`).
- `managed_timeout.rs` covers wait/create/recycle timeout configuration across enabled runtimes and expects a timeout error from `pool.get()` when create/recycle futures never complete (`crates/deadpool/tests/managed_timeout.rs:1-79`).
- `managed_resize.rs` covers shrink/grow behavior, borrowed objects across shrink/grow, concurrent grow from zero while a getter waits, and close-plus-resize status (`crates/deadpool/tests/managed_resize.rs:24-148`).
- `managed_unreliable_manager.rs` covers creation failure status remaining at zero and recycle failure discarding existing objects through manager detach accounting (`crates/deadpool/tests/managed_unreliable_manager.rs:1-98`).
- `managed_cancellation.rs` builds manager/hook combinations with ok/error/slow/never gates, spawns cancellation loops, and asserts size and available do not exceed max size after cancellations (`crates/deadpool/tests/managed_cancellation.rs:7-164`).
- `managed_hooks.rs` covers sync/async post-create hooks, post-create errors, pre-recycle success/error continuation paths, and post-recycle success/error continuation paths (`crates/deadpool/tests/managed_hooks.rs:32-184`).
- `managed_deadlock.rs` covers a drained single-connection pool where one create fails and a second waiter later creates successfully (`crates/deadpool/tests/managed_deadlock.rs:75-120`).
- `managed_config.rs` covers serde/config environment deserialization for pool max size and wait/create/recycle timeouts (`crates/deadpool/tests/managed_config.rs:1-68`).

### Unmanaged Pool Public API, Capacity Accounting, and Checkout/Checkin Paths

- The unmanaged module describes pools whose objects are user-created or added through `Pool::add`/`try_add`, and it reexports `PoolConfig`, `PoolError`, and `Status` (`crates/deadpool/src/unmanaged/mod.rs:1-49`).
- `Object<T>` stores an optional object and a weak pool; `Object::take` upgrades the pool, decrements `size`, adds a size semaphore permit, and returns the object (`crates/deadpool/src/unmanaged/mod.rs:51-78`).
- `Drop for Object<T>` pushes the object back into the pool queue under `queue` mutex, increments `available`, adds an object semaphore permit, and invokes cleanup for closed pools (`crates/deadpool/src/unmanaged/mod.rs:80-94`).
- `Pool::from_config` initializes queue capacity, `size` counter, `size_semaphore` with `max_size`, `available` as zero, and object availability semaphore as zero (`crates/deadpool/src/unmanaged/mod.rs:152-172`).
- `Pool::get` delegates to `timeout_get` using configured timeout (`crates/deadpool/src/unmanaged/mod.rs:174-182`).
- `try_get` tries the object availability semaphore, maps no permits to `PoolError::Timeout` and closure to `Closed`, pops an object from the queue under mutex, forgets the permit, decrements available, and returns an `Object` with weak pool reference (`crates/deadpool/src/unmanaged/mod.rs:184-207`).
- `timeout_get` handles four cases: no timeout waits on `semaphore.acquire`, zero timeout uses `try_acquire`, non-zero timeout with runtime uses `deadpool_runtime::timeout`, and timeout without runtime returns `NoRuntimeSpecified`; it then pops the object, forgets the permit, decrements available, and returns the wrapper (`crates/deadpool/src/unmanaged/mod.rs:209-247`).
- `add` awaits `size_semaphore.acquire`, forgets the permit, calls `_add`, and maps closed semaphore to `(object, PoolError::Closed)` (`crates/deadpool/src/unmanaged/mod.rs:249-267`).
- `try_add` uses `size_semaphore.try_acquire`, maps no size permits to `(object, PoolError::Timeout)` and closure to `(object, PoolError::Closed)`, and calls `_add` on success (`crates/deadpool/src/unmanaged/mod.rs:269-288`).
- `_add` increments size, pushes the object under the queue mutex, increments available, and adds one object availability permit (`crates/deadpool/src/unmanaged/mod.rs:290-303`).
- `remove`, `try_remove`, and `timeout_remove` are implemented by `get`/`try_get`/`timeout_get` followed by `Object::take` (`crates/deadpool/src/unmanaged/mod.rs:305-319`).
- `close` closes both semaphores and clears queued objects; `status` reads max size, size, and signed available to derive available/waiting fields (`crates/deadpool/src/unmanaged/mod.rs:321-353`).
- `PoolInner` stores config, `Mutex<Vec<T>>` queue, `AtomicUsize` size, `size_semaphore`, signed `available`, and object availability `Semaphore`; its `clear` method decrements size and available by queued length and clears the queue (`crates/deadpool/src/unmanaged/mod.rs:355-403`).
- `From<I> for Pool<T>` builds a pool from an exact-size iterator with queue preloaded, size initialized to length, size semaphore zero, available equal to length, and object semaphore initialized to length (`crates/deadpool/src/unmanaged/mod.rs:405-425`).
- Unmanaged config stores max size, optional timeout, and optional runtime; docs state configured timeouts require a runtime or `PoolError::NoRuntimeSpecified` is returned (`crates/deadpool/src/unmanaged/config.rs:5-33`).
- Unmanaged errors are `Timeout`, `Closed`, and `NoRuntimeSpecified`, with display strings for waiting timeout, closed pool, and missing runtime (`crates/deadpool/src/unmanaged/errors.rs:1-35`).

### Unmanaged Tests

- `unmanaged.rs` covers basic status as objects are checked out, close behavior with a waiter and future `get`/`try_get`, multi-threaded concurrent checkout/checkin, add/remove capacity behavior, `try_add`/`try_remove`, and a blocked add completing after removal (`crates/deadpool/tests/unmanaged.rs:1-180`).
- `unmanaged_timeout.rs` covers timeout without runtime, configured timeout without runtime, runtime-backed `timeout_get`, runtime-backed config timeout, and Tokio/async-std/smol feature variants (`crates/deadpool/tests/unmanaged_timeout.rs:1-83`).

### `deadpool-postgres` Integration

- `deadpool-postgres` imports `deadpool::managed`, reexports `tokio_postgres`, reexports config enums/types, reexports `GenericClient`, and invokes `deadpool::managed_reexports!` to publish aliases against its PostgreSQL `Manager` and `Object` (`crates/deadpool-postgres/src/lib.rs:39-70`).
- `Client` is a type alias for the managed `Object`, and internal recycle result/error aliases map to `managed::RecycleResult<tokio_postgres::Error>` and `managed::RecycleError<tokio_postgres::Error>` (`crates/deadpool-postgres/src/lib.rs:72-79`).
- PostgreSQL `Manager` stores `ManagerConfig`, `tokio_postgres::Config`, boxed `Connect`, and public `statement_caches` (`crates/deadpool-postgres/src/lib.rs:80-89`).
- `Manager::new`, `from_config`, and `from_connect` build managers from a PostgreSQL config plus TLS or connect implementation and initialize `StatementCaches::default` (`crates/deadpool-postgres/src/lib.rs:91-131`).
- `managed::Manager for Manager` creates a client via `connect.connect`, wraps it in `ClientWrapper`, attaches its statement cache, and returns it (`crates/deadpool-postgres/src/lib.rs:145-155`).
- PostgreSQL recycling checks `client.is_closed`, then optionally runs SQL from `RecyclingMethod::query`; closed clients and failing recycle SQL are returned as recycle errors, and `Fast` maps to no SQL (`crates/deadpool-postgres/src/lib.rs:157-172`, `crates/deadpool-postgres/src/config.rs:299-374`).
- `Manager::detach` removes the client wrapper's statement cache from the manager's `StatementCaches` collection (`crates/deadpool-postgres/src/lib.rs:174-176`, `crates/deadpool-postgres/src/lib.rs:240-249`).
- `Connect` abstracts creation of `(PgClient, JoinHandle<()>)`; `ConfigConnectImpl` clones TLS/config, connects via `pg_config.connect(tls)`, spawns the connection future, and returns the client plus task handle (`crates/deadpool-postgres/src/lib.rs:179-231`).
- `StatementCaches` stores weak references in a mutex-protected vector and provides `clear` and `remove` across currently upgradeable per-client caches (`crates/deadpool-postgres/src/lib.rs:233-270`).
- Per-client `StatementCache` stores prepared statements in an `RwLock<HashMap<StatementCacheKey, Statement>>` with an atomic size counter; it exposes `size`, `clear`, `remove`, and cached `prepare`/`prepare_typed` paths (`crates/deadpool-postgres/src/lib.rs:283-406`).
- `ClientWrapper` owns the original `PgClient`, a connection task `JoinHandle`, and an `Arc<StatementCache>`; it exposes cached prepare methods, transaction/build-transaction wrappers that share the statement cache, derefs to the `PgClient`, and aborts the connection task on drop (`crates/deadpool-postgres/src/lib.rs:408-490`).
- `Transaction` and `TransactionBuilder` wrap `tokio_postgres` transaction types while retaining the originating statement cache (`crates/deadpool-postgres/src/lib.rs:492-660`).
- `Config` stores PostgreSQL connection fields, manager config, and optional managed `PoolConfig`; `create_pool` calls `builder`, applies runtime if provided, and builds the managed pool; `builder` constructs a PostgreSQL manager and applies pool config (`crates/deadpool-postgres/src/config.rs:22-118`, `crates/deadpool-postgres/src/config.rs:146-192`).
- `get_pg_config` applies URL parsing, user/password/dbname/options/application name, host/port/address/timeouts/keepalive/SSL fields, default local host behavior, and validates dbname presence/non-empty (`crates/deadpool-postgres/src/config.rs:194-282`).
- `get_manager_config` and `get_pool_config` return provided manager/pool configs or defaults (`crates/deadpool-postgres/src/config.rs:284-297`).
- `RecyclingMethod` variants are `Fast`, `Verified`, `Clean`, and `Custom(String)`; `Clean` maps to a multi-statement reset sequence that avoids `DEALLOCATE ALL` and `DISCARD PLAN` (`crates/deadpool-postgres/src/config.rs:299-374`).
- `GenericClient` is a sealed async trait over query/execute/prepare/cached prepare/transaction/batch APIs, implemented for `Client` and `Transaction` (`crates/deadpool-postgres/src/generic_client.rs:1-88`, `crates/deadpool-postgres/src/generic_client.rs:90-177`, `crates/deadpool-postgres/src/generic_client.rs:179-268`).

### `deadpool-postgres` Tests

- PostgreSQL tests create pools from environment config with `Runtime::Tokio1` and `NoTls` (`crates/deadpool-postgres/tests/postgres.rs:1-43`).
- Tests cover basic cached prepare/query and statement-cache size, typed cached prepare, typed query errors, transaction wrappers, transaction builder settings, generic client compatibility, recycling methods (`Fast`, `Verified`, `Clean`, `Custom`), and per-client/all-client statement cache clearing (`crates/deadpool-postgres/tests/postgres.rs:45-213`).
- Configuration tests cover serde environment loading into PostgreSQL config and pool timeouts, plus URL parsing and override behavior (`crates/deadpool-postgres/tests/postgres.rs:247-312`).
- `async_trait.rs` is a compile test that imports `deadpool_postgres::GenericClient` and defines a trait method returning futures/streams without depending on the async-trait crate directly in user code (`crates/deadpool-postgres/tests/async_trait.rs:1-27`).

### Benchmarks and Examples for Checkout/Checkin/Concurrency

- `crates/deadpool/benches/managed.rs` defines `ITERATIONS = 1 << 15`, worker/pool-size configurations for 8/16/32 workers, a trivial managed manager, and a benchmark where each worker repeatedly calls `pool.get().await` (`crates/deadpool/benches/managed.rs:1-98`).
- `crates/deadpool/benches/unmanaged.rs` defines one million iterations over a pool with one added object and repeatedly calls `pool.get().await.unwrap()` (`crates/deadpool/benches/unmanaged.rs:1-22`).
- The PostgreSQL benchmark README describes comparing 16,000 queries with 16 workers, separate connections versus `deadpool-postgres`, and documents `cargo run --release` plus PostgreSQL environment setup (`examples/postgres-benchmark/README.md:1-70`).
- `examples/postgres-benchmark/src/main.rs` implements `without_pool` by opening a new PostgreSQL connection per iteration, `with_deadpool` by sharing a pool across workers and calling `pool.get().await` plus `prepare_cached`, and asserts the pooled path is faster (`examples/postgres-benchmark/src/main.rs:1-90`).

### Reference Core-Local Pool (`.paw/documentdb_core_local_pool`)

- The reference crate describes itself as a core-local connection pool for thread-per-core single-threaded Tokio runtimes, with each core owning private idle state and shared semaphore capacity across cores (`.paw/documentdb_core_local_pool/src/lib.rs:15-33`).
- Its manifest names `documentdb_core_local_pool`, describes it as a core-local async connection pool, and depends on `tokio`, `tokio-postgres`, and `tracing` workspace dependencies (`.paw/documentdb_core_local_pool/Cargo.toml:1-22`).
- `PoolConfig` stores `max_connections`, `max_idle_per_core`, optional max lifetime and idle timeout, prune interval, acquire timeout, and create timeout; defaults are 20 max connections, 10 max idle per core, 3600s lifetime, 300s idle lifetime, 10s prune interval, 30s acquire timeout, and 10s create timeout (`.paw/documentdb_core_local_pool/src/config.rs:9-57`).
- `PoolError` variants distinguish acquire timeout, create timeout, backend PostgreSQL error, and closed global semaphore (`.paw/documentdb_core_local_pool/src/error.rs:11-52`).
- `GlobalPermits` wraps `Arc<tokio::sync::Semaphore>` and max connection count, exposes new/available/max/close/release/inner methods, and documents that clones are shared across core pools (`.paw/documentdb_core_local_pool/src/pool.rs:47-102`).
- `PoolStatus` reports local idle count, local in-flight count, and global available permits (`.paw/documentdb_core_local_pool/src/pool.rs:104-113`).
- `CoreLocalPool` stores idle `Mutex<VecDeque<PooledConnection>>`, in-flight `Mutex<usize>`, shared `GlobalPermits`, `ConnectionManager`, and `PoolConfig` (`.paw/documentdb_core_local_pool/src/pool.rs:115-137`).
- The pool file documents a design using `std::sync::Mutex` for idle queue and in-flight counter, with the mutex not held across `.await`, and a permit invariant tying semaphore availability plus total connections to max connections (`.paw/documentdb_core_local_pool/src/pool.rs:8-33`).
- `CoreLocalPool::acquire` first calls the synchronous `try_pop_idle` fast path and falls back to `create_new_connection` (`.paw/documentdb_core_local_pool/src/pool.rs:168-190`).
- `acquire_clean` loops over idle connections, discarding broken/expired entries by releasing permits, runs `reset_session` for connections marked dirty, touches and increments in-flight for reusable connections, and falls back to new connection creation (`.paw/documentdb_core_local_pool/src/pool.rs:192-228`).
- `release` decrements in-flight, releases a permit for broken connections or when the idle queue has reached `max_idle_per_core`, otherwise pushes the connection to the idle queue (`.paw/documentdb_core_local_pool/src/pool.rs:230-248`).
- `release_timeout` decrements in-flight, discards broken connections, runs session reset, enforces idle limit, and pushes the connection to idle on success (`.paw/documentdb_core_local_pool/src/pool.rs:250-274`).
- `discard` decrements in-flight and releases one global permit without returning the connection to idle (`.paw/documentdb_core_local_pool/src/pool.rs:276-282`).
- `status` locks idle and in-flight state and reports global permits; `prune` retains non-broken/non-expired/non-idle-expired connections and releases permits for pruned entries (`.paw/documentdb_core_local_pool/src/pool.rs:284-325`).
- `try_pop_idle` attempts up to ten LIFO pops, releases permits for broken/expired connections, touches and increments in-flight for a reusable connection, and returns `None` after the cap (`.paw/documentdb_core_local_pool/src/pool.rs:327-352`).
- `create_new_connection` wraps global semaphore acquire in `acquire_timeout`, forgets the acquired permit, wraps manager creation in `create_timeout`, increments in-flight on success, and releases the permit on backend error or create timeout (`.paw/documentdb_core_local_pool/src/pool.rs:354-383`).
- `PooledConnection` wraps `tokio_postgres::Client`, birth/last-used instants, and `needs_reset`; it exposes client accessors, `into_client`, lifecycle timestamps, `mark_needs_reset`, `needs_reset`, `is_broken`, lifetime/idle checks, and `reset_session` SQL (`.paw/documentdb_core_local_pool/src/connection.rs:13-132`).
- `ConnectionManager` stores a PostgreSQL config and application name, connects with `NoTls`, spawns the connection future on the current runtime, and returns the client (`.paw/documentdb_core_local_pool/src/manager.rs:13-60`).
- Reference unit tests cover global permits, defaults/status, acquire timeout and closed semaphore, multi-core semaphore sharing/blocking/fairness, permit accounting stress, config values, zero/large/release-above-initial permits, per-core independent idle queues, and single-thread runtime non-deadlock behavior (`.paw/documentdb_core_local_pool/tests/pool_tests.rs:41-199`, `.paw/documentdb_core_local_pool/tests/pool_tests.rs:201-375`, `.paw/documentdb_core_local_pool/tests/pool_tests.rs:379-535`, `.paw/documentdb_core_local_pool/tests/pool_tests.rs:545-826`).
- Reference live tests require PostgreSQL on localhost:9712 and cover acquire/release, idle reuse, global limit, discard permit release, timeout release session reset, idle-limit discard, prune, two-core shared limit, broken idle handling, and regression cases for reuse/permits/session reset/prune/two-core limit (`.paw/documentdb_core_local_pool/tests/live_pool_tests.rs:1-52`, `.paw/documentdb_core_local_pool/tests/live_pool_tests.rs:56-240`, `.paw/documentdb_core_local_pool/tests/live_pool_tests.rs:244-390`, `.paw/documentdb_core_local_pool/tests/live_pool_tests.rs:398-560`).

### Gateway Reference (`.paw/documentdb/pg_documentdb_gw`)

- The gateway workspace depends on `deadpool = { version = "0.12.3", features = ["rt_tokio_1"] }`, `deadpool-postgres = "0.14.1"`, `tokio` with `full` and `tracing`, and `tokio-postgres` with `with-serde_json-1` and `array-impls` (`.paw/documentdb/pg_documentdb_gw/Cargo.toml:21-64`).
- `documentdb_gateway_core` consumes workspace `deadpool` and `deadpool-postgres` dependencies (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/Cargo.toml:12-45`).
- The gateway binary builds a Tokio multi-thread runtime using configured worker thread count, then initializes the connection pool manager and service context before running the gateway (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway/src/main.rs:31-63`, `.paw/documentdb/pg_documentdb_gw/documentdb_gateway/src/main.rs:99-134`).
- Gateway pool settings define prune interval, idle lifetime, and lifetime constants as 10s, 300s, and 3600s, compute adjusted max connections by subtracting system budget with a floor, and expose connection lifetime accessors (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/pool_settings.rs:13-81`).
- `ConnectionPool::new_with_user` builds a PostgreSQL config from setup, user/password/application name, search path, and command/transaction timeouts (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:47-81`, `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:145-160`).
- `InstrumentedManager` wraps `deadpool_postgres::Manager`, records successful connection creation durations, delegates recycle, and delegates detach (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:83-120`).
- `ConnectionPool` owns a primary `DeadpoolPool<InstrumentedManager>`, a timeout `DeadpoolPool<InstrumentedManager>`, atomic last-used timestamp, metrics, identifier, and prune task handle (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:122-139`).
- `new_with_user` builds the primary pool with `RecyclingMethod::Fast` and the timeout pool with `RecyclingMethod::Clean`, both with `Runtime::Tokio1`, adjusted max size, and wait timeout from `postgres_command_timeout_secs` (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:161-184`).
- The gateway pruner task periodically calls `retain` on both pools; the primary pool retains by configured idle/lifetime thresholds, and the timeout pool uses command-timeout idle lifetime plus the configured lifetime (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:186-212`).
- `acquire_connection` and `acquire_timeout_connection` update the atomic last-used timestamp, call `pool.get()` or `timeout_pool.get()`, and record timeout metrics when the error is a deadpool timeout (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:232-272`, `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/pool_metrics.rs:175-184`).
- `status` returns a non-mutating snapshot, `report_status` returns and flushes interval metrics, and `combined_status` sums max size, size, available, and waiting across primary and timeout pools (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:293-327`).
- `Drop for ConnectionPool` aborts the background pruner task (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:329-333`).
- `PoolManager` stores fixed system request/auth pools and `DashMap` maps for per-user and shared data pools; comments state `Arc<ConnectionPool>` allows sharing across threads from different connections (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/pool_manager.rs:40-52`).
- `PoolManager` exposes system/auth acquisition, allocates user data pools keyed by username/settings, lazily creates shared data pools keyed by settings, removes unused dynamic pools by last-used age, and reports statuses for all pools (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/pool_manager.rs:71-201`).
- Gateway startup creates SystemRequests and PreAuthRequests pools with budgets 2 and 5, validates them with a startup query, and returns an `Arc<PoolManager>` (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/pool_manager.rs:27-35`, `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/pool_manager.rs:225-326`).
- `Connection` wraps a pooled connection and an atomic in-transaction flag; query execution uses `prepare_typed_cached` on the pooled connection before running `query` (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection.rs:24-83`).
- `run_request_with_retries` selects the timeout pool only when gateway timeout is required and transaction timeout support is false; otherwise it selects the primary pool, wraps errors as `ErrorKind::PoolError`, and handles retry classification (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/query_dispatch.rs:237-291`, `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/query_dispatch.rs:308-514`).
- Gateway connection pool integration tests cover connection creation metrics, failed creation metrics, acquisition timeout metrics, `run_request_with_retries` timeout counting, system request pool timeout reporting, and command-timeout error behavior (`.paw/documentdb/pg_documentdb_gw/documentdb_tests/tests/connection_pool_tests.rs:86-247`).
- Gateway core tests for `ConnectionPool` cover construction, dual-pool combined max size, identifiers, last-used timestamp, and PostgreSQL password configuration (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:336-496`).
- Gateway core tests for `PoolManager` cover shared pool reuse, max-connection setting changes, user pool allocation keyed by settings, missing user errors, and pool stats counts (`.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/pool_manager.rs:477-620`).

## Code References

- `crates/deadpool/src/lib.rs:24-56` - Crate root feature-gated modules, runtime reexport, and `Status` fields/consistency docs.
- `crates/deadpool/src/managed/pool.rs:67-158` - Managed pool construction and checkout path.
- `crates/deadpool/src/managed/pool.rs:160-239` - Managed recycle/create paths with hooks, manager calls, metrics, and size accounting.
- `crates/deadpool/src/managed/pool.rs:241-368` - Managed resize, retain, timeouts accessor, close, is_closed, and status.
- `crates/deadpool/src/managed/pool.rs:454-518` - Managed object return/detach and timeout adapter internals.
- `crates/deadpool/src/managed/object.rs:50-93` - Managed `Object::take` and `Drop` return behavior.
- `crates/deadpool/src/managed/builder.rs:37-187` - Managed builder fields, timeout/runtime validation, and builder setters.
- `crates/deadpool/src/managed/config.rs:5-118` - Managed pool config, timeouts, and queue mode.
- `crates/deadpool/src/managed/manager.rs:5-37` - Managed manager create/recycle/detach contract.
- `crates/deadpool/src/managed/hooks.rs:9-169` - Hook types and hook vector application.
- `crates/deadpool/src/managed/errors.rs:5-123` - Managed recycle/timeout/pool error types.
- `crates/deadpool/src/unmanaged/mod.rs:51-94` - Unmanaged object take/drop return behavior.
- `crates/deadpool/src/unmanaged/mod.rs:152-247` - Unmanaged construction and get/try/timed checkout paths.
- `crates/deadpool/src/unmanaged/mod.rs:249-353` - Unmanaged add/try_add/remove/close/status paths.
- `crates/deadpool/src/unmanaged/mod.rs:355-425` - Unmanaged `PoolInner`, clear/is_closed, and `From<I>` constructor.
- `crates/deadpool/src/unmanaged/config.rs:5-53` - Unmanaged config and runtime timeout docs.
- `crates/deadpool-postgres/src/lib.rs:63-177` - PostgreSQL type aliases and manager create/recycle/detach implementation.
- `crates/deadpool-postgres/src/lib.rs:233-406` - PostgreSQL statement cache collections and per-client cache.
- `crates/deadpool-postgres/src/lib.rs:408-660` - Client, transaction, and transaction-builder wrappers.
- `crates/deadpool-postgres/src/config.rs:146-374` - PostgreSQL pool creation config and recycling methods.
- `crates/deadpool-postgres/src/generic_client.rs:18-268` - Generic client sealed trait and implementations.
- `.paw/documentdb_core_local_pool/src/pool.rs:47-395` - Reference global permits, core-local pool APIs, permit accounting, acquire/release/prune/create paths.
- `.paw/documentdb_core_local_pool/src/connection.rs:13-132` - Reference pooled PostgreSQL connection metadata and reset SQL.
- `.paw/documentdb_core_local_pool/src/manager.rs:13-60` - Reference PostgreSQL connection manager creation behavior.
- `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:122-327` - Gateway Deadpool-backed dual-pool wrapper, acquire paths, metrics, prune, and status.
- `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/pool_manager.rs:40-201` - Gateway pool manager storage, data/shared pool creation, cleanup, and reporting.
- `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/query_dispatch.rs:337-354` - Gateway timeout-pool selection on session-level statement timeout path.
- `.github/workflows/deadpool.yml:8-96` - Core crate check/clippy/MSRV/doc/fmt/test workflow commands.
- `.github/workflows/deadpool-postgres.yml:8-143` - PostgreSQL crate check/wasm/reexport/clippy/MSRV/doc/fmt/test workflow commands.

## Architecture Documentation

- The managed pool pattern is checkout-time recycle/create plus drop-time return: the README states objects are returned by `Drop`, health is checked on next retrieval, and no background task is used by Deadpool itself (`crates/deadpool/README.md:109-119`).
- The same README documents that returning an object locks a single mutex and retrieval uses a semaphore to reduce mutex contention (`crates/deadpool/README.md:133-140`).
- Managed and unmanaged pool code both use semaphore permits with `permit.forget()` after a successful acquisition, then manually add permits when objects return, are detached, or capacity grows (`crates/deadpool/src/managed/pool.rs:150-158`, `crates/deadpool/src/managed/pool.rs:454-479`, `crates/deadpool/src/managed/pool.rs:275-279`, `crates/deadpool/src/unmanaged/mod.rs:197-203`, `crates/deadpool/src/unmanaged/mod.rs:258-303`).
- Managed queue ordering is explicit and configurable through FIFO/LIFO mode, and checkout selects `pop_front` or `pop_back` accordingly (`crates/deadpool/src/managed/config.rs:106-118`, `crates/deadpool/src/managed/pool.rs:135-139`).
- The crate root forbids unsafe code and enables rustdoc/lint strictness for nonstandard style, rust 2018 idioms, rustdoc link lints, missing docs, and related warnings (`crates/deadpool/src/lib.rs:1-22`, `crates/deadpool-postgres/src/lib.rs:1-21`, `crates/deadpool-runtime/src/lib.rs:1-22`).
- CI workflows are generated from crate metadata conventions: `gen-ci.sh` iterates `crates/*`, reads each `Cargo.toml` and optional `ci.config.yml`, renders `ci.jsonnet`, and writes `.github/workflows/<crate>.yml` (`gen-ci.sh:1-21`, `ci.jsonnet:45-90`, `ci.jsonnet:91-120`).
- The gateway reference keeps PostgreSQL-specific timeout/session-reset behavior in gateway pool selection and `deadpool-postgres` recycling methods, with the generic Deadpool dependency appearing as a workspace dependency and gateway-specific selection inside `ConnectionPool`/`query_dispatch` (`.paw/documentdb/pg_documentdb_gw/Cargo.toml:31-32`, `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/connection_pool.rs:179-184`, `.paw/documentdb/pg_documentdb_gw/documentdb_gateway_core/src/postgres/conn_mgmt/query_dispatch.rs:337-354`).
- The reference core-local pool separates local idle state and shared capacity through `CoreLocalPool` plus `GlobalPermits`; PostgreSQL-specific connection/session handling is held in `ConnectionManager` and `PooledConnection` (`.paw/documentdb_core_local_pool/src/pool.rs:47-137`, `.paw/documentdb_core_local_pool/src/manager.rs:13-60`, `.paw/documentdb_core_local_pool/src/connection.rs:13-132`).

## Open Questions

None.
