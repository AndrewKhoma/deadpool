# Lockfree Deadpool Implementation Plan

## Overview

Implement an opt-in lockfree/core-local pool mode for the `deadpool` crate while keeping existing managed, unmanaged, and `deadpool-postgres` behavior as the default path. The plan separates user-visible opt-in API, shared internal storage abstractions, managed pool support, unmanaged pool support, PostgreSQL validation, and final documentation so each phase can be reviewed independently.

The main architectural decision is to add a new storage mode through builder/constructor APIs rather than extending existing `PoolConfig` structs. `PoolConfig` exposes public fields in both managed and unmanaged modules (`crates/deadpool/src/managed/config.rs:10-34`, `crates/deadpool/src/unmanaged/config.rs:10-33`), so adding fields would create avoidable source-compatibility risk. The new mode should default to the current shared semaphore-plus-mutex implementation and require explicit opt-in for core-local behavior.

## Current State Analysis

Managed pools currently use `Arc<PoolInner>`, a Tokio `Semaphore` for capacity/waiting, a mutex-protected `VecDeque` for idle objects, and configured `QueueMode` FIFO/LIFO dequeue behavior (`.paw/work/lockfree-deadpool/CodeResearch.md:77-85`). Checked-out managed objects return through `Drop`, and `Object::take` detaches the object permanently (`.paw/work/lockfree-deadpool/CodeResearch.md:97-107`). Managed checkout also owns manager create/recycle hooks, timeout phases, resize, retain, close, status, and weak-pool behavior (`.paw/work/lockfree-deadpool/CodeResearch.md:87-107`).

Unmanaged pools currently use separate semaphores for object availability and size capacity plus a mutex-protected object queue; public behavior covers `get`, `try_get`, `timeout_get`, `add`, `try_add`, remove variants, close, status, and object take/drop return (`.paw/work/lockfree-deadpool/CodeResearch.md:120-137`). Existing unmanaged constructors are `Pool::new`, `Pool::from_config`, `Default`, and `From<I>` (`.paw/work/lockfree-deadpool/CodeResearch.md:125-135`).

`deadpool-postgres` consumes the managed pool through `deadpool::managed`, generated reexports, PostgreSQL manager creation/recycling, statement caches, config builders, and tests covering cached statements, recycling methods, transactions, generic client compatibility, config, and feature behavior (`.paw/work/lockfree-deadpool/CodeResearch.md:144-169`). The gateway reference builds primary `Fast` and timeout `Clean` Deadpool-backed PostgreSQL pools, records timeout metrics, combines status, and prunes through managed `retain` (`.paw/work/lockfree-deadpool/CodeResearch.md:201-222`).

The local reference pool demonstrates the target model as one pool per core with local idle state, shared global capacity permits, acquire/release/discard/status/prune operations, and PostgreSQL-specific reset behavior kept outside the generic capacity model (`.paw/work/lockfree-deadpool/CodeResearch.md:178-199`). The reference still contains bounded lifecycle synchronization for idle/status/prune operations (`.paw/work/lockfree-deadpool/CodeResearch.md:186-193`), matching the spec boundary that lifecycle work may coordinate globally (`.paw/work/lockfree-deadpool/Spec.md:82-94`).

## Desired End State

Deadpool has an opt-in pool mode named `PoolMode::CoreLocal`, available to managed builders and unmanaged constructors when the `core-local` feature is enabled. `PoolMode::Shared` remains the default and routes through the current semaphore-plus-mutex behavior. The core-local mode exposes explicit user-created local pool handles that share global capacity while each handle owns local idle state. The plan does not rely on automatic Tokio-worker, thread-local, or task-local sharding inside one shared `Pool`; users choose local handles explicitly. Checked-out objects carry their origin local-handle identity: same-handle return is the steady-state lockfree path, while cross-handle or migrated-task return uses a documented safe reclamation path that returns to the origin handle when possible or releases/detaches capacity if the origin handle is closed or inactive. Managed `QueueMode` behavior remains part of the compatibility contract: core-local mode must either preserve FIFO/LIFO ordering for objects visible to the selected local storage scope or explicitly document any opt-in limitation before implementation is considered complete.

