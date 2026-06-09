---
date: "2026-06-09 02:03:26 UTC"
git_commit: "36d17e3f8a7be4d1b232bb7a6795bcc983e86e1f"
branch: "users/andrewkhoma/lockfree"
repository: "/workspace"
topic: "Lockfree Deadpool Spec Research"
tags: [research, specification]
status: complete
---

# Spec Research: Lockfree Deadpool

## Summary
Deadpool today exposes a small async pooling API split between managed pools, which create/recycle objects through a `Manager`, and unmanaged pools, where users add/remove objects directly. Checkout waits for capacity, optional runtime-backed timeouts can fail with timeout errors, and checked-out objects are returned when their wrapper is dropped. The core crate documents semaphore-based waiting and mutex-protected object queues on checkout/checkin paths.

PostgreSQL integration is provided by `deadpool-postgres`, which wraps `tokio-postgres`, adds statement caching, and exposes recycling methods. The provided gateway reference currently uses Deadpool pools, while the provided local-pool reference demonstrates core-local PostgreSQL pooling with per-core idle ownership and shared global permits.

## Agent Notes
- User wants the current Deadpool library updated on the current branch to support a lockfree implementation suitable for a thread-per-core application model.
- Public Deadpool API compatibility should be preserved where practical, with opt-in application-specific extensions allowed.
- "Lockfree" means checkout/checkin steady-state hot paths should avoid blocking locks or scheduler parking; bounded synchronization is acceptable for initialization, shutdown, resizing, and error recovery.
- Deliverable scope is the Deadpool library in this repository. The gateway under `.paw/documentdb/pg_documentdb_gw` should be used as reference/validation context only.
- The drafted PostgreSQL-oriented analogue under `.paw/documentdb_core_local_pool` is a local reference for the desired model.
- External context may exist in ADO PR `https://msdata.visualstudio.com/CosmosDB/_git/pgmongo/pullrequest/2031705`; only request that it be pulled under `.paw` if local references are insufficient.

## Research Findings

### Question 1: What public APIs, trait contracts, feature flags, and crate boundaries does the current Deadpool library expose that are relevant to pool checkout/checkin behavior and PostgreSQL integration?
**Answer**: The core `deadpool` crate exposes `managed` and `unmanaged` pool modules. Managed pools expose `Pool`, `PoolBuilder`, `Manager`, `Object`, `WeakPool`, `PoolConfig`, `Timeouts`, `QueueMode`, hooks, `Metrics`, `Status`, and pool errors; managers create, recycle, and optionally detach objects. Unmanaged pools expose `Pool`, `Object`, `PoolConfig`, `add`/`try_add`, `get`/`try_get`/`timeout_get`, `remove` variants, close/status APIs, and timeout/closed/no-runtime errors. Runtime timeout support is feature-gated through `rt_tokio_1`, `rt_async-std_1`, and `rt_smol_2`; serde config is feature-gated. `deadpool-postgres` is a separate crate using `deadpool` managed pools and reexported pool aliases for PostgreSQL.
**Evidence**: `crates/deadpool/README.md` feature and managed/unmanaged sections; `crates/deadpool/Cargo.toml` feature list; managed module API docs; unmanaged module API docs; `crates/deadpool-postgres/README.md` and `Cargo.toml`.
**Implications**: Spec requirements must account for both managed and unmanaged checkout/checkin behavior and for `deadpool-postgres` relying on the managed API and reexported type aliases.

### Question 2: Where does the current pool implementation use blocking synchronization, async-aware locks, channels, semaphores, wait queues, or other contention points on checkout/checkin hot paths?
**Answer**: The documented core pool behavior uses a `tokio::sync::Semaphore` to wait for capacity and a mutex-protected queue for pooled objects; the README states that returning an object locks a single mutex and retrieving uses a semaphore to reduce mutex contention. Managed checkout can wait for a semaphore permit, then creates or recycles an object; checkin happens when `Object` is dropped. Unmanaged checkout/add paths similarly wait on semaphores for available objects or size capacity, with `try_*` variants returning immediately. No public pool behavior is documented around channels; channel usage observed is in tests.
**Evidence**: `crates/deadpool/README.md` “Deadpool is fast” and FAQ sections; managed/unmanaged API docs; `managed_timeout`, `unmanaged_timeout`, and `unmanaged::try_*` tests.
**Implications**: Existing hot-path behavior includes both async waiting/backpressure and brief mutex-protected queue access.

