# Feature Specification: Lockfree Deadpool

**Branch**: users/andrewkhoma/lockfree  |  **Created**: 2026-06-09  |  **Status**: Draft
**Input Brief**: Make the Deadpool library suitable for lockfree steady-state use in a thread-per-core gateway while preserving existing API compatibility where practical.

## Overview

Applications that run a thread-per-core or core-affine request model need database pool checkout and return behavior that does not introduce centralized blocking on the request hot path. The gateway reference depends on Deadpool-backed PostgreSQL pools today, so the library needs a path that can serve these workloads without forcing an application rewrite or coupling the generic pool to gateway-specific assumptions.

The feature introduces an opt-in lockfree pool behavior for steady-state checkout and checkin, focused on the case where capacity and reusable objects are already available. Existing Deadpool users should continue to rely on the current public API and observable behaviors, while thread-per-core users can choose the new behavior where it fits their runtime model.

The lockfree guarantee is intentionally scoped to ordinary object checkout and return after the pool is initialized and operating normally. Lifecycle and maintenance activities such as initialization, resizing, shutdown, pruning, status collection, connection creation, recycling failures, and recovery may use bounded synchronization as long as that boundary is documented and does not change existing user-facing behavior.

## Objectives

- Enable thread-per-core applications to use Deadpool with non-blocking steady-state checkout and checkin behavior.
- Preserve existing managed, unmanaged, and PostgreSQL integration behavior where practical so downstream users can migrate incrementally.
- Keep lockfree behavior opt-in, explicit, and documented rather than changing default pool semantics unexpectedly.
- Maintain existing backpressure, timeout, recycling, close, status, and error behaviors from a user perspective.
- Provide validation that the new behavior is correct under concurrent use and useful for the provided gateway reference.

## User Scenarios & Testing

### User Story P1 – Thread-Per-Core Pool Hot Path

Narrative: As a gateway operator running a core-affine request model, I want repeated pool checkout and return operations to avoid blocking synchronization in steady state so request handling can stay local and predictable.

Independent Test: Configure the opt-in lockfree behavior with available capacity and reusable objects, then run repeated concurrent checkout/checkin operations and confirm they complete without deadlock, permit leaks, or lifecycle errors.

Acceptance Scenarios:
1. Given an initialized opt-in lockfree pool with reusable objects available, When workers repeatedly check out and return objects within configured capacity, Then the operations complete without waiting on lifecycle synchronization and without losing capacity.
2. Given all configured capacity is in use, When another worker requests an object, Then existing backpressure and timeout behavior is preserved.
3. Given a checked-out object is returned, When it becomes eligible for reuse, Then it can be acquired again without requiring an application-level handoff or gateway-specific code.

### User Story P2 – Existing Deadpool Compatibility

Narrative: As an existing Deadpool user, I want current managed, unmanaged, and PostgreSQL pooling behavior to remain compatible so upgrading the library does not force broad application changes.

Independent Test: Run existing behavioral and public-API tests for the core and PostgreSQL crates without requiring source changes in downstream-style usage.

Acceptance Scenarios:
1. Given code using existing managed pool checkout and object-return behavior, When the library is upgraded, Then that code continues to compile and observe the same pool lifecycle semantics unless it opts into new behavior.
2. Given code using existing unmanaged pool add/get/remove behavior, When the library is upgraded, Then that code continues to observe the same pool capacity and close semantics.
3. Given PostgreSQL integration code using Deadpool-backed pools, When the library is upgraded, Then PostgreSQL pool construction, checkout, recycling, status, and error behavior remain compatible.

### User Story P3 – Predictable Lifecycle Boundaries

Narrative: As a library maintainer or advanced user, I want the lockfree guarantee boundaries to be explicit so I can reason about lifecycle operations separately from request hot paths.

Independent Test: Exercise initialization, resize, close, status, object creation, recycling failure, and timeout paths and confirm they remain correct even when bounded synchronization is used.

Acceptance Scenarios:
1. Given the pool is being initialized, resized, closed, or maintained, When lifecycle operations run, Then they may coordinate globally but must preserve documented pool results.
2. Given object creation or recycling fails, When the pool recovers or reports an error, Then capacity accounting and future checkout behavior remain correct.
3. Given status is queried during load, When the pool reports its state, Then existing eventual-consistency expectations are preserved.

### User Story P4 – Gateway-Oriented Validation Without Gateway Ownership

Narrative: As the gateway application owner, I want Deadpool’s new behavior to match the provided gateway and reference-pool constraints without requiring the gateway tree itself to be modified by this work.

Independent Test: Validate the library against the gateway’s current pooling expectations and the reference pool’s thread-per-core behavior model without committing gateway source changes.

