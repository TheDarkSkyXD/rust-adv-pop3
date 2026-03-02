---
phase: 04-protocol-extensions
plan: "01"
subsystem: auth
tags: [pop3, resp-codes, apop, md5, error-handling, rfc1939, rfc2449]

# Dependency graph
requires:
  - phase: 03-tls-and-publish
    provides: Pop3Error enum, parse_status_line, SessionState, client.rs architecture

provides:
  - Pop3Error::MailboxInUse, LoginDelay, SysTemp, SysPerm variants
  - parse_resp_code() helper mapping [CODE] brackets to typed errors
  - parse_status_line() dispatches -ERR [CODE] to typed variants
  - apop() method with #[deprecated] and MD5 security warning rustdoc
  - extract_apop_timestamp() and compute_apop_digest() helpers
  - md5 = "0.7" dependency

affects: [04-02-builder-api, future-auth-phases]

# Tech tracking
tech-stack:
  added: [md5 = "0.7"]
  patterns:
    - RESP-CODE bracket parsing dispatched via parse_resp_code() before ServerError fallthrough
    - Deprecated method pattern with #[deprecated] attribute and prominent Security Warning rustdoc
    - Test helper variant (build_test_client_with_greeting) for greeting-dependent tests

key-files:
  created: []
  modified:
    - src/error.rs
    - src/response.rs
    - src/client.rs
    - Cargo.toml

key-decisions:
  - "[04-01]: parse_resp_code() strips bracket code, keeps text after ] — consistent with ServerError stripping -ERR"
  - "[04-01]: [AUTH] RESP-CODE maps to AuthFailed (not a new variant) — merges into existing semantic error"
  - "[04-01]: Unknown RESP-CODEs fall through to ServerError with full text preserved"
  - "[04-01]: apop() returns ServerError immediately if no timestamp in greeting — no silent fallback"
  - "[04-01]: apop() map_err promotes ServerError->AuthFailed; RESP-CODE variants pass through unchanged"
  - "[04-01]: apop() deprecated with note referencing login() as the preferred alternative"

patterns-established:
  - "RESP-CODE parsing: strip prefix, match code, map to typed variant, fall through to ServerError"
  - "Auth method error promotion: ServerError->AuthFailed for plain -ERR, RESP-CODEs pass through"

requirements-completed: [CMD-03, CMD-04]

# Metrics
duration: 4min
completed: 2026-03-01
---

# Phase 4 Plan 01: RESP-CODES Parsing + APOP Authentication Summary

**RFC 2449 RESP-CODE parsing dispatching -ERR [CODE] to 4 new typed Pop3Error variants, plus RFC 1939 APOP authentication with MD5 digest via md5 crate and #[deprecated] security warning**

## Performance

- **Duration:** ~4 min
- **Started:** 2026-03-01T22:52:23Z
- **Completed:** 2026-03-01T22:55:43Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Added `MailboxInUse`, `LoginDelay`, `SysTemp`, `SysPerm` to `Pop3Error` for structured RESP-CODE errors
- Updated `parse_status_line()` to dispatch `-ERR [CODE]` responses to typed variants via `parse_resp_code()` helper
- Implemented `apop()` method with MD5 digest (RFC 1939 test vector verified: `c4c9334bac560ecc979e58001b3e22fb`)
- `apop()` carries `#[deprecated]` attribute with clear `# Security Warning` rustdoc explaining MD5 is broken
- 88 unit tests + 2 integration tests + 20 doc tests all pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Add RESP-CODE error variants and update parse_status_line** - `ece7410` (feat)
2. **Task 2: Add APOP authentication method with MD5 and tests** - `4f5cdad` (feat)

**Plan metadata:** (docs commit follows)

## Files Created/Modified

- `src/error.rs` - Added MailboxInUse, LoginDelay, SysTemp, SysPerm variants before ServerError
- `src/response.rs` - Added parse_resp_code() helper, updated parse_status_line() -ERR branch, added 7 RESP-CODE tests
- `src/client.rs` - Added extract_apop_timestamp(), compute_apop_digest(), apop() method, build_test_client_with_greeting(), 15 new tests (2 login RESP-CODE + 13 APOP)
- `Cargo.toml` - Added md5 = "0.7" dependency

## Decisions Made

- `parse_resp_code()` strips the bracket code from the message (e.g., `[IN-USE] msg` becomes `msg`) for consistency with `ServerError` stripping `-ERR`
- `[AUTH]` RESP-CODE maps directly to the existing `AuthFailed` variant (no new variant needed)
- Unknown RESP-CODEs preserve the full text including brackets in `ServerError`
- `apop()` returns `ServerError` with "no timestamp in greeting" if greeting lacks `<...>` (no silent fallback to USER/PASS)
- `apop()` map_err promotes `ServerError->AuthFailed` for plain `-ERR`; RESP-CODE variants (MailboxInUse etc.) pass through unchanged

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Applied rustfmt formatting to test code**

- **Found during:** Task 2 (APOP tests)
- **Issue:** `cargo fmt --check` reported formatting differences on multi-arg `build_test_client_with_greeting()` calls in tests — rustfmt collapsed them from multi-line to single line
- **Fix:** Ran `cargo fmt` to apply canonical formatting
- **Files modified:** `src/client.rs`
- **Verification:** `cargo fmt --check` passes; all 88 tests still pass
- **Committed in:** 4f5cdad (part of Task 2 commit)

---

**Total deviations:** 1 auto-fixed (formatting — Rule 3 blocking)
**Impact on plan:** Trivial formatting only. No logic or scope changes.

## Issues Encountered

None — plan executed cleanly. The RFC 1939 section 7 test vector (`c4c9334bac560ecc979e58001b3e22fb`) matched exactly on first attempt.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- RESP-CODES and APOP complete — ready for Plan 02 (builder API) which adds `.apop()` on the builder
- `apop()` standalone method exists; builder `.apop()` is Plan 02's task
- All 88 unit + 2 integration + 20 doc tests passing; clippy and fmt clean

---
*Phase: 04-protocol-extensions*
*Completed: 2026-03-01*
