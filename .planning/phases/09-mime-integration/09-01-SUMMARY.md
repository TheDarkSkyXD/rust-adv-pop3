---
phase: 09-mime-integration
plan: "09-01"
subsystem: mime
tags: [mail-parser, mime, rfc5322, feature-flag, optional-dependency]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: Pop3Error enum, Result type, types.rs structure
  - phase: 02-async-core
    provides: async Pop3Client with retr() and top() methods
provides:
  - retr_parsed() and top_parsed() methods on Pop3Client behind mime feature flag
  - Pop3Error::MimeParse(String) variant (unconditional)
  - ParsedMessage type alias (mail_parser::Message<'static>) re-exported from crate root
  - mail-parser 0.11 optional dependency
  - mime CI test matrix job (rustls-tls + mime combination)
  - examples/mime.rs standalone MIME example
affects: []

# Tech tracking
tech-stack:
  added: [mail-parser 0.11, hashify 0.2 (transitive proc-macro dep)]
  patterns:
    - "Feature-gated optional dependency using dep: syntax in Cargo.toml features"
    - "Unconditional error variant (MimeParse) for exhaustive matching across feature combos"
    - "Type alias instead of newtype to expose upstream API directly"

key-files:
  created:
    - examples/mime.rs
    - .planning/phases/09-mime-integration/09-01-SUMMARY.md
  modified:
    - Cargo.toml
    - src/error.rs
    - src/types.rs
    - src/client.rs
    - src/lib.rs
    - .github/workflows/ci.yml

key-decisions:
  - "ParsedMessage = mail_parser::Message<'static> type alias — exposes full mail-parser API directly, no wrapping"
  - "MimeParse variant is unconditional (not feature-gated) — enables exhaustive matching regardless of feature combination"
  - "Auto-fixed: error-path test mocks use '+OK\\r\\n.\\r\\n' (zero-byte data) because mail-parser returns None only for empty input per best-effort parsing contract"

patterns-established:
  - "Separate #[cfg(feature = \"mime\")] impl block for MIME methods keeps public API organized"
  - "ok_or_else on Option from mail_parser::MessageParser::parse for ergonomic error conversion"

requirements-completed:
  - MIME-01
  - MIME-02

# Metrics
duration: 4min
completed: "2026-03-02"
---

# Phase 09 Plan 01: MIME Parsing via mail-parser Summary

**Optional MIME parsing via mail-parser 0.11 behind mime feature flag — retr_parsed() and top_parsed() return owned mail_parser::Message<'static> with zero binary-size overhead when feature is disabled**

## Performance

- **Duration:** ~4 min
- **Started:** 2026-03-02T04:22:47Z
- **Completed:** 2026-03-02T04:27:00Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- Added `mail-parser = { version = "0.11", optional = true }` and `mime = ["dep:mail-parser"]` feature to Cargo.toml
- Added `Pop3Error::MimeParse(String)` variant unconditionally — callers can exhaustive-match without activating the mime feature
- Added `ParsedMessage` type alias (`mail_parser::Message<'static>`) in types.rs and re-exported from crate root behind `#[cfg(feature = "mime")]`
- Added `retr_parsed()` and `top_parsed()` methods on `Pop3Client` in a separate `#[cfg(feature = "mime")]` impl block
- Added 5 unit tests covering happy path (plain text, multipart MIME, headers-only) and error path for both methods
- Added `test-mime` CI job testing `rustls-tls,mime` feature combination
- Created `examples/mime.rs` standalone example with `required-features = ["mime"]`

## Task Commits

Each task was committed atomically:

1. **Task 1: Add mail-parser dep, MimeParse variant, ParsedMessage alias, retr_parsed/top_parsed, lib.rs re-exports** - `e621703` (feat)
2. **Task 2: Add unit tests, CI matrix entry, examples/mime.rs** - `196791f` (feat)

**Plan metadata:** (pending — to be committed with SUMMARY.md, STATE.md, ROADMAP.md)

## Files Created/Modified

- `Cargo.toml` - Added mail-parser optional dep, mime feature, examples/mime.rs entry
- `src/error.rs` - Added MimeParse(String) variant (unconditional)
- `src/types.rs` - Added ParsedMessage type alias behind #[cfg(feature = "mime")]
- `src/client.rs` - Added #[cfg(feature = "mime")] impl block with retr_parsed/top_parsed + 5 tests
- `src/lib.rs` - Added ParsedMessage re-export, updated feature flags table (added pool and mime rows)
- `.github/workflows/ci.yml` - Added test-mime job (rustls-tls + mime)
- `examples/mime.rs` - Standalone MIME example with required-features

## Decisions Made

- `ParsedMessage` is a type alias not a newtype — callers get the full `mail_parser::Message<'static>` API without wrapping overhead
- `MimeParse` variant exists unconditionally — per locked decision #10, enables exhaustive pattern matching regardless of whether the mime feature is active
- Used `ok_or_else` on `Option` from `MessageParser::parse` for clean error conversion without nesting

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Corrected error-path test mock data for MimeParse tests**

- **Found during:** Task 2 (unit tests)
- **Issue:** Plan specified `b"+OK\r\n\r\n.\r\n"` (blank line then dot) as test input for the "malformed message" error path. However, `mail-parser` follows best-effort parsing (locked decision #8) and returns `Some(...)` for content with an empty body — `parse()` only returns `None` for zero-length input (empty bytes). The blank line produces `data = "\n"` which is non-empty, so no error is triggered.
- **Fix:** Changed the mock to `b"+OK\r\n.\r\n"` (immediate dot terminator, no blank line). This produces `data = ""` (zero bytes), which causes `MessageParser::parse(b"")` to return `None` as expected.
- **Files modified:** `src/client.rs` (tests `retr_parsed_returns_mime_error_for_malformed_message` and `top_parsed_returns_mime_error_for_garbage`)
- **Verification:** Both tests pass; 168/168 tests pass with `--features mime`
- **Committed in:** `196791f` (Task 2 commit)

**2. [Rule 1 - Bug] Fixed rustfmt formatting in examples/mime.rs**

- **Found during:** Task 2 (cargo fmt --check)
- **Issue:** The multi-line `Pop3Client::connect(...)` call style did not match rustfmt's output
- **Fix:** Reformatted to rustfmt's preferred style: `let mut client = Pop3Client::connect(...).await?;`
- **Files modified:** `examples/mime.rs`
- **Verification:** `cargo fmt --check` passes
- **Committed in:** `196791f` (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (both Rule 1 — behavior mismatch with actual library API)
**Impact on plan:** Both auto-fixes necessary for correctness; no scope creep.

## Issues Encountered

None beyond the auto-fixed deviations above.

## Next Phase Readiness

- Phase 9 (MIME Integration) is now complete — this is the only plan in the phase
- All 9 phases complete, milestone v2.0 achieved
- Crate now provides optional async MIME parsing behind the `mime` feature flag
- No blockers for crates.io publish

---
*Phase: 09-mime-integration*
*Completed: 2026-03-02*