The strict lockfree guarantee applies to pre-populated steady-state checkout/checkin through the same local handle: that local-success path must not await, park on the runtime, acquire a blocking lock, or touch the shared semaphore/global idle queue while reusable local objects are available. Bounded synchronization remains permitted for local-handle creation, cross-handle reclamation, waiting/backpressure, object creation, manager recycle/hook execution, resizing, retain/prune-style maintenance, status snapshots, close, and recovery paths.

The managed implementation preserves manager contracts, timeout phases, hooks, drop-return, detach, weak-pool, status, resize, retain, and close semantics. The unmanaged implementation preserves add/get/remove/try/timed behavior, close/status behavior, and object take/drop semantics. `deadpool-postgres` remains source-compatible and exposes a forwarding `core-local` feature that enables `deadpool/core-local`, making PostgreSQL opt-in visible through the downstream crate without gateway source changes.

Verification will combine existing crate tests, new managed/unmanaged core-local tests, feature/serde checks, optional PostgreSQL integration checks where environment supports them, and executable benchmark/stress gates. For pre-populated steady-state checkout/checkin at 1, 8, 16, and 32 workers, core-local mode must show zero observed blocking/parking on the local hot path, p99 checkout latency no worse than shared mode, and throughput no worse than shared mode.

## What We're NOT Doing

- Changing the default pool behavior for existing users.
- Adding fields to existing public `PoolConfig` structs.
- Modifying or committing changes to `.paw/documentdb/pg_documentdb_gw`.
- Pulling the external ADO PR unless later implementation uncovers a local-reference gap.
- Moving PostgreSQL session-reset behavior into the generic `deadpool` core.
- Promising lockfree behavior for initialization, shutdown, resize, retain/prune, status snapshots, object creation, manager recycling, hook execution, timeout waiting, or recovery paths.
- Automatically inferring runtime-worker affinity for a single shared pool without explicit local handles.
- Introducing strict waiter fairness beyond existing documented behavior.

## Phase Status

- [x] **Phase 1: Opt-In API and Storage Abstraction** - Add feature-gated core-local mode surfaces and internal storage boundaries without changing default behavior.
- [x] **Phase 2: Managed Core-Local Pool Mode** - Implement managed-pool core-local storage integration and managed behavior coverage.
- [x] **Phase 3: Unmanaged Core-Local Pool Mode** - Implement unmanaged-pool core-local storage integration and unmanaged behavior coverage.
- [x] **Phase 4: PostgreSQL Compatibility and Stress Validation** - Validate reexported PostgreSQL usage, gateway-style expectations, feature combinations, and performance/stress entry points.
- [ ] **Phase 5: Documentation** - Document opt-in usage, lockfree boundaries, compatibility expectations, and as-built implementation details.

## Phase Candidates

<!-- No unresolved candidates initially. Add checkbox items here if implementation discovers follow-up work. -->

---

## Phase 1: Opt-In API and Storage Abstraction

### Changes Required:

