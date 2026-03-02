---
phase: 05-pipelining
plan: "02"
subsystem: protocol
tags: [pipelining, tcp, batching, capa, pop3]

# Dependency graph
requires:
  - phase: 05-01
    provides: BufWriter on Transport writer, pub(crate) reader/writer access, ConnectionClosed variant, is_closed()/set_closed()

provides:
  - CAPA-based pipelining auto-detection via login() and apop()
  - supports_pipelining() public accessor
  - retr_many(&[u32]) batch retrieval with windowed pipelined path
  - dele_many(&[u32]) batch deletion with windowed pipelined path
  - Sequential fallback when server lacks PIPELINING capability

affects: [06-uidl-caching, 07-reconnection, 08-connection-pooling]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Windowed pipeline: send PIPELINE_WINDOW=4 commands, flush, drain responses before next window
    - Per-item result independence: Vec<Result<T>> not Result<Vec<T>> for batch methods
    - CAPA probe in login/apop: automatic, errors silently ignored (not all servers support CAPA)
    - Upfront ID validation: reject entire batch immediately if any ID is 0

key-files:
  created: []
  modified:
    - src/client.rs
    - tests/integration.rs

key-decisions:
  - "CAPA probe runs after every successful login() and apop() call -- caller never configures this (PIPE-02)"
  - "PIPELINE_WINDOW = 4 -- conservative window preventing TCP send-buffer deadlock with large RETR responses"
  - "Per-item errors (server -ERR) do not abort batch; I/O errors preserve received results and fill rest with ConnectionClosed"
  - "retr_many_pipelined and dele_many_pipelined are private methods -- only retr_many/dele_many are public API"
  - "read_retr_response() private helper avoids duplication between pipelined and sequential RETR parsing"
  - "Integration tests updated to include CAPA probe after login -- mock server now expects CAPA in conversation"

patterns-established:
  - "Batch methods: validate all inputs upfront, return empty vec for empty slice, branch on is_pipelining flag"
  - "Pipelined send: write_all each command then flush once per window (single system call)"
  - "I/O error mid-pipeline: early return Ok(results) with remaining slots filled by ConnectionClosed"

requirements-completed: [PIPE-01, PIPE-02, PIPE-03, PIPE-04, PIPE-05]

# Metrics
duration: 5min
completed: "2026-03-02"
---

# Phase 5 Plan 02: CAPA Pipelining Detection + Batch Methods Summary

**CAPA-driven pipelining auto-detection in login/apop plus retr_many/dele_many batch methods with windowed TCP-safe pipelining and sequential fallback**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-02T01:13:47Z
- **Completed:** 2026-03-02T01:18:55Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- Added `is_pipelining: bool` field to `Pop3Client` and CAPA probe in `login()` and `apop()` after successful auth
- Added `pub fn supports_pipelining(&self) -> bool` accessor for caller diagnostics
- Implemented `retr_many(&[u32])` and `dele_many(&[u32])` with windowed pipelined path (PIPELINE_WINDOW=4) and transparent sequential fallback
- All 124 unit tests + 2 integration tests + 27 doc tests passing; clippy and fmt clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Pipelining flag, CAPA probe, supports_pipelining()** - `989abbb` (feat)
2. **Task 2: retr_many and dele_many with pipelined/sequential paths** - `0f81672` (feat)

## Files Created/Modified

- `src/client.rs` - Pop3Client struct extended with is_pipelining field; CAPA probe in login/apop; supports_pipelining() accessor; retr_many/dele_many public methods; retr_many_pipelined/dele_many_pipelined/read_retr_response private helpers; PIPELINE_WINDOW constant; all test helpers updated; new tests for detection and batch methods; existing login/apop success tests updated with CAPA mock expectations
- `tests/integration.rs` - Both integration tests updated to include CAPA exchange after successful login

## Decisions Made

- PIPELINE_WINDOW = 4: conservative value safe for large RETR responses, matches Phase 5 research guidance
- Per-item results `Vec<Result<T>>`: allows callers to handle individual message failures without losing other results; I/O errors fill remaining with ConnectionClosed
- Upfront validation: all IDs validated before any I/O, so zero ID rejects the whole batch immediately
- CAPA errors silently suppressed: `unwrap_or_default()` ensures servers that don't support CAPA still work

## Deviations from Plan

None - plan executed exactly as written.

The integration tests required updating (matching plan Step 8 which noted "every existing test that calls login() now needs to account for the CAPA probe"), and this was covered in the plan guidance for Task 1. The `build_authenticated_test_client_with_pipelining()` helper unused-warning was not generated (tests compile cleanly).

## Issues Encountered

None.

## Next Phase Readiness

- Phase 5 (Pipelining) is now complete: all 5 PIPE requirements implemented
- Phase 6 (UIDL Caching) can proceed independently -- no Phase 5 dependencies
- Phase 7 (Reconnection) uses `ConnectionClosed` error variant from Phase 5 -- ready
- Phase 8 (Connection Pooling) uses `is_closed()` and `Pop3ClientBuilder::Clone` from Phases 4-5 -- ready

---
*Phase: 05-pipelining*
*Completed: 2026-03-02*
