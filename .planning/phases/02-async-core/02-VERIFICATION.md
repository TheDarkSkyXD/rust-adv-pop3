---
phase: 02-async-core
verified: 2026-03-01T21:00:00Z
status: gaps_found
score: 9/12 must-haves verified
re_verification: false
gaps:
  - truth: "cargo test passes cleanly (all compilation units including examples)"
    status: failed
    reason: "examples/basic.rs still uses the old synchronous v1 API (TlsMode, sync connect, no .await) — cargo test fails to compile the example with 3 errors"
    artifacts:
      - path: "examples/basic.rs"
        issue: "Uses removed TlsMode import and sync Pop3Client::connect API — does not compile against the new async API"
    missing:
      - "Update examples/basic.rs to use the async API: add #[tokio::main], use .await on connect/login/stat/quit, remove TlsMode import, use plain TCP connect signature"
  - truth: "All v1.0.6 commands are confirmed correct by integration tests against a mock server"
    status: failed
    reason: "ROADMAP Success Criterion #2 requires integration tests against a mock server for all v1.0.6 commands (STAT, LIST, UIDL, RETR, DELE, NOOP, RSET, QUIT). Tests exist only as unit tests in client.rs using tokio_test mocks — there is no integration test suite (no tests/ directory). The ROADMAP criterion is the contractual definition."
    artifacts: []
    missing:
      - "Clarify whether ROADMAP Success Criterion #2 is satisfied by in-source tokio_test mock tests or requires a separate integration test suite (tests/ directory). If the criterion is considered met by the existing mock-based tests, document this decision. If a real integration test suite is required, create it."
---

# Phase 2: Async Core — Verification Report

**Phase Goal:** All public API methods are async and work over a plain TCP connection — developers can connect, authenticate, and run every v1.0.6 command against a real server with no blocking calls
**Verified:** 2026-03-01T21:00:00Z
**Status:** gaps_found
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

All must-haves are drawn from PLAN frontmatter and the ROADMAP Phase 2 Success Criteria.