- **`crates/deadpool/Cargo.toml`**: Add a `core-local` feature for lockfree/core-local internal queue primitives while preserving current default features (`.paw/work/lockfree-deadpool/CodeResearch.md:51-60`).
- **`crates/deadpool/src/lib.rs` / `crates/deadpool/src/mode.rs`**: Add a shared `PoolMode` type exported as `deadpool::PoolMode` when `managed` or `unmanaged` is enabled; keep `PoolMode::Shared` available for existing/default behavior and gate `PoolMode::CoreLocal` behind `core-local`. Reexport the same type from managed and unmanaged modules so unmanaged-only builds do not depend on managed config (`.paw/work/lockfree-deadpool/CodeResearch.md:51-60`).
- **`crates/deadpool/src/managed/config.rs` / `crates/deadpool/src/unmanaged/config.rs`**: Reference the shared `PoolMode` type in module documentation without adding fields to either public `PoolConfig`; document `Shared` as default and `CoreLocal` as opt-in (`crates/deadpool/src/managed/config.rs:10-34`, `crates/deadpool/src/unmanaged/config.rs:10-33`, `.paw/work/lockfree-deadpool/Spec.md:82-95`).
- **`crates/deadpool/src/managed/builder.rs`**: Add a private builder field for mode plus public builder methods to select core-local mode and create explicit local handles sharing the pool's global capacity, preserving existing builder defaults and timeout validation (`crates/deadpool/src/managed/builder.rs:41-98`, `crates/deadpool/src/managed/builder.rs:100-187`).
- **`crates/deadpool/src/unmanaged/config.rs` / `crates/deadpool/src/unmanaged/mod.rs`**: Add unmanaged mode selection through new constructors `new_with_mode`, `from_config_with_mode`, and `from_iter_with_mode`, plus explicit local-handle construction for core-local pools, without altering `PoolConfig` fields or existing constructors (`crates/deadpool/src/unmanaged/config.rs:10-33`, `.paw/work/lockfree-deadpool/CodeResearch.md:120-137`).
- **`crates/deadpool/src/managed/reexports.rs`**: Ensure generated backend aliases expose the shared mode type, local-handle type, and builder method in downstream crates using `managed_reexports!` (`.paw/work/lockfree-deadpool/CodeResearch.md:62-66`).
- **`crates/deadpool-postgres/Cargo.toml` / `crates/deadpool-postgres/src/lib.rs`**: Add and reexport a forwarding `core-local` feature path that enables `deadpool/core-local`, so PostgreSQL users opt in through `deadpool-postgres` (`.paw/work/lockfree-deadpool/CodeResearch.md:144-163`).
- **Internal storage modules under `crates/deadpool/src/managed/` and `crates/deadpool/src/unmanaged/`**: Introduce component boundaries for the existing shared storage and the future core-local storage so later phases can switch by mode without duplicating public behavior (`.paw/work/lockfree-deadpool/CodeResearch.md:77-85`, `.paw/work/lockfree-deadpool/CodeResearch.md:120-137`).
- **Tests**: Add API-focused tests or compile coverage proving default constructors/builders still use shared mode, core-local mode can be selected when enabled, and public config struct initialization remains unchanged.

### Success Criteria:

#### Automated Verification:

- [x] Core feature checks pass: `cd crates/deadpool && cargo check --no-default-features --features managed,rt_tokio_1`
- [x] Core feature checks pass: `cd crates/deadpool && cargo check --no-default-features --features unmanaged,rt_tokio_1`
- [x] Core-local managed feature check passes: `cd crates/deadpool && cargo check --no-default-features --features managed,core-local,rt_tokio_1`
- [x] Core-local unmanaged feature check passes: `cd crates/deadpool && cargo check --no-default-features --features unmanaged,core-local,rt_tokio_1`
- [x] PostgreSQL forwarding feature check passes: `cd crates/deadpool-postgres && cargo check --no-default-features --features core-local,rt_tokio_1`
- [x] Core all-feature checks pass: `cd crates/deadpool && cargo check --all-features`
- [x] Core tests pass for touched API surfaces: `cd crates/deadpool && cargo test --all-features`
- [x] Formatting passes: `cd crates/deadpool && cargo fmt --check`

#### Manual Verification:

- [x] Existing examples of managed and unmanaged pool creation still compile without selecting the new mode.
- [x] Opt-in mode selection is explicit and does not require modifying `PoolConfig` struct literals.
- [x] Mode documentation names which behavior is default and which behavior is core-local.
- [x] Explicit local-handle behavior, same-handle hot-path guarantee, and migrated/cross-handle reclamation behavior are named before managed/unmanaged implementation begins.

---

## Phase 2: Managed Core-Local Pool Mode

### Changes Required:

