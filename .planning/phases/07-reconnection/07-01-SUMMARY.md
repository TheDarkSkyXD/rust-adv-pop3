---
phase: 07-reconnection
plan: "01"
subsystem: reconnection
tags: [backon, exponential-backoff, retry, pop3, async, tokio]

requires:
  - phase: 04-protocol-extensions
    provides: Pop3ClientBuilder with .clone() support needed for reconnect loop
  - phase: 05-pipelining
    provides: ConnectionClosed error variant used by is_retryable()

provides:
  - Outcome<T> enum (Fresh/Reconnected variants) with into_inner() and is_reconnected()
  - ReconnectingClientBuilder with configurable retry parameters
  - ReconnectingClient wrapping Pop3Client with stored builder and credentials
  - connect_and_auth() and is_retryable() internal helpers
  - ReconnectingClientBuilder::connect() with backon ExponentialBuilder retry loop
  - ReconnectingClient::do_reconnect() for Plan 02 command wrappers
  - backon = "1.6" dependency added unconditionally

affects:
  - 07-02-PLAN (consumes reconnect.rs to add command method wrappers and lib.rs re-exports)

tech-stack:
  added:
    - backon 1.6 (exponential backoff retry crate)
  patterns:
    - ReconnectCallback type alias to satisfy clippy::type_complexity for FnMut callbacks
    - "#[allow(dead_code)] on Plan 01 stubs consumed in Plan 02"
    - Borrow extraction pattern before async closure to avoid lifetime conflicts with backon
    - if/else on on_reconnect presence to avoid double-borrow with notify() + FnMut callback

key-files:
  created:
    - src/reconnect.rs
  modified:
    - Cargo.toml
    - src/lib.rs

key-decisions:
  - "ReconnectCallback type alias for Option<Box<dyn FnMut(u32, &Pop3Error) + Send>> — avoids clippy::type_complexity on both builder and client structs"
  - "Credentials passed to .connect() not stored on builder — reduces time credentials exist in plain-text builder fields"
  - "is_retryable covers Io, ConnectionClosed, Timeout, SysTemp — AuthFailed explicitly excluded to prevent account lockout on brute-force-protected servers"
  - "#[allow(dead_code)] on ReconnectingClient fields and do_reconnect — Plan 02 command wrappers will use them; suppression is narrowly scoped"

patterns-established:
  - "Outcome<T> return type pattern: every fallible ReconnectingClient method returns Result<Outcome<T>> so callers must handle reconnect signal"
  - "borrow extraction before async closure: extract &builder, user, pass as locals before || async move {} to avoid borrow conflicts"
  - "notify() bridging: local mut attempt counter adapts backon (err, dur) callback to user (attempt, &Pop3Error) signature"

requirements-completed:
  - RECON-01
  - RECON-02
  - RECON-04

duration: 3min
completed: 2026-03-02
---

# Phase 07 Plan 01: Reconnection Foundation Summary

**Outcome<T> enum, ReconnectingClientBuilder, and ReconnectingClient with backon ExponentialBuilder retry infrastructure — foundation types for transparent POP3 reconnection**

## Performance

- **Duration:** ~3 min
- **Started:** 2026-03-02T02:32:23Z
- **Completed:** 2026-03-02T02:35:05Z
- **Tasks:** 2 (Task 1: skeleton + Task 2: TDD unit tests, co-implemented)
- **Files modified:** 3 (src/reconnect.rs created, Cargo.toml, src/lib.rs)

## Accomplishments

- Added `backon = "1.6"` to Cargo.toml unconditionally (no feature flag)
- Created `src/reconnect.rs` with full module-level doc, Outcome<T> enum, ReconnectingClientBuilder, ReconnectingClient, connect_and_auth(), is_retryable(), and do_reconnect()
- Implemented backon ExponentialBuilder retry loop in both ReconnectingClientBuilder::connect() and ReconnectingClient::do_reconnect() with on_reconnect callback bridging
- Re-exported Outcome, ReconnectingClient, ReconnectingClientBuilder from lib.rs
- All 15 unit tests pass (10 is_retryable, 4 Outcome<T>, 1 builder defaults)
- cargo build, cargo clippy -D warnings, cargo fmt --check, cargo test — all clean

## Task Commits

Each task was committed atomically:

1. **Task 1 + Task 2: skeleton and unit tests** - `2054c8b` (feat)

_Note: TDD tests were implemented inline during skeleton creation; committed together per file._

## Files Created/Modified

- `src/reconnect.rs` — Full reconnect module: Outcome<T>, ReconnectingClientBuilder, ReconnectingClient, connect_and_auth, is_retryable, do_reconnect, 15 unit tests
- `Cargo.toml` — Added `backon = "1.6"` after `md5` in [dependencies]
- `src/lib.rs` — Added `pub mod reconnect` and re-exported Outcome, ReconnectingClient, ReconnectingClientBuilder

## Decisions Made

- ReconnectCallback type alias introduced to silence clippy::type_complexity on `Option<Box<dyn FnMut(u32, &Pop3Error) + Send>>` — this type appears on both structs
- Credentials passed to `.connect()` rather than stored on the builder — plan action note explicitly preferred this to avoid stale plaintext on the builder
- `#[allow(dead_code)]` narrowly applied to ReconnectingClient struct (all fields) and do_reconnect method — Plan 02 will flesh these out; suppression avoids forcing artificial usage
- `pub mod reconnect` used instead of private `mod reconnect` — module is part of public API (Outcome, ReconnectingClient, ReconnectingClientBuilder are all pub)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added ReconnectCallback type alias**
- **Found during:** Task 1 (clippy verification)
- **Issue:** `Option<Box<dyn FnMut(u32, &Pop3Error) + Send>>` used in two struct fields triggered `clippy::type_complexity` as an error under `-D warnings`
- **Fix:** Introduced `type ReconnectCallback = Option<Box<dyn FnMut(u32, &Pop3Error) + Send>>;` module-level type alias; replaced both occurrences
- **Files modified:** src/reconnect.rs
- **Verification:** `cargo clippy --features rustls-tls -- -D warnings` passes cleanly
- **Committed in:** 2054c8b (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (Rule 2 — missing critical fix for clippy compliance)
**Impact on plan:** Minimal scope. Type alias is a cosmetic correctness fix. No behavior change.

## Issues Encountered

- cargo fmt reformatted two long `build_exp_backoff(...)` call sites and the `matches!` arms in `is_retryable` — applied automatically with `cargo fmt`

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- `src/reconnect.rs` is ready for Plan 02 to add command method wrappers (stat, list, uidl, retr, dele, rset, noop, top, capa)
- `ReconnectingClient::do_reconnect()` is implemented and awaits use from Plan 02 wrappers
- `Outcome<T>` type is public and re-exported — Plan 02 command methods can return `Result<Outcome<T>>`
- All Plan 01 must-haves are satisfied

---
*Phase: 07-reconnection*
*Completed: 2026-03-02*
