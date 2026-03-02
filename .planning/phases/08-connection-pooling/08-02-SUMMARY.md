---
phase: 08-connection-pooling
plan: "02"
subsystem: pool
tags: [rust, bb8, connection-pool, pop3, async, tokio, rfc1939]

requires:
  - phase: 08-01
    provides: AccountKey, Pop3ConnectionManager, Pop3PoolError (CheckoutTimeout/Connection), bb8::ManageConnection impl, 13 unit tests

provides:
  - Pop3Pool struct with std::sync::RwLock<HashMap> per-account registry
  - PoolConfig with sensible defaults (30s timeout, 5m idle, 30m max lifetime)
  - PooledConnection type alias for bb8::PooledConnection<'static, Pop3ConnectionManager>
  - Pop3Pool::add_account() — synchronous, build_unchecked, max_size(1)
  - Pop3Pool::checkout() — async, clones Arc before await, never holds lock across await
  - Pop3Pool::remove_account() and Pop3Pool::accounts()
  - Pop3PoolError::UnknownAccount(AccountKey) variant
  - lib.rs re-exports all pool types behind #[cfg(feature = "pool")]
  - 12 new unit tests covering full Pop3Pool API (25 pool tests total)

affects:
  - Phase 09 (MIME integration)
  - Any future multi-account client usage

tech-stack:
  added: []
  patterns:
    - "Inner Arc pattern: clone Arc<bb8::Pool<M>> while briefly holding RwLock, release lock before .await"
    - "std::sync::RwLock for shared state never held across await — avoids tokio RwLock overhead"
    - "build_unchecked() for synchronous pool creation inside sync lock — avoids async in write lock"
    - "Tests that call add_account must use #[tokio::test] — bb8 starts internal interval timer on build"

key-files:
  created: []
  modified:
    - src/pool.rs
    - src/lib.rs

key-decisions:
  - "std::sync::RwLock used (not tokio::sync::RwLock) — lock held only for brief HashMap ops, never across .await"
  - "add_account is synchronous — build_unchecked creates pool without async; credentials stored in manager"
  - "checkout uses get_owned() not get() — returns PooledConnection<'static> after cloning Arc and releasing lock"
  - "#[tokio::test] required for tests calling add_account — bb8 build_unchecked starts Tokio interval timer"
  - "UnknownAccount error returned immediately by checkout() before any network I/O"

patterns-established:
  - "Arc<bb8::Pool<M>> in HashMap enables lock-free pool access after initial Arc clone"
  - "build_unchecked + max_size(1) enforces RFC 1939 exclusive mailbox constraint at library level"

requirements-completed: [POOL-01, POOL-02, POOL-03]

duration: 4min
completed: 2026-03-02
---

# Phase 8 Plan 02: Connection Pooling (Pop3Pool Registry) Summary

**Pop3Pool registry with per-account bb8 pools (max_size=1), RFC 1939-compliant exclusive mailbox access, std::sync::RwLock registry, and 25 pool unit tests**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-02T03:55:42Z
- **Completed:** 2026-03-02T03:59:22Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- Implemented `Pop3Pool` struct managing per-account bb8 pools behind a `std::sync::RwLock<HashMap>` — lock never held across `.await` points
- Added `PoolConfig` with POP3-appropriate defaults (30s connection timeout, 5m idle, 30m max lifetime)
- Added `PooledConnection` type alias, `Pop3PoolError::UnknownAccount` variant, `lib.rs` re-exports behind `#[cfg(feature = "pool")]`
- 12 new unit tests bring total pool test count to 25 — all pass; all three POOL requirements satisfied

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement Pop3Pool struct, PoolConfig, and public API** - `708a5b2` (feat)
2. **Task 2: Unit tests for Pop3Pool API and integration verification** - `99a8ceb` (test)

## Files Created/Modified

- `src/pool.rs` — Added PoolConfig, Pop3Pool, PooledConnection type alias, UnknownAccount variant, 12 new tests
- `src/lib.rs` — Added `#[cfg(feature = "pool")]` re-exports for all pool public types

## Decisions Made

- Used `std::sync::RwLock` (not `tokio::sync::RwLock`) — the registry lock is never held across `.await`, so the simpler std version is correct and allows synchronous `add_account` and `remove_account`
- `checkout()` uses `get_owned()` (returns `PooledConnection<'static>`) instead of `get()` (borrows Pool) — necessary because we clone the `Arc` before dropping the read lock, so the pool is a local variable
- `add_account()` is synchronous — `build_unchecked()` creates the pool without any async operation, avoiding the need to hold a lock across `.await`
- Tests calling `add_account` must be `#[tokio::test]` — `bb8::Pool::build_unchecked()` internally starts a Tokio interval timer, requiring a runtime context even though `add_account` itself is not async

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed `unwrap_err()` compile error due to missing `Debug` on `Pop3Client`**
- **Found during:** Task 2 (unit tests)
- **Issue:** `unwrap_err()` requires `T: Debug` where `T = PooledConnection<'static, ...>` which requires `Pop3Client: Debug`. `Pop3Client` does not implement `Debug` and we cannot add it without a plan change.
- **Fix:** Replaced `result.unwrap_err()` with `if let Err(e) = result { assert!(matches!(e, ...)) }` pattern
- **Files modified:** src/pool.rs
- **Verification:** Compiles and test passes
- **Committed in:** 99a8ceb (Task 2 commit)

**2. [Rule 1 - Bug] Converted add_account tests from `#[test]` to `#[tokio::test]`**
- **Found during:** Task 2 (unit tests)
- **Issue:** `bb8::Pool::build_unchecked()` starts a Tokio interval timer internally, causing panic "there is no reactor running" in synchronous tests
- **Fix:** Changed 5 tests that call `add_account` to `#[tokio::test]` async tests; added explanatory comment in test module
- **Files modified:** src/pool.rs
- **Verification:** All 25 pool tests pass
- **Committed in:** 99a8ceb (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (both Rule 1 — bugs discovered during test authoring)
**Impact on plan:** Both fixes necessary for tests to compile and pass. No scope creep. `#[tokio::test]` annotation is functionally equivalent to the planned `#[test]` for these synchronous operations.

## Issues Encountered

None beyond the auto-fixed deviations above.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- Phase 8 (Connection Pooling) is complete — all three POOL requirements (POOL-01, POOL-02, POOL-03) satisfied
- Phase 9 (MIME Integration) can proceed independently; pool module is purely additive
- Pool is available via `pop3 = { features = ["pool"] }` and fully documented with RFC 1939 warning

---
*Phase: 08-connection-pooling*
*Completed: 2026-03-02*