- **`crates/deadpool/src/managed/pool.rs`**: Route `PoolInner` construction and checkout/checkin through the storage abstraction introduced in Phase 1 while keeping current shared behavior for `PoolMode::Shared` (`.paw/work/lockfree-deadpool/CodeResearch.md:77-85`).
- **Managed core-local storage module**: Implement explicit local handles with local idle ownership plus shared global capacity accounting for managed objects, using non-blocking local operations for available-object checkout and drop-return through the same local handle in core-local mode (`.paw/work/lockfree-deadpool/Spec.md:82-94`, `.paw/work/lockfree-deadpool/CodeResearch.md:178-199`).
- **Managed cross-local behavior**: Implement origin-handle identity for checked-out objects, same-handle lockfree return, cross-handle safe reclamation back to the origin handle when possible, capacity release/detach when the origin handle is closed or inactive, and close/retain/status behavior across multiple local handles.
- **Managed core-local queue-mode behavior**: Preserve `QueueMode::Fifo` and `QueueMode::Lifo` ordering within the storage scope where reusable objects are visible, or document an explicit opt-in limitation if global FIFO/LIFO ordering is not representable in core-local mode (`crates/deadpool/src/managed/config.rs:106-118`, `.paw/work/lockfree-deadpool/Spec.md:67-76`).
- **`crates/deadpool/src/managed/pool.rs` create/recycle paths**: Preserve `try_create`, `try_recycle`, hook application, timeout mapping, metrics updates, `manager.detach`, and failed-object cleanup semantics while integrating core-local storage (`.paw/work/lockfree-deadpool/CodeResearch.md:87-95`).
- **`crates/deadpool/src/managed/object.rs` / managed return paths**: Preserve `Drop` return and `Object::take` detach behavior across shared and core-local modes (`.paw/work/lockfree-deadpool/CodeResearch.md:97-103`).
- **Managed lifecycle methods**: Preserve `resize`, `retain`, `close`, `is_closed`, `status`, `manager`, and `weak` behavior, with bounded synchronization allowed for lifecycle paths (`.paw/work/lockfree-deadpool/CodeResearch.md:104-107`, `.paw/work/lockfree-deadpool/Spec.md:122-144`).
- **`crates/deadpool/tests/managed*.rs`**: Add core-local variants for checkout/drop-return, `Object::take`, capacity exhaustion, timeout waiting, close with checked-out objects, resize grow/shrink, retain, create failure, recycle failure, cancellation stress, and hook success/error behavior (`.paw/work/lockfree-deadpool/CodeResearch.md:109-118`).
- **`crates/deadpool/benches/managed.rs`**: Add a benchmark group for managed core-local checkout/checkin alongside the current shared benchmark (`.paw/work/lockfree-deadpool/CodeResearch.md:171-176`).
- **`crates/deadpool/tests/managed_core_local_stress.rs`**: Add an ignored executable shared-vs-core-local managed stress gate for 1, 8, 16, and 32 workers with pre-populated local handles and capacity-pressure scenarios. The harness must include warm-up, a fixed iteration count, p99 checkout latency, throughput, and hot-path instrumentation counters for disallowed awaits, shared-semaphore touches, blocking lock acquisition, or runtime parking on same-handle local success paths.

### Success Criteria:

#### Automated Verification:

- [x] Managed tests pass: `cd crates/deadpool && cargo test --all-features --test managed`
- [x] Managed timeout tests pass: `cd crates/deadpool && cargo test --all-features --test managed_timeout`
- [x] Managed cancellation tests pass: `cd crates/deadpool && cargo test --all-features --test managed_cancellation`
- [x] Core all-feature tests pass: `cd crates/deadpool && cargo test --all-features`
- [x] Managed benchmark builds: `cd crates/deadpool && cargo bench --bench managed --no-run --all-features`
- [x] Managed stress gate command passes: `cd crates/deadpool && cargo test --all-features --test managed_core_local_stress -- --ignored`
- [x] Managed stress gate passes with zero observed blocking/parking on the local hot path, p99 checkout latency no worse than shared mode, and throughput no worse than shared mode at 1/8/16/32 workers.
- [x] Clippy passes: `cd crates/deadpool && cargo clippy --no-deps --all-features -- -D warnings`