| #  | Truth | Status | Evidence |
|----|-------|--------|----------|
| 1  | Transport reads use tokio::io::BufReader with async line-oriented reads | VERIFIED | `src/transport.rs` line 12: `reader: BufReader<Box<dyn io::AsyncRead + Unpin + Send>>`. `read_line` calls `self.reader.read_line()` via `AsyncBufReadExt`. |
| 2  | Multi-line responses correctly dot-unstuff per RFC 1939 via async read_multiline | VERIFIED | `src/transport.rs` lines 78-96: `read_multiline` loops calling `self.read_line().await?`, strips trailing CRLF, applies `strip_prefix("..")` dot-unstuffing. Test `dot_unstuffing_via_transport` confirms behavior. |
| 3  | Transport has a configurable read timeout that returns Pop3Error::Timeout on expiry | VERIFIED | `src/transport.rs` line 62-64: `tokio::time::timeout(self.timeout, self.reader.read_line(&mut line)).await.map_err(\|_\| Pop3Error::Timeout)??`. `Pop3Error::Timeout` variant confirmed in `src/error.rs` line 39. |
| 4  | Mock transport uses tokio_test::io::Builder for async test scripting | VERIFIED | `src/transport.rs` lines 105-112: `Transport::mock(mock: tokio_test::io::Mock)` constructor. Old `Cursor`/`Rc<RefCell<Vec<u8>>>` completely removed. |
| 5  | All public API methods on Pop3Client are async fn and can be awaited in a tokio runtime | VERIFIED | All 10 command methods verified as `pub async fn`: `connect`, `connect_default`, `login`, `stat`, `list`, `uidl`, `retr`, `dele`, `rset`, `noop`, `quit`, `top`, `capa`. `cargo test --lib` passes 57 tests including async ones. |
| 6  | SessionState enum tracks Connected, Authenticated, Disconnected — callers can read state via state() accessor | VERIFIED | `src/types.rs` lines 1-10: `SessionState` enum with three variants. `src/client.rs` lines 60-62: `pub fn state(&self) -> SessionState`. |
| 7  | login() returns error if already authenticated (SessionState is not Connected) | VERIFIED | `src/client.rs` lines 66-68: `if self.state != SessionState::Connected { return Err(Pop3Error::NotAuthenticated); }`. Test `login_rejects_when_already_authenticated` confirms this. |
| 8  | quit(self) consumes the client — compiler rejects any method call after quit | VERIFIED | `src/client.rs` line 174: `pub async fn quit(self) -> Result<()>` — takes ownership. Test `quit_consumes_client` has a commented compile-time proof. Move semantics enforced by Rust borrow checker. |
| 9  | All public types derive Debug including SessionState | VERIFIED | `src/types.rs` line 2: `#[derive(Debug, Clone, PartialEq, Eq)]` on `SessionState`. All other types (`Stat`, `ListEntry`, `UidlEntry`, `Message`, `Capability`) also have `Debug`. Test `session_state_derives_debug` confirms. |
| 10 | GitHub Actions CI runs cargo test, cargo clippy -D warnings, and cargo fmt --check on every push and pull_request | PARTIAL | `.github/workflows/ci.yml` exists and correctly defines three jobs with `dtolnay/rust-toolchain@stable`. However, the `cargo test` CI step will fail because `examples/basic.rs` does not compile against the async API. The CI workflow structure is correct; the failure is in the code under test. |
| 11 | cargo test passes cleanly (all compilation units including examples) | FAILED | `cargo test` produces 3 compile errors from `examples/basic.rs`: (1) `no TlsMode in the root` — TlsMode was removed in Phase 2; (2) `? operator cannot be applied to impl Future` — connect() is async, example has no .await; (3) type annotation error cascading from #2. `cargo test --lib` passes 57/57 tests cleanly. |
| 12 | All v1.0.6 commands confirmed correct by integration tests against a mock server | PARTIAL/UNCERTAIN | STAT, LIST, UIDL, RETR, DELE, NOOP, RSET, QUIT, TOP, CAPA are all covered by in-source `#[tokio::test]` unit tests using `tokio_test::io::Builder` mocks. However, ROADMAP Success Criterion #2 says "confirmed by integration tests against a mock server" — no `tests/` directory exists, no separate integration test suite. Whether the existing mock-based unit tests satisfy this criterion requires human judgment. |

**Score:** 9/12 truths verified (2 failed, 1 partial)

---

## Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `Cargo.toml` | tokio and tokio-test dependencies | VERIFIED | `tokio = { version = "1", features = ["net", "io-util", "time", "rt-multi-thread", "macros"] }`, `tokio-test = "0.4"` in dev-dependencies |
| `src/error.rs` | Pop3Error::Timeout variant | VERIFIED | Lines 38-40: `/// The operation timed out waiting for the server.` / `#[error("timed out")]` / `Timeout,` |
| `src/transport.rs` | Async Transport with BufReader, split halves, timeout, mock constructor | VERIFIED | 158 lines, fully async: `BufReader<Box<dyn AsyncRead>>`, `tokio::io::split`, `tokio::time::timeout`, `Transport::mock(tokio_test::io::Mock)` |
| `src/client.rs` | Async Pop3Client with SessionState, quit(self), all async methods | VERIFIED | 703 lines, all public methods `async fn`, uses `SessionState`, `quit(self)` consumes, 33+ `#[tokio::test]` tests |
| `src/types.rs` | SessionState enum with Debug, Clone, PartialEq, Eq derives | VERIFIED | Lines 1-10, all four derives present |
| `src/lib.rs` | Re-exports SessionState, updated async doctest | VERIFIED | Line 34: `pub use types::{Capability, ListEntry, Message, SessionState, Stat, UidlEntry};`. Doctest uses `#[tokio::main]` and `.await`. |
| `.github/workflows/ci.yml` | GitHub Actions CI workflow with test, clippy, fmt jobs | VERIFIED (structure) | File exists, valid YAML, 3 jobs. Passes fmt/clippy locally. `cargo test` job will fail due to `examples/basic.rs` gap. |
| `examples/basic.rs` | Updated async example (implied by goal) | FAILED | Still uses old v1 sync API: `TlsMode`, no `#[tokio::main]`, no `.await` — does not compile |

