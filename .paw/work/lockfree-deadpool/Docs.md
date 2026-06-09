# Lockfree Deadpool

## Overview

This work adds an opt-in `core-local` pool mode for Deadpool. The default `PoolMode::Shared` behavior remains unchanged for managed pools, unmanaged pools, and `deadpool-postgres`; users select `PoolMode::CoreLocal` explicitly when they want local handles with local idle queues and shared global capacity.

The mode is intended for thread-per-core or core-affine applications where the steady-state hot path repeatedly checks out and returns objects through the same local handle. In that same-handle path, reusable local objects are taken from lock-free local queues and returned to the originating local handle. Lifecycle paths, shared fallback, object creation, recycling, resizing, status snapshots, close/drain, and error recovery may still use bounded synchronization.

## Architecture and Design

### High-Level Architecture

The core crate now exposes `PoolMode` at the crate root and reexports it through managed and unmanaged modules. `PoolMode::Shared` is the default. `PoolMode::CoreLocal` is available when the `core-local` feature is enabled.

Managed and unmanaged pools both keep the existing shared storage path and add a core-local storage variant. A core-local pool owns shared capacity semaphores plus any number of explicit `LocalPool` handles. Each local handle owns a local idle queue. Checked-out managed objects and unmanaged objects carry origin-local metadata so same-handle returns can go back to the local queue; if the origin handle is gone or inactive, capacity is released and the object is detached or dropped according to the pool type.

### Design Decisions

- **Opt-in API**: Core-local mode is selected through builders or constructors rather than by adding fields to public `PoolConfig` structs. This preserves struct-literal compatibility.
- **Explicit local handles**: The library does not infer runtime worker identity or task locality. Users create local handles explicitly and decide where to store them.
- **Same-handle hot path**: The strict lockfree guarantee applies only to pre-populated checkout/checkin through the same local handle.
- **Queue order**: Managed `QueueMode::Fifo` and `QueueMode::Lifo` continue to apply to the shared fallback queue. Core-local idle queues are FIFO within each local handle; global FIFO/LIFO ordering across handles is not guaranteed.
- **Fairness**: Strict waiter fairness is not introduced for core-local mode. Existing shared wait/backpressure behavior remains available through shared fallback and `PoolMode::Shared`.

### Integration Points

`deadpool-postgres` forwards a `core-local` feature to `deadpool/core-local` and reexports the managed aliases, including `PoolMode` and `LocalPool`. PostgreSQL users can opt in through the usual builder flow without changing `Config` shape.

Gateway-style usage is represented by tests that build primary `Fast` and timeout `Clean` PostgreSQL pools, verify timeout error behavior, status shape, retain calls, statement-cache lifecycle, detach, failed transaction recovery, and clean/custom recycling with core-local mode enabled.

## User Guide

### Prerequisites

Enable the `core-local` feature in `deadpool` or `deadpool-postgres`.

### Basic Usage

Managed pools:

```rust
let pool = MyPool::builder(manager)
    .pool_mode(deadpool::PoolMode::CoreLocal)
    .build()?;
let local = pool.local();
let object = local.get().await?;
```

Unmanaged pools:

```rust
let pool = deadpool::unmanaged::Pool::new_with_mode(16, deadpool::PoolMode::CoreLocal);
let local = pool.local();
local.add(object).await?;
let object = local.get().await?;
```

PostgreSQL pools:

```rust
let pool = cfg
    .builder(tokio_postgres::NoTls)?
    .pool_mode(deadpool_postgres::PoolMode::CoreLocal)
    .build()?;
let local = pool.local();
let client = local.get().await?;
```

### Advanced Usage

Create one local handle per core, shard, or thread-per-core executor lane and keep checkout/checkin on that handle when possible. If an object returns after its origin local handle has been dropped, Deadpool releases capacity instead of retaining the object locally.

Use `PoolMode::Shared` as the rollback path if a deployment observes imbalance, unexpected local-handle churn, or runtime behavior that does not preserve intended affinity.

## API Reference

### Key Components

- `PoolMode::Shared`: default existing behavior.
- `PoolMode::CoreLocal`: explicit local-handle mode gated by `core-local`.
- `managed::LocalPool`: managed local handle.
- `unmanaged::LocalPool`: unmanaged local handle.
- `deadpool-postgres/core-local`: downstream feature forwarding to core Deadpool.

### Configuration Options

Core-local mode is selected through builders and constructors:

- Managed: `Pool::builder(manager).pool_mode(PoolMode::CoreLocal)` or `.core_local()`
- Unmanaged: `Pool::new_with_mode`, `Pool::from_config_with_mode`, or `Pool::from_iter_with_mode`
- PostgreSQL: `Config::builder(...).pool_mode(deadpool_postgres::PoolMode::CoreLocal)`

## Testing

### How to Test

Core crate checks:

- `cd crates/deadpool && cargo test --all-features`
- `cd crates/deadpool && cargo test --all-features --test managed_core_local_stress -- --ignored`
- `cd crates/deadpool && cargo test --all-features --test unmanaged_core_local_stress -- --ignored`
- `cd crates/deadpool && cargo bench --no-run --all-features`

PostgreSQL checks:

- `cd crates/deadpool-postgres && cargo test --no-default-features --features core-local,rt_tokio_1 --test core_local_api`
- `cd crates/deadpool-postgres && cargo test --features serde,core-local,rt_tokio_1,rt_async-std_1` when PostgreSQL service variables are configured.

### Edge Cases

Tests cover local returns, cross-handle visibility, local handle drop, object take/detach, close/drain, timeout behavior, resize, retain, create/recycle failure, hook failure, status under load, and shared-vs-core-local stress gates.

## Limitations and Future Work

Core-local mode does not infer runtime worker identity. It does not guarantee strict waiter fairness or global queue ordering across local handles. Lifecycle operations and shared fallback paths are outside the strict same-handle lockfree guarantee. PostgreSQL session-reset behavior remains in `deadpool-postgres` recycling methods rather than the generic core.
