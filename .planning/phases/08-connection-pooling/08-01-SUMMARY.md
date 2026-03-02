---
phase: 08-connection-pooling
plan: "01"
subsystem: networking
tags: [bb8, connection-pool, pop3, tokio, rwlock, rfc1939]

# Dependency graph
requires:
  - phase: 04-protocol-extensions
    provides: Pop3ClientBuilder with Clone + Debug, builder pattern for connections
  - phase: 05-pipelining
    provides: Pop3Client.is_closed(), Pop3Client.noop() for health checks
provides:
  - Pop3Pool: multi-account bb8 connection registry with max_size(1) per account
  - Pop3ConnectionManager: bb8 ManageConnection impl for Pop3Client
  - Pop3PoolError: pool-scoped error enum with Client/Pool/NoCredentials/AccountNotFound variants
  - AccountKey: (host, port, username) identity struct with Display
  - Pop3PoolConfig: connection_timeout + test_on_check_out configuration
  - pool feature flag gating all bb8 dependencies
affects:
  - 08-02-PLAN.md (tests for all pool types/methods)

# Tech tracking
tech-stack:
  added: [bb8 0.9 (optional, feature-gated)]
  patterns:
    - tokio::sync::RwLock<HashMap> for async-safe mutable pool registry
    - Arc<bb8::Pool<M>> per account for shared ownership across async tasks
    - build_unchecked() for lazy synchronous pool construction (no TCP at registration time)
    - #[cfg(feature = "pool")] gating on builder accessor methods
    - Manual From<bb8::RunError<E>> impl to avoid conflicting blanket impls

key-files:
  created:
    - src/pool.rs
  modified:
    - Cargo.toml
    - src/builder.rs
    - src/lib.rs

key-decisions:
  - "Pop3PoolError is a standalone enum in pool.rs (not added to Pop3Error) — respects feature flag boundary"
  - "hostname() and username() builder accessors gated with #[cfg(feature = pool)] to suppress dead_code warnings in base builds"
  - "add_account() is async (requires tokio::sync::RwLock write) with idempotent or_insert semantics"
  - "get() uses get_owned() (returns PooledConnection<'static, M>) not get() — Arc keeps pool alive after guard drops"
  - "From<bb8::RunError<Pop3Error>> maps User(e) to Client(e) for ergonomic matching, TimedOut to Pool variant"

patterns-established:
  - "Feature-gated module: #[cfg(feature = X)] pub mod X in lib.rs — same pattern as reconnect"
  - "Pool-level error type scoped to the module (Pop3PoolError) not added to core Pop3Error"
  - "bb8::Pool per account (not per server) with max_size(1) to enforce RFC 1939 mailbox exclusivity"

requirements-completed: [POOL-01, POOL-02, POOL-03]

# Metrics
duration: 3min
completed: 2026-03-01
---

# Phase 8 Plan 01: Connection Pool Core Summary

**bb8-backed Pop3Pool registry with per-account max_size(1), tokio RwLock HashMap, and RFC 1939 documentation throughout**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-01T00:31:16Z
- **Completed:** 2026-03-01T00:34:36Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Created `src/pool.rs` with complete `Pop3Pool`, `Pop3ConnectionManager`, `Pop3PoolError`, `AccountKey`, and `Pop3PoolConfig` types
- `Pop3ConnectionManager` implements `bb8::ManageConnection` with connect() calling builder, is_valid() sending NOOP, has_broken() checking is_closed()
- `Pop3Pool` uses `tokio::sync::RwLock<HashMap<AccountKey, Arc<bb8::Pool<M>>>>` — async-aware, no DashMap deadlock footgun, no new dependency
- Module and struct rustdoc prominently warns about RFC 1939 exclusive-lock constraint (satisfies POOL-03)
- Pool feature flag gates bb8 — base crate compiles without any bb8 dependency (satisfies POOL-01 gating requirement)
- Both `cargo clippy` (with and without pool feature) and `cargo fmt --check` pass with zero warnings

## Task Commits

Each task was committed atomically:

1. **Task 1: Add bb8 dependency, builder accessors, and create pool.rs** - `766945f` (feat)
2. **Task 2: Wire lib.rs re-exports and run full lint/fmt pass** - `e539ec3` (feat)

## Files Created/Modified

- `src/pool.rs` - Complete pool module: AccountKey, Pop3PoolConfig, Pop3PoolError, Pop3ConnectionManager, Pop3Pool
- `Cargo.toml` - Added `pool = ["dep:bb8"]` feature and `bb8 = { version = "0.9", optional = true }` dependency
- `src/builder.rs` - Added `pub(crate) hostname()`, `effective_port()` (already existed, kept pub(crate)), `username()` methods (all #[cfg(feature = "pool")] gated)
- `src/lib.rs` - Added `#[cfg(feature = "pool")] pub mod pool;`

## Decisions Made

- **Pop3PoolError lives in pool.rs, not error.rs** — error type scoped to the feature boundary; core Pop3Error stays unchanged
- **Builder accessor methods gated with `#[cfg(feature = "pool")]`** — prevents dead_code warnings in base builds where pool.rs is not compiled; cleaner than `#[allow(dead_code)]`
- **`add_account()` is async** — required by tokio::sync::RwLock write lock; keeps API straightforward without blocking wrappers
- **`get()` uses `get_owned()`** — returns `PooledConnection<'static, M>` allowing the RwLock guard to be dropped before the bb8 checkout, while Arc keeps the pool alive
- **Manual `From<bb8::RunError<Pop3Error>>` impl** — maps `RunError::User(e)` to `Pop3PoolError::Client(e)` for ergonomic auth failure matching; avoids conflicting `#[from]` blanket impls that would conflict with the `Client(#[from] Pop3Error)` variant

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None — the Windows MSVC linker (dlltool.exe not found) prevented `cargo build` from linking, but `cargo check` confirmed all Rust-level compilation is correct. This is the pre-existing environment constraint documented in STATE.md and does not affect correctness.

## Next Phase Readiness

- Plan 02 (`08-02-PLAN.md`) can proceed: adds unit tests for all pool types using mock infrastructure
- All public types (Pop3Pool, Pop3ConnectionManager, Pop3PoolError, AccountKey, Pop3PoolConfig) have full rustdoc coverage
- Pool is ready for testing: add_account, get, remove_account, contains_account, accounts, pool_count all implemented

---
*Phase: 08-connection-pooling*
*Completed: 2026-03-01*