### Question 3: What lifecycle operations exist today for pool creation, object construction, recycling, timeouts, shutdown/drop, status reporting, resizing, and error recovery, and which of those can tolerate bounded synchronization?
**Answer**: Existing lifecycle operations include builder/config-based pool creation, manager-driven object construction, checkout with optional wait/create/recycle timeouts, recycling on later checkout, checkin via `Drop`, object removal via `Object::take`, close/is_closed, status snapshots, runtime resizing for managed pools, `retain` maintenance, and hooks around creation/recycling. Documentation explicitly says `retain` blocks the entire pool while it runs; resize/close/status/retain are lifecycle or maintenance operations rather than ordinary object use. Error recovery behavior includes discarding failed recycled objects, returning errors from failed creation, and waking waiters when a drained pool creation fails.
**Evidence**: Managed builder/config/object/pool API docs; README reasons section; `managed_resize`, `managed_unreliable_manager`, `managed_deadlock`, `managed_hooks`, and `managed` retain tests.
**Implications**: The spec can distinguish steady-state checkout/checkin from existing creation, recycling, resizing, closing, status, retain, and error paths.

### Question 4: How does the current implementation preserve fairness, timeout behavior, backpressure, recycling semantics, and object validity across concurrent checkout/checkin operations?
**Answer**: Backpressure is enforced by pool maximum size: checkout waits when capacity is exhausted, and zero-duration timeout/try variants return timeout immediately. Timeouts cover managed wait, create, and recycle phases; without a runtime, configured timeouts return `NoRuntimeSpecified`. Queue mode controls object reuse order (`Fifo` least-recently-added or `Lifo` most-recently-added), but no public documentation promises strict waiter fairness. Recycling is checked before handing an idle managed object back out; failed recycle or failed pre/post recycle hooks cause that object to be discarded and another object to be tried/created. Status is documented as eventually consistent under load.
**Evidence**: `PoolConfig`, `Timeouts`, `QueueMode`, `PoolError`, and `Status` API docs; `managed_timeout`, `managed_unreliable_manager`, `managed_hooks`, `managed_cancellation`, and unmanaged timeout/concurrency tests.
**Implications**: Timeout, backpressure, queue-mode, recycling, and eventual-status behaviors are observable compatibility points; strict waiter fairness is not documented as a public contract.

### Question 5: What test coverage currently exists for pool behavior, concurrency, timeouts, recycling, manager errors, PostgreSQL-specific behavior, and public API compatibility?
**Answer**: Core pool tests cover managed basic/status/drop behavior, close behavior, multi-threaded concurrency, `Object::take`, retain, timeouts across runtimes, resize grow/shrink/close interactions, unreliable create/recycle managers, cancellation stress, hooks, configuration deserialization, drained-pool behavior, unmanaged add/remove/try/get/close/concurrency, and unmanaged timeouts. PostgreSQL tests cover basic queries, cached statements, typed prepare errors, transactions, generic client compatibility, recycling methods, statement cache clearing, config-from-env, URL overrides, and compile compatibility without `async_trait`.
**Evidence**: Test files under `crates/deadpool/tests`; test files under `crates/deadpool-postgres/tests`; deadpool and deadpool-postgres workflow test jobs.
**Implications**: Existing behavior is covered by unit/integration-style tests for the core crates and live PostgreSQL tests for the PostgreSQL crate.

### Question 6: What architecture and behavior does `.paw/documentdb_core_local_pool` implement for PostgreSQL pooling, especially around per-core state, lockfree/local queues, ownership transfer, and connection lifecycle management?
**Answer**: The reference crate is described as a core-local async PostgreSQL connection pool for thread-per-core runtimes. Each core creates a `CoreLocalPool` with its own idle queue while clones of `GlobalPermits` share a global semaphore limit. The public behavior includes `acquire`, `acquire_clean`, `release`, `release_timeout`, `discard`, `status`, and `prune`; idle reuse is local, new connections require global permit capacity, broken/expired/excess connections release permits, and clean release/acquire reset session state for connections marked dirty.
**Evidence**: `.paw/documentdb_core_local_pool` crate manifest and module docs; public docs for `CoreLocalPool`, `GlobalPermits`, `PoolConfig`, `PooledConnection`, and `PoolError`; `pool_tests` and `live_pool_tests` names/behaviors.
**Implications**: The reference behavior centers on per-core idle ownership, shared global capacity, explicit release/discard, lifecycle pruning, and PostgreSQL session-reset semantics.

