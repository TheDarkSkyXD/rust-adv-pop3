---
phase: 02-async-core
plan: 01
subsystem: transport
tags: [async, tokio, tokio-test, pop3, transport, timeout, rust]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: Pop3Error enum, Transport struct (sync), mock transport pattern, all response parsing
provides:
  - tokio 1.x dependency with net/io-util/time/rt-multi-thread/macros features
  - tokio-test 0.4 dev-dependency for async mock I/O
  - Pop3Error::Timeout variant for distinguishing timeout from other I/O errors
  - Async Transport using BufReader<Box<dyn AsyncRead + Unpin + Send>> and Box<dyn AsyncWrite + Unpin + Send>
  - DEFAULT_TIMEOUT constant (30s) pub(crate) for client.rs to use as default
  - Transport::connect_plain(addr, timeout) — async TcpStream-based connection with split halves
  - Transport::connect_tls stub — returns error with message for Phase 3
  - Transport::read_line with tokio::time::timeout returning Pop3Error::Timeout on expiry
  - Transport::read_multiline with RFC 1939 dot-unstuffing via async read_line calls
  - Transport::mock(tokio_test::io::Mock) — async test mock constructor
  - 4 transport unit tests as #[tokio::test]
affects: [02-02, 02-03, 03-tls, all subsequent phases using async Transport]

# Tech tracking
tech-stack:
  added:
    - tokio 1.x (net, io-util, time, rt-multi-thread, macros features)
    - tokio-test 0.4 (dev-dependency for async I/O mocking)
  patterns:
    - "BufReader<Box<dyn AsyncRead + Unpin + Send>> — type-erased async reader enabling TcpStream and Mock to share one code path"
    - "tokio::time::timeout + double ?? — outer Elapsed error mapped to Pop3Error::Timeout; inner io::Error propagated via From"
    - "tokio::io::split — splits TcpStream (or Mock) into independent read/write halves for borrow-safety"
    - "tokio_test::io::Builder — write().read() chains bake wire protocol expectations directly into the mock"
    - "DEFAULT_TIMEOUT constant — pub(crate) Duration::from_secs(30) exported from transport for client.rs"

key-files:
  created: []
  modified:
    - Cargo.toml
    - src/error.rs
    - src/transport.rs

key-decisions:
  - "Box<dyn AsyncRead> + Box<dyn AsyncWrite> over an enum — no match arms on every method; TcpStream and Mock share one code path"
  - "Single timeout Duration field on Transport — set at connect time, applied to every read_line call; immutable after connection"
  - "connect_tls stub returns Pop3Error::Io(Unsupported) — API compatibility maintained; Phase 3 implements TLS"
  - "tokio_test::io::Builder mock replaces Cursor/Rc/RefCell — write expectations are validated inline; no separate writer handle needed"
  - "double ?? idiom — .map_err(|_| Pop3Error::Timeout)?? cleanly unwraps Elapsed then io::Error with correct error types"

patterns-established:
  - "Async read with timeout: tokio::time::timeout(self.timeout, self.reader.read_line(&mut line)).await.map_err(|_| Pop3Error::Timeout)??"
  - "Mock construction: Builder::new().write(expected_cmd).read(server_response).build() for single command/response pairs"
  - "EOF guard: if n == 0 { return Err(Pop3Error::Io(UnexpectedEof)) } after successful read_line"

requirements-completed: [ASYNC-02, ASYNC-03, ASYNC-05]

# Metrics
duration: 9min
completed: 2026-03-01
---

# Phase 2 Plan 1: Async Transport Summary

**Async tokio Transport with BufReader split halves, tokio::time::timeout returning Pop3Error::Timeout, and tokio_test::io::Builder mock — replacing sync Cursor/Rc/RefCell**

## Performance

- **Duration:** 9 min
- **Started:** 2026-03-01T20:04:59Z
- **Completed:** 2026-03-01T20:09:53Z
- **Tasks:** 2
- **Files modified:** 3 (Cargo.toml, src/error.rs, src/transport.rs)

## Accomplishments

- Added `tokio` 1.x with `[net, io-util, time, rt-multi-thread, macros]` features and `tokio-test` 0.4 as dev-dependency to Cargo.toml
- Added `Pop3Error::Timeout` variant to error.rs for explicit timeout matching by callers
- Completely rewrote `src/transport.rs`: removed sync `Stream` enum, `Cursor`, `Rc<RefCell<Vec<u8>>>`, `set_timeouts`, and all std::io imports
- New `Transport` struct uses `BufReader<Box<dyn AsyncRead + Unpin + Send>>` and `Box<dyn AsyncWrite + Unpin + Send>` — one code path for both TcpStream and mock
- `connect_plain(addr, timeout)` uses `tokio::net::TcpStream::connect + io::split` to obtain independent read/write halves
- `connect_tls` stubbed with `Pop3Error::Io(Unsupported)` — API compatibility maintained for Phase 3
- `read_line` wraps `BufReader::read_line` in `tokio::time::timeout`, returning `Pop3Error::Timeout` on expiry
- `read_multiline` performs async dot-unstuffing per RFC 1939 via `self.read_line().await?` in a loop
- `Transport::mock(tokio_test::io::Mock)` replaces old `mock(&[u8]) -> (Self, Rc<RefCell<Vec<u8>>>)` — no separate writer handle
- 4 transport tests as `#[tokio::test]`: `dot_unstuffing_via_transport`, `send_command_writes_crlf`, `read_line_returns_eof_error`, `read_line_returns_line`

