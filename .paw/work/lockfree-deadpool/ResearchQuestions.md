# Research Questions: Lockfree Deadpool

Target Branch: users/andrewkhoma/lockfree
Issue URL: none

## Agent Notes from Intake

- User wants the current Deadpool library updated on the current branch to support a lockfree implementation suitable for a thread-per-core application model.
- Public Deadpool API compatibility should be preserved where practical, with opt-in application-specific extensions allowed.
- "Lockfree" means checkout/checkin steady-state hot paths should avoid blocking locks or scheduler parking; bounded synchronization is acceptable for initialization, shutdown, resizing, and error recovery.
- Deliverable scope is the Deadpool library in this repository. The gateway under `.paw/documentdb/pg_documentdb_gw` should be used as reference/validation context only.
- The drafted PostgreSQL-oriented analogue under `.paw/documentdb_core_local_pool` is a local reference for the desired model.
- External context may exist in ADO PR `https://msdata.visualstudio.com/CosmosDB/_git/pgmongo/pullrequest/2031705`; only request that it be pulled under `.paw` if local references are insufficient.

## Internal System Behavior Questions

1. What public APIs, trait contracts, feature flags, and crate boundaries does the current Deadpool library expose that are relevant to pool checkout/checkin behavior and PostgreSQL integration?
2. Where does the current pool implementation use blocking synchronization, async-aware locks, channels, semaphores, wait queues, or other contention points on checkout/checkin hot paths?
3. What lifecycle operations exist today for pool creation, object construction, recycling, timeouts, shutdown/drop, status reporting, resizing, and error recovery, and which of those can tolerate bounded synchronization?
4. How does the current implementation preserve fairness, timeout behavior, backpressure, recycling semantics, and object validity across concurrent checkout/checkin operations?
5. What test coverage currently exists for pool behavior, concurrency, timeouts, recycling, manager errors, PostgreSQL-specific behavior, and public API compatibility?
6. What architecture and behavior does `.paw/documentdb_core_local_pool` implement for PostgreSQL pooling, especially around per-core state, lockfree/local queues, ownership transfer, and connection lifecycle management?
7. How does `.paw/documentdb/pg_documentdb_gw` use Deadpool or PostgreSQL pooling today, and what integration behaviors matter for a thread-per-core gateway model?
8. Are there existing benchmark, stress, or integration-test entry points in this repository or the provided gateway reference that can measure checkout/checkin latency, contention, and correctness under multi-threaded or per-core workloads?
9. Which parts of the reference pool can be generalized safely into Deadpool without coupling the library to the gateway or PostgreSQL-specific assumptions?
10. Are there repository documentation or contribution conventions that should be updated when changing the pool architecture or adding opt-in lockfree behavior?

## Optional External/Context Questions

1. If local references do not explain the intended thread-per-core constraints or gateway integration contract, ask the user to pull the ADO PR context under `.paw` before planning implementation details.