Acceptance Scenarios:
1. Given the gateway uses Deadpool-backed PostgreSQL pools with timeout and status behavior, When the updated library is evaluated as a dependency, Then those expectations remain available.
2. Given the reference pool demonstrates per-core idle reuse and shared capacity constraints, When planning and implementation adapt generic behavior, Then PostgreSQL-specific assumptions are kept out of Deadpool core APIs.

### Edge Cases

- Capacity exhaustion must preserve existing try, wait, and timeout outcomes.
- Object creation failure must not leak capacity or prevent later successful checkout.
- Recycling failure must discard invalid objects and preserve later checkout behavior.
- Returning or detaching objects must not double-release capacity.
- Closing a pool while objects are checked out must preserve existing close and return semantics.
- Status queries under load may remain eventually consistent but must not panic or corrupt accounting.
- Opt-in lockfree behavior must have documented behavior when the runtime model does not provide stable local execution.
- Existing queue-order configuration must either be preserved or have any opt-in-mode limitation explicitly documented.

## Requirements

### Functional Requirements

- FR-001: The library MUST provide an opt-in behavior suitable for lockfree steady-state checkout and checkin in thread-per-core workloads. (Stories: P1)
- FR-002: The default behavior MUST preserve existing managed, unmanaged, and PostgreSQL integration semantics unless the user explicitly opts into new behavior. (Stories: P2)
- FR-003: In the opt-in behavior, checkout/checkin MUST avoid blocking locks, async lock waits, runtime parking, or centralized contention when operating within available steady-state capacity. (Stories: P1)
- FR-004: The pool MUST preserve configured maximum-capacity backpressure, including immediate failure paths and timeout paths. (Stories: P1, P2)
- FR-005: Managed pool object creation, recycling, detaching, drop-return, hook, and error behaviors MUST remain compatible from a user perspective. (Stories: P2, P3)
- FR-006: Unmanaged pool add, try-add, get, try-get, timed-get, remove, close, and status behaviors MUST remain compatible from a user perspective. (Stories: P2)
- FR-007: PostgreSQL integration MUST continue to expose compatible pool construction, checkout, recycling, statement-cache, status, and error behavior. (Stories: P2, P4)
- FR-008: The opt-in behavior MUST support local idle reuse and shared global capacity accounting without adding gateway-specific or PostgreSQL-specific requirements to the core pool abstraction. (Stories: P1, P4)
- FR-009: Lifecycle operations MAY use bounded synchronization, but their boundary relative to the lockfree guarantee MUST be documented. (Stories: P3)
- FR-010: Pool status and metrics behavior MUST remain safe and compatible under concurrent load, including existing eventual-consistency expectations. (Stories: P2, P3)
- FR-011: Existing feature-gated runtime and configuration behavior MUST remain compatible for supported feature combinations. (Stories: P2)
- FR-012: The repository MUST include tests or benchmarks that exercise the new opt-in behavior under concurrent checkout/checkin and capacity pressure. (Stories: P1, P4)
- FR-013: Documentation MUST explain how users select the new behavior, what compatibility is preserved, and which operations are outside the steady-state lockfree guarantee. (Stories: P2, P3)
- FR-014: PostgreSQL users MUST be able to opt into the core-local behavior through a `deadpool-postgres` feature path that forwards the core Deadpool feature and remains covered by downstream crate verification. (Stories: P2, P4)

### Key Entities

- Pool: A bounded collection of reusable user-managed or manager-created objects.
- Managed Pool: A pool that creates and recycles objects through a manager contract.
- Unmanaged Pool: A pool where users explicitly add and remove reusable objects.
- Checked-Out Object: A temporary owner of pooled capacity that returns or detaches the object according to existing rules.
- Opt-In Lockfree Behavior: A user-selected pool mode intended to avoid blocking synchronization on steady-state checkout/checkin.
- Local Pool Handle: An explicit user-created handle representing one core/local execution context; handles share global capacity but own their local idle state.
- Local Idle State: Reusable object availability associated with a local pool handle.
- Shared Capacity: Global accounting that prevents the pool from exceeding configured maximum size.

### Cross-Cutting / Non-Functional

- Compatibility: Existing public behavior and feature combinations should remain source-compatible where practical.
- Correctness: Capacity accounting must remain accurate across checkout, return, detach, close, timeout, creation failure, and recycling failure.
- Performance: The new behavior must be measurable through repository tests or benchmarks focused on concurrent checkout/checkin, with comparative pass/fail gates against existing shared-mode behavior.
- Documentation: Lockfree guarantees and exclusions must be explicit enough for users to decide whether the opt-in mode fits their runtime model.

## Success Criteria

