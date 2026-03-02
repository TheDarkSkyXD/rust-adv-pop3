---
phase: 07-reconnection
plan: "02"
subsystem: reconnect
tags: [rust, tokio, pop3, reconnect, async, backon]

# Dependency graph
requires:
  - phase: 07-01
    provides: Outcome<T> enum, ReconnectingClient struct, ReconnectingClientBuilder, is_retryable, do_reconnect
  - phase: 06-01
    provides: unseen_uids, fetch_unseen, prune_seen methods on Pop3Client
  - phase: 05-02
    provides: retr_many, dele_many batch methods on Pop3Client
provides:
  - All 15 command methods on ReconnectingClient returning Result<Outcome<T>>
  - quit(self) returning Result<()> with best-effort disconnect on transient errors
  - Non-async read-only accessors: greeting, state, is_encrypted, is_closed, supports_pipelining
  - lib.rs re-exports for ReconnectingClient, ReconnectingClientBuilder, Outcome
  - Supplemental is_retryable unit tests covering all Pop3Error variants
  - Integration-ready mock client test infrastructure (new_for_test, build_authenticated_mock_client)
affects:
  - 08-connection-pooling (uses Pop3Client; ReconnectingClient not pooled but pattern similar)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Match-on-retryable pattern: Ok(v) => Fresh(v), Err(e) if is_retryable => do_reconnect + Reconnected(v), Err(e) => Err(e)"
    - "Unit-type Outcome fix: dele/rset/noop use Ok(()) => Fresh(()), then explicit Reconnected(()) — avoids clippy::let_unit_value"
    - "Cross-module test helper: pub(crate) build_authenticated_mock_client in client.rs allows reconnect tests to inject mock clients"

key-files:
  created: []
  modified:
    - src/reconnect.rs
    - src/client.rs

key-decisions:
  - "fetch_unseen wraps Vec<(UidlEntry, Message)> — matches actual Pop3Client return type, not Vec<Message> as plan template suggested"
  - "pub(crate) build_authenticated_mock_client added to client.rs — cleanest way to share mock construction without exposing Pop3Client fields"
  - "Full reconnect round-trip tests deferred to integration tests — do_reconnect() calls Pop3ClientBuilder::connect() requiring real TCP, not unit-testable"

patterns-established:
  - "Outcome wrapper pattern: every fallible method on ReconnectingClient matches on is_retryable to distinguish reconnect vs. direct error paths"
  - "Best-effort quit: retryable errors silently swallowed on disconnect; only non-retryable errors (ServerError, AuthFailed, etc.) propagate from quit()"

requirements-completed:
  - RECON-01
  - RECON-02
  - RECON-03
  - RECON-04

# Metrics
duration: 4min
completed: 2026-03-02
---

# Phase 7 Plan 02: Reconnection Command Wrappers Summary

**All 15 delegating command methods on ReconnectingClient with Outcome<T> wrapping and best-effort quit, completing RECON-01 through RECON-04**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-02T02:38:35Z
- **Completed:** 2026-03-02T02:42:43Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Added all 15 command methods (stat, list, uidl, retr, dele, rset, noop, top, capa, retr_many, dele_many, unseen_uids, fetch_unseen, prune_seen) each returning `Result<Outcome<T>>`
- Fixed `quit(self)` to be best-effort: transient I/O errors silently ignored (connection already dead), non-transient errors propagated
- Added 5 non-async read-only accessors: greeting, state, is_encrypted, is_closed, supports_pipelining
- Added 7 new unit tests covering supplemental is_retryable variants and Fresh/non-retryable method paths
- Verified zero regressions: all 163 unit + 2 integration + 32 doc tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement all delegating command methods on ReconnectingClient** - `5103184` (feat)
2. **Task 2: Tests, lib.rs re-exports, fmt check** - `a398990` (test)

**Plan metadata:** (included in final docs commit)

## Files Created/Modified
- `src/reconnect.rs` - Added 15 command wrapper methods, fixed quit best-effort, 5 read-only accessors, new_for_test constructor, 7 new tests
- `src/client.rs` - Added pub(crate) build_authenticated_mock_client helper for cross-module test use

## Decisions Made
- `fetch_unseen` wraps `Vec<(UidlEntry, Message)>` — matches actual `Pop3Client::fetch_unseen` return type; plan template showed `Vec<Message>` which was incorrect
- `pub(crate) build_authenticated_mock_client` added to `client.rs` — the cleanest way to share mock construction across modules without exposing private `Pop3Client` struct fields
- Full reconnect round-trip tests (Io error → do_reconnect → Reconnected) documented as TODO for integration tests — `do_reconnect()` calls `Pop3ClientBuilder::connect()` which opens a real TCP socket, making it untestable at the unit level

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed clippy::let_unit_value in dele/rset/noop**
- **Found during:** Task 1 (Implement command methods)
- **Issue:** Using `let v = self.client.dele(id).await?;` then `Ok(Outcome::Reconnected(v))` where `v: ()` triggers clippy::let_unit_value error
- **Fix:** Changed to `self.client.dele(id).await?; Ok(Outcome::Reconnected(()))` — explicit unit literal
- **Files modified:** src/reconnect.rs
- **Verification:** `cargo clippy --features rustls-tls -- -D warnings` passes clean
- **Committed in:** 5103184 (Task 1 commit)

**2. [Rule 3 - Blocking] Added pub(crate) build_authenticated_mock_client to client.rs**
- **Found during:** Task 2 (Tests)
- **Issue:** `build_authenticated_test_client` is private to `client` module; `reconnect.rs` tests cannot access it; direct struct field construction fails because Pop3Client fields are private
- **Fix:** Added `pub(crate)` test helper in `client.rs` that `reconnect.rs` can call via `crate::client::build_authenticated_mock_client(mock)`
- **Files modified:** src/client.rs
- **Verification:** All reconnect tests compile and pass
- **Committed in:** a398990 (Task 2 commit)

**3. [Rule 1 - Bug] Auto-fmt formatting fixes**
- **Found during:** Task 2 (fmt check)
- **Issue:** Method signatures for unseen_uids/prune_seen and assert! calls in tests needed reformatting per rustfmt style
- **Fix:** Ran `cargo fmt` to auto-apply rustfmt style
- **Files modified:** src/reconnect.rs
- **Verification:** `cargo fmt --check` passes clean
- **Committed in:** a398990 (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (1 bug/clippy fix, 1 blocking import fix, 1 formatting)
**Impact on plan:** All auto-fixes necessary for correctness and zero-warning compliance. No scope creep.

## Issues Encountered
- `build_authenticated_test_client` in `client.rs` was private (plan said it was pub(crate) — incorrect). Resolved by adding a new `pub(crate)` helper with the same behavior.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 7 (Reconnection) complete: RECON-01 through RECON-04 all satisfied
- `ReconnectingClient` is fully usable: all command methods, best-effort quit, accessors
- Ready for Phase 8 (Connection Pooling) — that phase uses `Pop3Client` directly via bb8 ManageConnection trait; `ReconnectingClient` is a separate consumer-level abstraction

---
*Phase: 07-reconnection*
*Completed: 2026-03-02*