### Question 7: How does `.paw/documentdb/pg_documentdb_gw` use Deadpool or PostgreSQL pooling today, and what integration behaviors matter for a thread-per-core gateway model?
**Answer**: The gateway depends on `deadpool` and `deadpool-postgres`. Its `ConnectionPool` builds two Deadpool-backed PostgreSQL pools per logical pool: a primary pool using fast recycling and a timeout pool using clean recycling for session-level timeout state. It records connection creation and pool-timeout metrics, combines status across the two pools, prunes idle/aged connections periodically, and aborts the pruner when the pool drops. `PoolManager` maintains fixed system/auth pools and dynamic user/shared data pools keyed by credentials/settings. The gateway binary creates a configurable multi-thread Tokio runtime today.
**Evidence**: Gateway `Cargo.toml`; `connection_pool`, `pool_manager`, `pool_settings`, and `main` docs/config behavior; gateway `connection_pool_tests` metric and timeout behaviors.
**Implications**: Gateway integration expects Arc-shareable logical pools, Deadpool timeout errors, combined status reporting, pruning, per-user/shared pool maps, and separate clean recycling for session-timeout use.

### Question 8: Are there existing benchmark, stress, or integration-test entry points in this repository or the provided gateway reference that can measure checkout/checkin latency, contention, and correctness under multi-threaded or per-core workloads?
**Answer**: The core `deadpool` crate has Criterion benches for managed `get` under multiple worker/pool-size combinations and an unmanaged `use_pool` bench. The `examples/postgres-benchmark` program compares 16 workers running 16,000 PostgreSQL queries with and without `deadpool-postgres`. Gateway integration tests exercise pool acquisition metrics and timeout counting with real PostgreSQL. The local-pool reference has concurrency/regression tests for multi-core global permit sharing, timeouts, no permit leaks, idle limits, closed-pool behavior, and single-thread runtime non-hanging behavior. No dedicated per-core checkout/checkin latency benchmark was observed in the main Deadpool repo or gateway reference.
**Evidence**: `crates/deadpool/benches`; `examples/postgres-benchmark` README and main program; gateway `connection_pool_tests`; `.paw/documentdb_core_local_pool/tests` test names/behaviors.
**Implications**: Existing entry points cover throughput/comparison and correctness; per-core latency measurement is not documented as an existing benchmark target.

### Question 9: Which parts of the reference pool can be generalized safely into Deadpool without coupling the library to the gateway or PostgreSQL-specific assumptions?
**Answer**: Behaviorally separable parts of the reference include core-local idle ownership, shared global capacity permits, acquire timeout vs create timeout, explicit release/discard semantics, status fields for idle/in-flight/global capacity, pruning by idle age/lifetime, and permit accounting invariants. PostgreSQL-specific parts include `tokio_postgres::Config`/`Client`, application-name manager behavior, connection health via PostgreSQL client closure state, and SQL session-reset behavior.
**Evidence**: `.paw/documentdb_core_local_pool` public docs for `PoolConfig`, `CoreLocalPool`, `GlobalPermits`, `PooledConnection`, and `ConnectionManager`; live pool tests covering reset/reuse/prune; gateway pool settings constants mirrored by the reference config.
**Implications**: Spec language can separate generic pool behaviors from PostgreSQL-only connection lifecycle behavior without describing implementation internals.

### Question 10: Are there repository documentation or contribution conventions that should be updated when changing the pool architecture or adding opt-in lockfree behavior?
**Answer**: The repository uses per-crate READMEs as crate-level docs, per-crate CHANGELOGs, Cargo feature metadata, docs.rs all-features metadata, and per-crate GitHub Actions workflows for check/clippy/doc/fmt/test. The `deadpool` workflow checks feature combinations, clippy with all features, rustdoc, rustfmt, MSRV 1.85, and tests; `deadpool-postgres` additionally tests against PostgreSQL and checks reexported features. No `CONTRIBUTING.md` was observed.
**Evidence**: `crates/deadpool/README.md`, `CHANGELOG.md`, and `Cargo.toml`; `crates/deadpool-postgres/README.md`, `CHANGELOG.md`, and `Cargo.toml`; `.github/workflows/deadpool.yml`; `.github/workflows/deadpool-postgres.yml`; `release.toml`.
**Implications**: User-visible pool architecture or feature changes are expected to be reflected in crate docs, feature metadata, changelogs, and existing CI coverage patterns.

## Open Unknowns

None.

## User-Provided External Knowledge (Manual Fill)

- [ ] If local references do not explain the intended thread-per-core constraints or gateway integration contract, ask the user to pull the ADO PR context under `.paw` before planning implementation details.
