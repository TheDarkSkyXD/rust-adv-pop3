---
phase: 04-protocol-extensions
plan: "02"
subsystem: api
tags: [rust, builder-pattern, fluent-api, tls, pop3]

# Dependency graph
requires:
  - phase: 04-01
    provides: Pop3Client::apop() method that builder delegates to for APOP auto-auth

provides:
  - Pop3ClientBuilder struct with fluent consuming-chain API
  - Smart port defaults (110 plain/STARTTLS, 995 TLS)
  - Auto-auth via .credentials() (USER/PASS) and .apop() (APOP) on connect()
  - Feature-gated .tls() and .starttls() builder methods
  - Pop3ClientBuilder re-exported from crate root

affects: [05-pipelining, 06-uidl-caching, 07-reconnection, 08-connection-pooling]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Consuming-chain builder pattern (methods return Self, connect() is terminal)
    - Internal enum guards for TLS mode and auth mode (last-wins semantics)
    - Feature-gated pub methods via #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]

key-files:
  created:
    - src/builder.rs
  modified:
    - src/lib.rs

key-decisions:
  - "Pop3ClientBuilder::new(hostname) only — no convenience ::plain()/::tls() constructors; chain is fluent enough"
  - "Internal TlsMode and AuthMode enums are private — only builder methods are public API"
  - "Last-wins semantics for both TLS mode and auth mode — consistent, predictable behavior"
  - "Builder derives Debug and Clone — Clone required by Phase 8 connection pooling (bb8 reuse)"

patterns-established:
  - "Builder pattern: consuming chain via mut self, terminal async fn connect() returns Result<Pop3Client>"
  - "Smart defaults: port derived from tls_mode in effective_port() helper when no explicit .port() override"

requirements-completed: [API-01, API-02]

# Metrics
duration: 2min
completed: "2026-03-02"
---

# Phase 4 Plan 02: Pop3ClientBuilder Fluent API Summary

**Pop3ClientBuilder with consuming-chain fluent API, smart port defaults (110/995), and auto-auth via .credentials()/.apop() on connect()**

## Performance

- **Duration:** ~2 min
- **Started:** 2026-03-02T00:39:27Z
- **Completed:** 2026-03-02T00:41:17Z
- **Tasks:** 2 (both committed in single atomic commit)
- **Files modified:** 2

## Accomplishments

- Created `src/builder.rs` with `Pop3ClientBuilder` struct, internal `TlsMode` and `AuthMode` enums, all builder configuration methods (consuming chain style), and async `connect()` terminal method
- 16 unit tests covering port defaults, TLS last-wins precedence, auth mode setting, hostname storage, Debug/Clone derives, and consuming chain compilation
- Wired `mod builder` and `pub use builder::Pop3ClientBuilder` into `src/lib.rs`
- All 103 unit tests + 2 integration tests + 27 doc tests pass; clippy and fmt clean

## Task Commits

Each task was committed atomically:

1. **Tasks 1 & 2: builder struct + tests + lib.rs wiring** - `c87db8e` (feat)

**Plan metadata:** (docs commit follows)

## Files Created/Modified

- `src/builder.rs` - Pop3ClientBuilder with TlsMode/AuthMode enums, builder methods, connect(), and 16 unit tests
- `src/lib.rs` - Added `mod builder` declaration and `pub use builder::Pop3ClientBuilder` re-export

## Decisions Made

- `Pop3ClientBuilder::new(hostname)` only — no convenience `::plain()`/`::tls()` constructors; the builder chain is fluent enough (per CONTEXT.md direction)
- Internal `TlsMode` and `AuthMode` enums kept private — only builder methods are public API surface
- Last-wins semantics for both TLS mode and auth mode: calling `.tls().starttls()` results in STARTTLS; calling `.apop(...).credentials(...)` results in USER/PASS auth
- Builder derives `Debug` and `Clone` — `Clone` specifically required by Phase 8 connection pooling design where bb8 reuses the builder to create new connections

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed cargo fmt formatting in test**
- **Found during:** Task 2 verification (cargo fmt --check)
- **Issue:** `timeout_is_configurable` test had a line break that rustfmt rejects — the builder call was split across two lines when it fits on one
- **Fix:** Collapsed to single line: `let builder = Pop3ClientBuilder::new("host.example.com").timeout(Duration::from_secs(60));`
- **Files modified:** src/builder.rs
- **Verification:** `cargo fmt --check` passes with no diff
- **Committed in:** c87db8e (combined task commit)

---

**Total deviations:** 1 auto-fixed (1 blocking — fmt violation)
**Impact on plan:** Trivial formatting fix, no functional change. No scope creep.

## Issues Encountered

None beyond the formatting fix above.

## Next Phase Readiness

- Phase 4 complete: RESP-CODES parsing + APOP authentication (04-01) and Pop3ClientBuilder fluent API (04-02)
- `Pop3ClientBuilder` is now the recommended entry point to the library
- Phase 5 (Pipelining) can proceed — builder's `Clone` derive satisfies Phase 8 connection pooling requirement noted in roadmap

---
*Phase: 04-protocol-extensions*
*Completed: 2026-03-02*
