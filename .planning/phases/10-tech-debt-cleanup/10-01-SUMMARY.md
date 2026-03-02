---
phase: 10-tech-debt-cleanup
plan: "01"
subsystem: transport, pool
tags: [rust, async, tokio, bb8, pop3, starttls, dead_code, clippy]

# Dependency graph
requires:
  - phase: 08-connection-pooling
    provides: Pop3ConnectionManager, bb8 pool, SessionState-based client
  - phase: 03-tls-and-publish
    provides: transport.rs STARTTLS upgrade_in_place, tls_handshake helpers
provides:
  - Stale #[allow(dead_code)] annotations removed from transport.rs
  - Plan-phase doc references removed from transport.rs
  - Double-login guard on Pop3ConnectionManager::connect()
  - Auth rustdoc section on Pop3ConnectionManager
  - Unit test validating authenticated-client precondition
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Double-login guard: check client.state() != Authenticated before calling login() in pool connect()"
    - "No stale plan-phase references in shipped source — only current functionality described"

key-files:
  created: []
  modified:
    - src/transport.rs
    - src/pool.rs

key-decisions:
  - "[10-01]: Double-login guard uses client.state() != SessionState::Authenticated — idiomatic, leverages existing SessionState enum"
  - "[10-01]: Three stale #[allow(dead_code)] removed; two legitimate ones preserved (Upgrading variant, no-TLS connect_tls stub)"

patterns-established:
  - "Guard pattern: check session state before issuing auth commands to prevent redundant round-trips"

requirements-completed: []

# Metrics
duration: 8min
completed: 2026-03-01
---

# Phase 10 Plan 01: Tech Debt Cleanup Summary

**Removed three stale `#[allow(dead_code)]` annotations and all plan-phase doc references from transport.rs; added double-login guard to `Pop3ConnectionManager::connect()` with SessionState check, auth rustdoc, and guard precondition unit test.**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-01T00:00:00Z
- **Completed:** 2026-03-01T00:08:00Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Removed `#[allow(dead_code)]` from `upgrade_in_place`, `tls_handshake` (rustls), and `tls_handshake` (openssl) — these methods are now referenced/reachable and the annotations were stale
- Removed "Plan 02" and "Plan 0X" references from `transport.rs` doc comments; updated `Upgrading` variant doc to clean prose without phase references
- Added `if client.state() != SessionState::Authenticated` guard in `Pop3ConnectionManager::connect()` preventing redundant USER/PASS when builder already auto-authenticated
- Added `# Authentication` rustdoc section on `Pop3ConnectionManager` documenting the guard behavior
- Added `authenticated_client_state_is_authenticated` unit test in `pool::tests` verifying the guard's precondition
- All 194 tests pass; clippy zero warnings on `--features rustls-tls,pool,mime` and `--no-default-features --features pool`

## Task Commits

Each task was committed atomically:

1. **Task 1: Remove stale dead_code annotations and plan references from transport.rs** - `80e1eb0` (chore)
2. **Task 2: Add double-login guard and auth rustdoc to Pop3ConnectionManager** - `76d7dd3` (feat)

**Plan metadata:** (docs commit — see below)

## Files Created/Modified
- `src/transport.rs` - Removed 3 stale `#[allow(dead_code)]` lines and "Plan 02" references; updated Upgrading variant doc
- `src/pool.rs` - Added SessionState import, double-login guard in connect(), auth rustdoc section, new unit test

## Decisions Made
- Double-login guard uses `client.state() != SessionState::Authenticated` — idiomatic and leverages existing `SessionState` enum without additional types
- Preserved the two legitimate `#[allow(dead_code)]` annotations: `Upgrading` variant (genuinely transient, never matched externally) and no-TLS `connect_tls` stub (dead by design when TLS features are inactive)

## Deviations from Plan

None — plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

All advisory gaps from the v2.0+v3.0 milestone audit are closed. Phase 10 (tech debt cleanup) is complete.

---
*Phase: 10-tech-debt-cleanup*
*Completed: 2026-03-01*
