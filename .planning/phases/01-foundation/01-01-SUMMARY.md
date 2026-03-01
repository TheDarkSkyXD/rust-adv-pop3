---
phase: 01-foundation
plan: 01
subsystem: testing
tags: [rust, pop3, mock-io, test-infrastructure, error-handling]

# Dependency graph
requires: []
provides:
  - Pop3Error::AuthFailed(String) variant distinct from ServerError
  - Stream::Mock variant in transport.rs with Cursor reader and Rc<RefCell<Vec<u8>>> writer
  - Transport::mock() constructor for scripting server responses in tests
  - build_test_client and build_authenticated_test_client helpers in client.rs
  - 14 new mock I/O tests proving FIX-01 through FIX-04 and covering login/stat/list
affects: [02-foundation, future phases requiring mock I/O test patterns]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Mock I/O transport: Cursor<Vec<u8>> read side + Rc<RefCell<Vec<u8>>> write side for wire-level test assertions"
    - "AuthFailed vs ServerError: login() converts ServerError to AuthFailed for semantic clarity"
    - "build_authenticated_test_client: skips login handshake by directly setting authenticated=true"

key-files:
  created: []
  modified:
    - src/error.rs
    - src/transport.rs
    - src/client.rs

key-decisions:
  - "AuthFailed(String) variant added between ServerError and Parse for semantic auth failure reporting"
  - "login() maps ServerError -> AuthFailed on both USER and PASS rejections; NotAuthenticated remains client-side guard"
  - "Stream::Mock uses Rc<RefCell<Vec<u8>>> not Arc<Mutex> — tests are single-threaded, no overhead needed"
  - "Stream::Mock is #[cfg(test)] only — not visible in public API, no type parameter added to Pop3Client"

patterns-established:
  - "Wire-level assertion pattern: build_authenticated_test_client(server_bytes) -> assert_eq!(&*writer.borrow(), expected_bytes)"
  - "Auth timing pattern: build_test_client for login tests, assert client.authenticated after success or failure"
  - "FIX test naming: suffix _fixNN for traceability (e.g., rset_sends_correct_command_fix01)"

requirements-completed: [FOUND-04, FIX-01, FIX-02, FIX-03, FIX-04, QUAL-01]

# Metrics
duration: 25min
completed: 2026-03-01
---

# Phase 1 Plan 01: Foundation Summary

**Pop3Error::AuthFailed variant, Stream::Mock transport harness with Rc<RefCell> write capture, and 14 wire-level tests proving all four v1 bugs fixed**

## Performance

- **Duration:** ~25 min
- **Started:** 2026-03-01T18:43:00Z
- **Completed:** 2026-03-01T19:08:03Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments

- Added `Pop3Error::AuthFailed(String)` variant; `login()` now returns `AuthFailed` (not generic `ServerError`) when USER or PASS is rejected
- Built `Stream::Mock` test infrastructure in `transport.rs` with in-memory Cursor reader and Rc<RefCell<Vec<u8>>> writer for inspecting bytes sent on the wire
- Wrote 14 new mock I/O tests covering FIX-01 (rset wire bytes), FIX-02 (noop wire bytes), FIX-03 (auth flag timing), FIX-04 (list round-trip), plus login/stat/list happy and error paths

## Task Commits

Each task was committed atomically:

1. **Task 1: Add AuthFailed error variant and build mock transport infrastructure** - `fc19de5` (feat)
2. **Task 2: Write bug-proof tests and initial command mock I/O tests** - `db123e0` (test)

## Files Created/Modified

- `src/error.rs` - Added `AuthFailed(String)` variant to `Pop3Error`
- `src/transport.rs` - Added `Stream::Mock` variant, `Transport::mock()` constructor
- `src/client.rs` - Updated `login()` to return `AuthFailed`, added `build_test_client`, `build_authenticated_test_client`, and 14 new tests

## Decisions Made

- `AuthFailed(String)` is the caller-facing error from `login()` when credentials are rejected; `ServerError` remains for other commands
- `NotAuthenticated` is the client-side guard error (calling commands while not logged in) — distinct from `AuthFailed`
- `Rc<RefCell<Vec<u8>>>` chosen over `Arc<Mutex<...>>` for mock writer since all tests are single-threaded
- `Stream::Mock` confined entirely to `#[cfg(test)]` — no public API leakage, no type parameter on `Pop3Client`

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- `cargo fmt` reformatted some long lines in `src/client.rs` and pre-existing lines in `src/response.rs` — applied formatting before final commit to keep `cargo fmt --check` clean.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Mock transport infrastructure is ready for Plan 02 to add tests for retr, dele, uidl, quit, capa, and top
- All 4 bug fixes (FIX-01..04) have named proof tests with wire-level or behavioral assertions
- 37 total tests pass (23 original + 14 new), zero clippy warnings, formatting clean

## Self-Check: PASSED

- src/error.rs: FOUND
- src/transport.rs: FOUND
- src/client.rs: FOUND
- .planning/phases/01-foundation/01-01-SUMMARY.md: FOUND
- commit fc19de5 (Task 1): FOUND
- commit db123e0 (Task 2): FOUND

---
*Phase: 01-foundation*
*Completed: 2026-03-01*