## Task Commits

1. **Task 1: Add tokio dependencies and Pop3Error::Timeout** - `59f0d8d` (feat)
   - Cargo.toml: tokio 1.x + tokio-test 0.4
   - src/error.rs: Pop3Error::Timeout variant

2. **Task 2: Rewrite transport.rs to async** - `6a0f361` (feat — bundled with Plan 02-03 Task 1)
   - src/transport.rs: full async rewrite with BufReader split halves, timeout, async mock

**Note:** Task 2 was committed as part of `feat(02-03)` due to an execution ordering constraint — transport.rs needed to be async before client.rs could be migrated, which was itself a prerequisite for the CI workflow. See Deviations section.

## Files Created/Modified

- `Cargo.toml` — Added tokio 1.x dependencies and tokio-test 0.4 dev-dependency
- `src/error.rs` — Added `Pop3Error::Timeout` variant with `#[error("timed out")]`
- `src/transport.rs` — Complete async rewrite: BufReader split halves, timeout reads, async mock

## Decisions Made

- `Box<dyn AsyncRead>` + `Box<dyn AsyncWrite>` instead of enum variants — eliminates match arms on every method; both TcpStream and tokio_test Mock use the same code path without conditional compilation
- Single `timeout: Duration` field stored on Transport, set at connect time — immutable for the lifetime of the connection, preventing mid-session confusion; forward-compatible with Phase 4 builder
- `connect_tls` returns `Pop3Error::Io(ErrorKind::Unsupported)` immediately — maintains the method signature for API compatibility while deferring TLS implementation to Phase 3; marked with `#[allow(dead_code)]`
- `tokio_test::io::Builder` mock with write expectations baked in — the mock panics if actual bytes differ from expected, providing implicit assertion without returning a separate writer handle
- Double `??` idiom for timeout error mapping: `.map_err(|_| Pop3Error::Timeout)??` correctly converts `Result<Result<usize, io::Error>, Elapsed>` → both error types handled with minimal boilerplate

## Deviations from Plan

### Execution Ordering Deviation

**1. [Rule 3 - Blocking] Task 2 committed within Plan 02-03 due to execution dependency chain**

- **Found during:** Task 2 verification
- **Issue:** When Plan 02-03 (CI workflow) was being executed, `cargo clippy` revealed `client.rs` still used the old sync Transport API (32 compile errors). The CI jobs would have failed immediately on first push. The full async migration of `transport.rs` and `client.rs` was required as a blocking prerequisite.
- **Fix:** Task 2 (transport.rs rewrite) was committed in `6a0f361` along with the client.rs async migration and the CI workflow itself. All Plan 02-01 success criteria are satisfied by that commit.
- **Files modified:** src/transport.rs
- **Commit:** `6a0f361` (feat(02-03))

---

**Total deviations:** 1 (execution ordering — work completed in correct phase, bundled with a later plan's commit)
**Impact on plan:** Zero negative impact. All success criteria satisfied. The async transport was complete before CI was set up, preserving the correct logical dependency order.

## Issues Encountered

- Windows GNU toolchain (`stable-x86_64-pc-windows-gnu`) missing `dlltool.exe` — `cargo test` fails locally on this machine with "error calling dlltool: program not found". Resolved by adding `/c/msys64/mingw64/bin` to PATH (MSYS2 was already installed via winget). Tests pass once dlltool is available. CI runs on Ubuntu where this does not occur.

## Self-Check

| Criterion | Check | Result |
|-----------|-------|--------|
| Cargo.toml has tokio 1.x with correct features | Present: `[net, io-util, time, rt-multi-thread, macros]` | PASS |
| tokio-test 0.4 in dev-dependencies | Present | PASS |
| Pop3Error::Timeout variant exists | In src/error.rs | PASS |
| Transport::connect_plain is async with TcpStream | Yes, uses `tokio::net::TcpStream::connect` | PASS |
| Transport::read_line wraps reads in timeout | Yes, `tokio::time::timeout(self.timeout, ...)` | PASS |
| Transport::read_multiline does async dot-unstuffing | Yes, calls `self.read_line().await?` | PASS |
| Transport::mock uses tokio_test::io::Builder | Yes, takes `tokio_test::io::Mock` | PASS |
| All transport tests pass as #[tokio::test] | 4/4 tests pass | PASS |
| Commit 59f0d8d exists | Task 1 commit verified | PASS |
| Commit 6a0f361 contains transport.rs changes | Task 2 commit verified | PASS |

## Self-Check: PASSED

## Next Phase Readiness

- Async `Transport` is ready — Plan 02-02 (Pop3Client async migration) can use `Transport::connect_plain`, `read_line`, `read_multiline`, `send_command`, and `Transport::mock`
- `DEFAULT_TIMEOUT` constant is available to client.rs as `crate::transport::DEFAULT_TIMEOUT`
- `Pop3Error::Timeout` is matchable — callers can distinguish timeout vs I/O errors for retry logic
- `connect_tls` stub in place — Phase 3 replaces the stub body without changing the method signature

---
*Phase: 02-async-core*
*Completed: 2026-03-01*