#### Manual Verification:

- [x] Core-local managed checkout/drop-return loops reuse objects without capacity loss.
- [x] Cross-local return/reclamation behavior is tested for two local handles sharing capacity, including return to origin handle and release/detach when the origin handle is inactive.
- [x] Managed `QueueMode::Fifo` and `QueueMode::Lifo` behavior is tested or documented as an explicit core-local limitation.
- [x] Status is polled under concurrent checkout/checkin, detach/take, close/drain, resize, retain, and injected create/recycle failures, with no panic/underflow and final exact recovery after load drains.
- [x] Managed create/recycle/hook failure paths leave the pool able to serve future checkouts.
- [x] Lifecycle methods remain available and documented as outside the steady-state lockfree guarantee.

---

## Phase 3: Unmanaged Core-Local Pool Mode

### Changes Required:

- **`crates/deadpool/src/unmanaged/mod.rs`**: Route unmanaged construction, `get`, `try_get`, `timeout_get`, `add`, `try_add`, remove variants, close, and status through shared/core-local storage mode dispatch while keeping current behavior as default (`.paw/work/lockfree-deadpool/CodeResearch.md:120-137`).
- **Unmanaged core-local storage module**: Implement explicit local handles with local idle ownership and shared capacity accounting for user-supplied objects, with non-blocking local operations for available-object checkout and drop-return through the same local handle in core-local mode (`.paw/work/lockfree-deadpool/Spec.md:82-94`, `.paw/work/lockfree-deadpool/CodeResearch.md:178-199`).
- **Unmanaged cross-local behavior**: Implement origin-handle identity for checked-out objects, same-handle lockfree return, cross-handle safe reclamation back to the origin handle when possible, capacity release when the origin handle is closed or inactive, and close/status behavior across multiple local handles.
- **`Object<T>` return/take paths**: Preserve object `Drop` return and `Object::take` capacity release semantics across storage modes (`.paw/work/lockfree-deadpool/CodeResearch.md:120-124`).
- **Unmanaged close/status/clear paths**: Preserve semaphore closure, queued object cleanup, and status field behavior with eventual consistency under load (`.paw/work/lockfree-deadpool/CodeResearch.md:133-137`, `.paw/work/lockfree-deadpool/Spec.md:113-120`).
- **`crates/deadpool/tests/unmanaged*.rs`**: Add core-local variants for add/get/drop-return, `try_add`, `try_get`, remove, blocked add after remove, close, timeout/no-runtime behavior, concurrency, and capacity accounting stress (`.paw/work/lockfree-deadpool/CodeResearch.md:139-142`).
- **`crates/deadpool/benches/unmanaged.rs`**: Add a benchmark group for unmanaged core-local checkout/checkin alongside the current shared benchmark (`.paw/work/lockfree-deadpool/CodeResearch.md:171-176`).
- **Unmanaged boundary tests**: Cover max-size zero if allowed, max-size one under concurrent checkout/return/detach, locally idle objects at close, checked-out return after close, large max-size stress, and duplicate-release prevention.
- **`crates/deadpool/tests/unmanaged_core_local_stress.rs`**: Add an ignored executable shared-vs-core-local unmanaged stress gate for 1, 8, 16, and 32 workers with pre-populated local handles and capacity-pressure scenarios. The harness must include warm-up, a fixed iteration count, p99 checkout latency, throughput, and hot-path instrumentation counters for disallowed awaits, shared-semaphore touches, blocking lock acquisition, or runtime parking on same-handle local success paths.

### Success Criteria:

#### Automated Verification:

- [x] Unmanaged tests pass: `cd crates/deadpool && cargo test --all-features --test unmanaged`
- [x] Unmanaged timeout tests pass: `cd crates/deadpool && cargo test --all-features --test unmanaged_timeout`
- [x] Core all-feature tests pass: `cd crates/deadpool && cargo test --all-features`
- [x] Unmanaged benchmark builds: `cd crates/deadpool && cargo bench --bench unmanaged --no-run --all-features`
- [x] Unmanaged stress gate command passes: `cd crates/deadpool && cargo test --all-features --test unmanaged_core_local_stress -- --ignored`
- [x] Unmanaged stress gate passes with zero observed blocking/parking on the local hot path, p99 checkout latency no worse than shared mode, and throughput no worse than shared mode at 1/8/16/32 workers.
- [x] Clippy passes: `cd crates/deadpool && cargo clippy --no-deps --all-features -- -D warnings`

#### Manual Verification:

- [x] Core-local unmanaged add/get/drop-return loops preserve object count and capacity.
- [x] Cross-local return/reclamation behavior is tested for two local handles sharing capacity, including return to origin handle and release when the origin handle is inactive.
- [x] `Object::take` releases capacity exactly once in core-local mode.
- [x] Status is polled under concurrent checkout/checkin, take/remove, close/drain, resize, and capacity-pressure scenarios, with no panic/underflow and final exact recovery after load drains.
- [x] Close and timeout behavior match existing observable outcomes.

---

## Phase 4: PostgreSQL Compatibility and Stress Validation

### Changes Required:

- **`crates/deadpool-postgres/Cargo.toml` / `crates/deadpool-postgres/src/lib.rs` / generated reexports**: Ensure `deadpool-postgres` users can access the new managed opt-in mode through a forwarding `core-local` feature, existing type aliases, and builder flows without changing PostgreSQL manager behavior (`.paw/work/lockfree-deadpool/CodeResearch.md:144-163`).
- **`crates/deadpool-postgres/src/config.rs`**: Preserve default config-driven pool creation; add mode selection only through a source-compatible path that does not require adding fields to existing public config structs unless a non-breaking wrapper is used (`.paw/work/lockfree-deadpool/CodeResearch.md:158-161`).
- **`crates/deadpool-postgres/tests/`**: Add coverage showing PostgreSQL pools still build and run with existing config paths and can opt into core-local mode through the managed builder surface when the `deadpool-postgres/core-local` feature is enabled (`.paw/work/lockfree-deadpool/CodeResearch.md:164-169`).
- **Gateway-reference validation**: Use `.paw/documentdb/pg_documentdb_gw` as a compatibility reference for dual-pool `Fast`/`Clean` recycling, timeout errors, combined status, retain-based pruning, and metrics expectations without committing gateway changes; represent those expectations through automated downstream-style tests or extracted contract fixtures rather than manual notes only (`.paw/work/lockfree-deadpool/CodeResearch.md:201-222`).
- **PostgreSQL lifecycle tests**: Add dirty-session and cache lifecycle cases under core-local mode, including `Clean` and custom recycling, statement-cache clear/remove, detach, failed transactions, timeout-pool style clean reuse, and session-state reset expectations.
- **`crates/deadpool/benches/managed.rs` / `crates/deadpool/benches/unmanaged.rs` / stress tests**: Add or extend benchmark/stress entry points to compare shared and core-local checkout/checkin under concurrent workers and capacity pressure (`.paw/work/lockfree-deadpool/Spec.md:113-120`, `.paw/work/lockfree-deadpool/CodeResearch.md:171-176`).
- **Workflow/feature validation**: Check feature combinations and reexported-feature tooling following current CI commands (`.paw/work/lockfree-deadpool/CodeResearch.md:37-48`).

### Success Criteria:

#### Automated Verification:

- [x] PostgreSQL feature check passes: `cd crates/deadpool-postgres && cargo check --features serde,rt_tokio_1,rt_async-std_1`
- [x] PostgreSQL core-local feature check passes: `cd crates/deadpool-postgres && cargo check --features serde,core-local,rt_tokio_1,rt_async-std_1`
- [x] PostgreSQL reexport check passes: `cd crates/deadpool-postgres && ../../tools/check-reexported-features.sh`
- [x] PostgreSQL non-live core-local API tests pass; live PostgreSQL tests are covered by existing service-backed workflow when a PostgreSQL service is available: `cd crates/deadpool-postgres && cargo test --no-default-features --features core-local,rt_tokio_1 --test core_local_api`
- [x] Core stress/benchmark builds pass: `cd crates/deadpool && cargo bench --no-run --all-features`
- [x] Core docs build: `cd crates/deadpool && cargo doc --no-deps --all-features`
- [x] PostgreSQL docs build: `cd crates/deadpool-postgres && cargo doc --no-deps --features serde,rt_tokio_1,rt_async-std_1`

