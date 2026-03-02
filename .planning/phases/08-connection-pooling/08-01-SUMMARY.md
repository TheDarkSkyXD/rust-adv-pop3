---
phase: 08-connection-pooling
plan: "01"
subsystem: pool
tags: [bb8, connection-pool, rust, tokio, thiserror]

# Dependency graph
requires:
  - phase: 07-reconnection
    provides: Pop3Client.is_closed() for has_broken() check
  - phase: 04-protocol-extensions
    provides: Pop3ClientBuilder with Clone + Debug derives
provides:
  - AccountKey struct (Debug, Clone, PartialEq, Eq, Hash) for use as HashMap key
  - Pop3ConnectionManager implementing bb8::ManageConnection
  - Pop3PoolError enum with CheckoutTimeout and Connection(Pop3Error) variants
  - From<bb8::RunError<Pop3Error>> for Pop3PoolError
affects:
  - 08-02-connection-pooling (Pop3Pool registry struct)

# Tech tracking
tech-stack:
  added: [bb8 = "0.9" (default-features = false, optional), parking_lot disabled via default-features = false]
  patterns: [feature-gated optional dependency, bb8::ManageConnection impl with native impl Future return types]

key-files:
  created: [src/pool.rs]
  modified: [Cargo.toml, src/lib.rs]

key-decisions:
  - "bb8 added with default-features = false to disable parking_lot feature — avoids dlltool.exe Windows GNU toolchain constraint"
  - "pool module gated behind #[cfg(feature = pool)] in lib.rs — pub mod pool only compiled when feature active"
  - "is_valid() returns conn.noop() directly — avoids redundant async block (clippy::redundant_async_block)"
  - "Pop3PoolError is separate from Pop3Error — pool-level errors (checkout timeout) conceptually distinct from POP3 protocol errors"
  - "Pop3ConnectionManager clones builder+credentials into locals before async move in connect() — ensures returned Future is Send without borrowing &self"

patterns-established:
  - "Pattern: bb8::ManageConnection impl uses owned clones before async move for Send futures"
  - "Pattern: is_valid() delegates to client method directly (no intermediate async block)"
  - "Pattern: Pop3PoolError maps bb8::RunError variants via From impl for caller ergonomics"

requirements-completed: [POOL-01, POOL-02]

# Metrics
duration: 5min
completed: 2026-03-02
---

# Phase 8 Plan 01: Connection Pooling Foundation Summary

**bb8-backed POP3 pool foundation with AccountKey (Hash+Eq), Pop3ConnectionManager (ManageConnection), and Pop3PoolError (CheckoutTimeout/Connection) behind optional `pool` feature flag**

## Performance

- **Duration:** ~5 min
- **Started:** 2026-03-02T03:46:19Z
- **Completed:** 2026-03-02T03:51:13Z
- **Tasks:** 2
- **Files modified:** 3 (Cargo.toml, src/lib.rs, src/pool.rs)

## Accomplishments

- Added `bb8 = { version = "0.9", default-features = false, optional = true }` and `pool = ["dep:bb8"]` feature flag
- Created `src/pool.rs` with `AccountKey`, `Pop3ConnectionManager`, and `Pop3PoolError`
- Implemented `bb8::ManageConnection` for `Pop3ConnectionManager` with `connect()`, `is_valid()`, and `has_broken()`
- All 13 unit tests pass (AccountKey eq/hash/debug/clone, Pop3PoolError display/from, has_broken live/closed)
- Clippy clean with `-D warnings`, `cargo fmt` clean

## Task Commits

Each task was committed atomically:

1. **Task 1 + Task 2: Add bb8 dependency, pool.rs with all types and unit tests** - `817b0ae` (feat)

_Note: Task 1 and Task 2 were developed together — implementation and tests written in the same pool.rs file and committed in a single atomic unit. All 13 tests pass._

## Files Created/Modified

- `src/pool.rs` — AccountKey, Pop3ConnectionManager (ManageConnection impl), Pop3PoolError, 13 unit tests
- `Cargo.toml` — bb8 optional dependency (default-features = false), pool feature flag
- `src/lib.rs` — #[cfg(feature = "pool")] pub mod pool declaration

## Decisions Made

- **bb8 default-features = false**: The bb8 crate enables `parking_lot` by default, which requires `dlltool.exe` on the Windows GNU toolchain (pre-existing environment constraint). Disabling default features avoids the C dependency while retaining full async pool functionality.
- **is_valid() returns conn.noop() directly**: Initial implementation used `async move { noop_fut.await }` which clippy flagged as `redundant_async_block`. Returning `conn.noop()` directly satisfies the trait and passes clippy.
- **No is_valid lifetime annotation**: Attempting to add named lifetimes (`'life0, 'life1`) caused E0195 because the trait declares `is_valid` without explicit lifetimes. The direct `conn.noop()` return approach works cleanly without lifetime annotations.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Disabled bb8 parking_lot feature to fix Windows build**
- **Found during:** Task 1 (cargo build --features pool,rustls-tls)
- **Issue:** bb8 0.9's default `parking_lot` feature pulls in `parking_lot_core` which requires `dlltool.exe` on Windows GNU toolchain — `error: error calling dlltool 'dlltool.exe': program not found`
- **Fix:** Changed `bb8 = { version = "0.9", optional = true }` to `bb8 = { version = "0.9", default-features = false, optional = true }` — disables parking_lot feature while keeping full bb8 async functionality
- **Files modified:** Cargo.toml
- **Verification:** `cargo build --features pool,rustls-tls` succeeds; `cargo test --features pool,rustls-tls pool` runs all 13 tests
- **Committed in:** 817b0ae (Task 1 commit)

**2. [Rule 1 - Bug] Fixed is_valid() to return conn.noop() directly**
- **Found during:** Task 1 (clippy verification)
- **Issue:** Initial `async move { noop_fut.await }` triggered `clippy::redundant_async_block`; lifetime-annotated signature triggered E0195
- **Fix:** Return `conn.noop()` directly with no named lifetimes — the future borrows `conn` but satisfies the Send bound since Pop3Client is Send
- **Files modified:** src/pool.rs
- **Verification:** `cargo clippy --features pool,rustls-tls -- -D warnings` passes with zero warnings
- **Committed in:** 817b0ae (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (both Rule 1 - Bug)
**Impact on plan:** Both fixes essential for compilation and lint compliance on the target environment. No scope creep.

## Issues Encountered

- **bb8 parking_lot + Windows GNU**: Pre-existing environment constraint (documented in STATE.md). Resolved by disabling bb8's `parking_lot` default feature — uses standard Tokio synchronization primitives instead.
- **is_valid() lifetime puzzle**: bb8's trait declares `is_valid` without explicit lifetime parameters; adding them triggers E0195. Solved by returning `conn.noop()` directly which the compiler can infer correctly.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `src/pool.rs` is ready for Plan 02 which adds `Pop3Pool` registry struct with `add_account()` and `checkout()` API
- `AccountKey` is available as HashMap key type
- `Pop3ConnectionManager` is fully functional — `connect()` builds+authenticates, `is_valid()` sends NOOP, `has_broken()` checks is_closed()
- `Pop3PoolError` From impl ready for caller error handling

---
*Phase: 08-connection-pooling*
*Completed: 2026-03-02*
