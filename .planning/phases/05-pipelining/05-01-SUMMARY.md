---
phase: 05-pipelining
plan: "01"
subsystem: transport
tags: [tokio, bufwriter, pipelining, error-handling, connection-state]

# Dependency graph
requires:
  - phase: 04-protocol-extensions
    provides: Pop3ClientBuilder with Clone (required by Phase 8 connection pooling)
provides:
  - BufWriter<WriteHalf> on Transport writer for batched pipelined command flushing
  - pub(crate) reader/writer/timeout fields on Transport for direct batch method access
  - Pop3Error::ConnectionClosed variant replacing Io(UnexpectedEof) on EOF
  - Transport::is_closed() / set_closed() for EOF and quit() state tracking
  - Pop3Client::is_closed() public accessor delegating to transport
affects: [05-02-pipelining-batch-methods, 07-reconnection, 08-connection-pooling]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - BufWriter wrapping WriteHalf for efficient batched TCP writes
    - is_closed flag tracks known-closed state (not a live TCP probe)
    - ConnectionClosed variant for semantic EOF errors vs generic I/O errors

key-files:
  created: []
  modified:
    - src/transport.rs
    - src/error.rs
    - src/client.rs

key-decisions:
  - "BufWriter default buffer size (8 KB) is used -- commands are ~15 bytes each, no bottleneck"
  - "ConnectionClosed wording 'connection closed' matches existing EOF message in legacy code"
  - "is_closed is a private bool on Transport, set on EOF in read_line() and by quit() via set_closed()"
  - "InnerStream promoted to pub(crate) to satisfy private_interfaces lint (reader/writer are pub(crate))"
  - "Test for is_closed_true_after_eof requires write expectation in mock (stat() sends STAT before reading)"

patterns-established:
  - "Transport pub(crate) fields: reader, writer, timeout -- direct access for batch pipelining methods"
  - "is_closed / set_closed separation: read_line sets automatically, quit() sets explicitly via set_closed()"

requirements-completed: [PIPE-05]

# Metrics
duration: 4min
completed: 2026-03-02
---

# Phase 5 Plan 01: Transport Infrastructure for Pipelining Summary

**BufWriter<WriteHalf> on Transport with pub(crate) reader/writer, ConnectionClosed variant, and is_closed() accessor -- all infrastructure required for RFC 2449 pipelining batch methods**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-02T01:06:32Z
- **Completed:** 2026-03-02T01:10:15Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments

- Wrapped Transport writer in `BufWriter<io::WriteHalf<InnerStream>>` -- multiple `write_all` calls now accumulate in the 8 KB buffer before a single `flush()` sends them as one TCP write, enabling efficient pipelining
- Made `reader`, `writer`, and `timeout` fields `pub(crate)` so batch methods in `client.rs` can access them directly without going through `send_command()`
- Added `Pop3Error::ConnectionClosed` variant replacing `Io(UnexpectedEof)` on EOF -- callers can now match on a specific variant for reconnection logic
- Added `is_closed()` public accessor on `Pop3Client` and `set_closed()` on `Transport`; `quit()` marks transport closed after successful QUIT response
- 11 tests total: 3 new transport tests, 2 new client tests, 1 updated existing test

## Task Commits

1. **Task 1: Upgrade Transport writer to BufWriter and add is_closed field** - `5586ded` (feat)
2. **Task 2: Add is_closed() public accessor on Pop3Client and update quit()** - `def1c04` (feat)

**Plan metadata:** (this commit, docs)

## Files Created/Modified

- `src/transport.rs` - BufWriter upgrade, pub(crate) fields, InnerStream pub(crate), ConnectionClosed on EOF, is_closed/set_closed, upgrade_in_place into_inner() fix, 3 new tests, 1 updated test
- `src/error.rs` - Added Pop3Error::ConnectionClosed variant with doc comment
- `src/client.rs` - Added is_closed() public method, updated quit() to call set_closed(), 2 new tests

## Decisions Made

- `InnerStream` promoted to `pub(crate)` to satisfy the `private_interfaces` lint -- reader and writer are `pub(crate)` and their type includes `InnerStream`, so `InnerStream` itself must be at least `pub(crate)`. This is an internal enum with no public API surface.
- `is_closed_true_after_eof` test in `client.rs` uses `write(b"STAT\r\n")` in the mock builder -- `stat()` calls `send_command` before reading, so the mock must expect the write or it panics with "unexpected write". This is a test infrastructure refinement, not a behavioral change.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Made InnerStream pub(crate) to fix private_interfaces lint**
- **Found during:** Task 1 (BufWriter upgrade and pub(crate) fields)
- **Issue:** Promoting `reader` and `writer` to `pub(crate)` exposed `InnerStream` (private enum) via those fields. Compiler emits `private_interfaces` warning, which becomes an error under `-D warnings`.
- **Fix:** Changed `enum InnerStream` to `pub(crate) enum InnerStream`. No external API change -- the crate boundary hides it from callers.
- **Files modified:** `src/transport.rs`
- **Verification:** `cargo clippy -- -D warnings` passes with zero warnings
- **Committed in:** `5586ded` (Task 1 commit)

**2. [Rule 1 - Bug] Fixed is_closed_true_after_eof test mock to include STAT write expectation**
- **Found during:** Task 2 (client tests)
- **Issue:** The plan's test code used `Builder::new().build()` (empty mock), but `stat()` calls `send_command("STAT")` before reading, causing the mock to panic with "unexpected write (0 actions remain)".
- **Fix:** Changed mock to `Builder::new().write(b"STAT\r\n").build()` so the write is expected; EOF is returned on the subsequent read, which sets `is_closed = true`.
- **Files modified:** `src/client.rs`
- **Verification:** `cargo test -- is_closed` passes all 4 is_closed tests
- **Committed in:** `def1c04` (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 missing critical lint fix, 1 bug in plan's test spec)
**Impact on plan:** Both fixes necessary for correctness -- one for clippy compliance, one for correct test infrastructure. No scope creep.

## Issues Encountered

None beyond the two auto-fixed deviations above.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Transport infrastructure complete: BufWriter, pub(crate) fields, ConnectionClosed, is_closed -- all four requirements for Phase 5 pipelining satisfied
- Plan 05-02 can now implement `retr_many` and `dele_many` batch methods that directly access `transport.reader` / `transport.writer`
- Phase 7 (reconnection) can match on `Pop3Error::ConnectionClosed` for reconnect triggers
- Phase 8 (bb8 pooling) can call `client.is_closed()` for health check in `ManageConnection::is_valid()`

---
*Phase: 05-pipelining*
*Completed: 2026-03-02*