- SC-001: Existing managed, unmanaged, and PostgreSQL pool behavior remains compatible as demonstrated by the repository’s current behavioral tests. (FR-002, FR-005, FR-006, FR-007, FR-011)
- SC-002: New automated coverage demonstrates repeated concurrent checkout/checkin with opt-in lockfree behavior without deadlocks, lost capacity, or double returns. (FR-001, FR-003, FR-008, FR-012)
- SC-003: Capacity exhaustion, timeout, creation failure, recycling failure, close, detach, and status edge cases are covered for the new behavior or explicitly shown to reuse existing behavior. (FR-004, FR-005, FR-006, FR-009, FR-010)
- SC-004: Downstream-style PostgreSQL pool usage remains compatible without modifying the gateway reference tree. (FR-007, FR-008)
- SC-005: Documentation clearly distinguishes steady-state lockfree checkout/checkin from lifecycle operations that may use bounded synchronization. (FR-009, FR-013)
- SC-006: Benchmark or stress-test entry points compare checkout/checkin behavior under concurrent and thread-per-core-like workloads and record comparative results. (FR-003, FR-012)
- SC-007: For pre-populated steady-state checkout/checkin at 1, 8, 16, and 32 workers, the opt-in core-local mode shows zero observed blocking/parking on the local hot path, p99 checkout latency no worse than shared mode, and throughput no worse than shared mode. (FR-001, FR-003, FR-012)
- SC-008: `deadpool-postgres` feature-forwarding, docs, and tests demonstrate that PostgreSQL users can opt into core-local behavior through the downstream crate without gateway source changes. (FR-007, FR-014)

## Assumptions

- The provided local reference pool and gateway reference contain enough context to proceed without pulling the external ADO PR unless later code research finds a gap.
- Opt-in behavior is acceptable and preferred over changing default behavior for all Deadpool users.
- Strict waiter fairness is not a required public behavior unless code research later identifies a stronger compatibility contract.
- Gateway source changes are outside this work; the gateway is used for validation context only.
- Thread-per-core suitability is represented by explicit per-core/local pool handles sharing global capacity; automatic runtime-worker or task-local sharding is not assumed.
- Checked-out objects from core-local handles carry their origin local handle identity. Same-handle return is the steady-state lockfree path; cross-handle or migrated-task return is outside that guarantee and must use a documented safe reclamation path that returns the object to its origin handle when possible or releases/detaches capacity if the origin handle is closed or inactive.

## Scope

In Scope:
- Deadpool core pooling behavior needed for opt-in lockfree steady-state checkout/checkin.
- Managed and unmanaged pool compatibility.
- PostgreSQL integration compatibility as a downstream user of the managed pool API.
- `deadpool-postgres` feature forwarding and verification for the opt-in core-local mode.
- Tests, benchmarks, and documentation directly related to the new behavior.
- Use of `.paw` reference trees to understand gateway and reference-pool expectations.

Out of Scope:
- Rewriting or committing changes to the gateway reference tree.
- Requiring the external ADO PR as an input unless local references prove insufficient.
- Moving PostgreSQL-specific session-reset behavior into the generic Deadpool core.
- Guaranteeing lockfree behavior for initialization, shutdown, resizing, maintenance, status collection, object creation, or error recovery.
- Automatically inferring runtime-worker affinity for a single shared pool without explicit local handles.
- Introducing strict waiter fairness unless required for existing compatibility.

## Dependencies

- Existing Deadpool core managed and unmanaged pool contracts.
- Existing Deadpool PostgreSQL integration behavior.
- Provided local reference pool for the thread-per-core behavior model.
- Provided gateway reference for downstream compatibility expectations.
- Existing repository tests, examples, benchmarks, and feature-gated CI patterns.

## Risks & Mitigations

- Public API compatibility regression: Existing users may rely on subtle behavior. Mitigation: keep the new behavior opt-in and run existing API and behavior tests.
- Capacity-accounting errors under concurrency: Lost or duplicate permits could cause deadlocks or over-capacity use. Mitigation: add stress coverage for checkout, return, detach, close, timeout, and failure paths.
- Runtime model mismatch: A generic library may not know whether execution is truly core-affine. Mitigation: document the required runtime assumptions for the opt-in behavior.
- Local-handle misuse: Applications may return or share objects across local handles. Mitigation: define the cross-local return contract, fail-safe behavior, and stress tests before implementation.
- PostgreSQL coupling: The reference pool contains PostgreSQL-specific behavior that should not leak into the generic core. Mitigation: separate generic pool requirements from PostgreSQL integration compatibility requirements.
- Incomplete external context: The referenced ADO PR may contain additional constraints. Mitigation: proceed with local references and ask for the PR to be pulled under `.paw` only if code research identifies missing requirements.

## References

- User intake: Current PAW workflow request on 2026-06-09.
- Research: `.paw/work/lockfree-deadpool/SpecResearch.md`
- Reference inputs: `.paw/documentdb_core_local_pool`, `.paw/documentdb/pg_documentdb_gw`