#### Manual Verification:

- [x] Existing `deadpool-postgres` config usage remains default-compatible.
- [x] The gateway reference expectations are represented by automated tests or extracted contract fixtures without editing gateway source.
- [x] Bench/stress output records p99 checkout latency, throughput, and blocking/parking observations for shared and core-local checkout/checkin behavior.

---

## Phase 5: Documentation

### Changes Required:

- **`.paw/work/lockfree-deadpool/Docs.md`**: Create the as-built technical reference with the implemented public API, internal storage model, lockfree boundary, lifecycle exclusions, test matrix, benchmark/stress guidance, and gateway-reference validation notes.
- **`crates/deadpool/README.md`**: Update feature/API documentation for the opt-in core-local mode, explicit local-handle contract, origin-handle/cross-local reclamation behavior, default compatibility, runtime assumptions, same-handle lockfree steady-state boundary, comparative performance gate, and examples consistent with existing README style (`.paw/work/lockfree-deadpool/CodeResearch.md:28-35`).
- **`crates/deadpool/CHANGELOG.md`**: Add an unreleased/user-visible entry for the new opt-in behavior and documentation updates, following the current Keep a Changelog structure (`.paw/work/lockfree-deadpool/CodeResearch.md:28-35`).
- **`crates/deadpool-postgres/README.md`**: Document `deadpool-postgres/core-local` feature forwarding and builder usage for PostgreSQL opt-in (`.paw/work/lockfree-deadpool/CodeResearch.md:28-35`).
- **`crates/deadpool-postgres/CHANGELOG.md`**: Add an entry for PostgreSQL-facing feature forwarding and documentation changes (`.paw/work/lockfree-deadpool/CodeResearch.md:28-35`).
- **Queueing and rollout guidance**: Document staged adoption, fallback to `PoolMode::Shared`, metrics to monitor, benchmark/stress validation expectations, waiter-fairness non-goals, and the scope where `QueueMode::Fifo` / `QueueMode::Lifo` applies in core-local mode.

### Success Criteria:

#### Automated Verification:

- [ ] Core docs build: `cd crates/deadpool && cargo doc --no-deps --all-features`
- [ ] PostgreSQL docs build if PostgreSQL docs changed: `cd crates/deadpool-postgres && cargo doc --no-deps --features serde,rt_tokio_1,rt_async-std_1`
- [ ] Formatting remains clean: `cd crates/deadpool && cargo fmt --check`

#### Manual Verification:

- [ ] Docs clearly state default behavior is unchanged.
- [ ] Docs clearly state the steady-state operations covered by the lockfree guarantee.
- [ ] Docs clearly state explicit local-handle usage, same-handle hot-path guarantees, and origin-handle/cross-local reclamation behavior.
- [ ] Docs clearly list lifecycle operations and manager/recycle behavior outside the strict lockfree boundary.
- [ ] Docs clearly state whether core-local mode preserves, weakens, or excludes strict waiter fairness and where `QueueMode::Fifo` / `QueueMode::Lifo` ordering applies.
- [ ] Docs include enough opt-in guidance for a gateway-style user to evaluate the new mode without modifying the gateway reference tree.
- [ ] Docs include staged rollout, monitoring, and rollback guidance for returning to shared mode.

---

## References

- Issue: none
- Spec: `.paw/work/lockfree-deadpool/Spec.md`
- Research: `.paw/work/lockfree-deadpool/SpecResearch.md`, `.paw/work/lockfree-deadpool/CodeResearch.md`