---

## Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/transport.rs` | `tokio::io::BufReader` | `Box<dyn AsyncRead + Unpin + Send>` | VERIFIED | Line 3: `use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};` Line 12: `reader: BufReader<Box<dyn io::AsyncRead + Unpin + Send>>` |
| `src/transport.rs` | `tokio::time::timeout` | `read_line` and `read_multiline` wrapping | VERIFIED | Line 62: `tokio::time::timeout(self.timeout, self.reader.read_line(&mut line)).await.map_err(\|_\| Pop3Error::Timeout)??` |
| `src/transport.rs` | `src/error.rs` | `Pop3Error::Timeout` on elapsed | VERIFIED | `Pop3Error::Timeout` in map_err at line 64 |
| `src/client.rs` | `src/transport.rs` | all Transport method calls use `.await` | VERIFIED | `send_and_check` at line 202: `self.transport.send_command(cmd).await?` and `self.transport.read_line().await?`. `read_multiline` called with `.await?` in list/uidl/retr/top/capa. |
| `src/client.rs` | `src/types.rs` | `SessionState` enum used for state field | VERIFIED | Line 12: `state: SessionState`. Line 6 import. |
| `src/lib.rs` | `src/types.rs` | `pub use types::SessionState` | VERIFIED | Line 34 of lib.rs |
| `src/client.rs` (quit) | Rust borrow checker | move semantics prevent use-after-quit | VERIFIED | `pub async fn quit(self)` — `self` consumed by value. Compiler rejects use-after-move. |
| `.github/workflows/ci.yml` | `cargo test` | test job step | WIRED (structure) | `run: cargo test` present. Will fail in CI due to broken example. |
| `.github/workflows/ci.yml` | `dtolnay/rust-toolchain@stable` | Rust toolchain setup action | VERIFIED | Present in all three jobs with correct pinning |

---

## Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| ASYNC-01 | 02-02 | All public API methods are `async fn` using tokio runtime | SATISFIED | All 10 command methods verified as `pub async fn` in `src/client.rs`. 57 tests pass as `#[tokio::test]`. |
| ASYNC-02 | 02-01 | Reads use `tokio::io::BufReader` with line-oriented buffering | SATISFIED | `transport.rs` line 12: `reader: BufReader<Box<dyn io::AsyncRead + Unpin + Send>>`. `AsyncBufReadExt::read_line` used in `read_line()`. |
| ASYNC-03 | 02-01 | Multi-line responses correctly handle RFC 1939 dot-unstuffing | SATISFIED | `read_multiline()` in `transport.rs` lines 78-96. `strip_prefix("..")` removes leading dot. Test `dot_unstuffing_via_transport` and `retr_dot_unstuffing` confirm. |
| ASYNC-04 | 02-02 | Session state tracked via `SessionState` enum (not a public bool field) | SATISFIED | `SessionState` enum in `types.rs`, `state: SessionState` field in `Pop3Client`, `state()` accessor, `login()` guard, exported from `lib.rs`. |
| ASYNC-05 | 02-01 | Connection supports configurable read/write timeouts | SATISFIED | `connect_plain(addr, timeout: Duration)`, `DEFAULT_TIMEOUT = Duration::from_secs(30)`, `tokio::time::timeout` wraps every `read_line()` call. |
| API-03 | 02-02 | All public types derive `Debug` | SATISFIED | All 6 types in `types.rs` have `#[derive(Debug, ...)]`. `Pop3Error` in `error.rs` line 4 has `#[derive(Debug, ...)]`. `Pop3Client` does not derive Debug (struct is not public for debug purposes — not required by the requirement). |
| API-04 | 02-02 | `Client` consumes `self` on `quit()` preventing use-after-disconnect | SATISFIED | `pub async fn quit(self) -> Result<()>` at `client.rs` line 174. Compile-time guarantee via Rust move semantics. |
| QUAL-03 | 02-03 | GitHub Actions CI runs tests, clippy, and format checks | PARTIAL | `.github/workflows/ci.yml` exists with all three jobs. Structure is correct. However, the `cargo test` step will fail in CI due to `examples/basic.rs` referencing the removed `TlsMode` type. CI cannot currently pass in this state. |

**Orphaned requirements check:** REQUIREMENTS.md maps exactly ASYNC-01, ASYNC-02, ASYNC-03, ASYNC-04, ASYNC-05, API-03, API-04, QUAL-03 to Phase 2. All 8 are claimed by the 3 PLANs. No orphaned requirements.

---

## Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `examples/basic.rs` | 1 | `use pop3::{Pop3Client, TlsMode}` — imports removed type | BLOCKER | `cargo test` fails to compile. CI `cargo test` job will fail on first push. Example ships with the crate and is part of the published API surface. |
| `examples/basic.rs` | 3 | `fn main()` — synchronous, no `#[tokio::main]` | BLOCKER | Cascades from TlsMode issue; example cannot call async `connect()`. |
| `examples/basic.rs` | 6 | `connect(...)?` — no `.await`, sync API call | BLOCKER | Three compile errors total from this single stale file. |
| `src/transport.rs` | 36-46 | `connect_tls` stub returns `Pop3Error::Io(Unsupported)` | INFO | Expected per Phase 2 plan; `#[allow(dead_code)]` suppresses warning. Phase 3 will replace the stub. Not a blocker. |

---

## Human Verification Required

### 1. Integration Test Criterion Interpretation

**Test:** Review ROADMAP Phase 2 Success Criterion #2: "All v1.0.6 commands work correctly over a plain TCP connection confirmed by integration tests against a mock server"
**Expected:** Determine whether the 57 unit tests using `tokio_test::io::Builder` in `src/client.rs` satisfy this criterion, or whether a separate integration test suite in `tests/` is required.
**Why human:** The criterion uses "integration tests against a mock server" which could mean (a) any test that exercises the full client-to-wire path (satisfied by the existing mock tests) or (b) a separate test binary in `tests/` that connects to a running mock POP3 server process. The existing tests are comprehensive and cover all commands but they are in-source unit tests, not the traditional Rust `tests/` integration test pattern.

---

## Gaps Summary

Two gaps block full Phase 2 goal achievement:

**Gap 1 — Broken example (blocker):** `examples/basic.rs` was not updated when `TlsMode` was removed and `Pop3Client::connect()` was made async. The file references `pop3::TlsMode` (no longer exported), uses the old 2-argument connect signature, calls async methods without `.await`, and has a synchronous `fn main`. This causes `cargo test` to fail with 3 compile errors. Since the CI workflow runs `cargo test` (not `cargo test --lib`), the CI `test` job will fail on every push. The fix is straightforward: rewrite `examples/basic.rs` to use `#[tokio::main]`, the new `Pop3Client::connect(addr, timeout)` signature, `.await` on all async calls, and remove `TlsMode`.

**Gap 2 — Integration test criterion (uncertain):** ROADMAP Success Criterion #2 specifies "integration tests against a mock server" as the evidence bar. The current test suite (57 tests, all in `src/client.rs` using `tokio_test::io::Builder`) provides solid command coverage but does so via in-source unit tests rather than a `tests/` integration suite. Whether this satisfies the ROADMAP criterion is a judgment call that requires human review. If the criterion is considered satisfied by the existing mock-based tests, documenting this decision closes the gap. If not, a `tests/` integration suite would be required.

**Root cause:** Gap 1 is a direct consequence of the TlsMode removal decision made in Plan 02-02 — the PLANs explicitly called out updating `lib.rs` re-exports and removing `TlsMode`, but `examples/basic.rs` was not in any plan's `files_modified` list and was not updated.

---

_Verified: 2026-03-01T21:00:00Z_
_Verifier: Claude (gsd-verifier)_
