---
phase: 06-uidl-caching
plan: "01"
subsystem: api
tags: [pop3, uidl, incremental-sync, hashset, rust, tokio]

# Dependency graph
requires:
  - phase: 05-pipelining
    provides: retr_many for pipelined bulk fetch (referenced in fetch_unseen Performance note)
provides:
  - unseen_uids: filters UIDL list against caller-supplied seen set (CACHE-01)
  - fetch_unseen: one-call incremental download returning Vec<(UidlEntry, Message)> (CACHE-02)
  - prune_seen: reconciles seen set by removing ghost UIDs, mutates in-place (CACHE-03)
affects: [07-reconnection, 08-connection-pooling]

# Tech tracking
tech-stack:
  added: ["std::collections::HashSet (no new dependencies)"]
  patterns:
    - "Incremental sync via caller-managed seen set — library never persists state"
    - "Delegation chain: fetch_unseen -> unseen_uids -> uidl() (single UIDL round-trip)"
    - "prune_seen builds HashSet<&str> borrowing from server_entries in same scope (borrow lifetime trick)"

key-files:
  created: []
  modified:
    - src/client.rs

key-decisions:
  - "No require_auth() calls in wrapper methods — unseen_uids/fetch_unseen/prune_seen delegate auth enforcement to uidl() and retr()"
  - "fetch_unseen fails fast on first retr() error — no partial results returned"
  - "prune_seen takes &mut HashSet<String> and mutates in-place — caller retains ownership"
  - "Section heading uses doc comment (/// # Incremental Sync) with no blank line before first method to satisfy clippy::empty_line_after_doc_comments"

patterns-established:
  - "Incremental sync pattern: unseen_uids() for filtering, fetch_unseen() for convenience download, prune_seen() for cache reconciliation"
  - "Performance note in fetch_unseen rustdoc pointing callers to unseen_uids() + retr_many() for pipelined use"

requirements-completed: [CACHE-01, CACHE-02, CACHE-03]

# Metrics
duration: 3min
completed: 2026-03-02
---

# Phase 6 Plan 1: UIDL Caching — Incremental Sync Methods Summary

**Three incremental sync methods on Pop3Client — unseen_uids, fetch_unseen, prune_seen — enabling caller-managed UID-based mailbox sync with ghost entry pruning**

## Performance

- **Duration:** ~3 min
- **Started:** 2026-03-02T01:53:48Z
- **Completed:** 2026-03-02T01:56:51Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments

- `unseen_uids(&seen)` filters UIDL response against caller-supplied HashSet, returns only new UidlEntry values (CACHE-01)
- `fetch_unseen(&seen)` calls unseen_uids once then retrieves each unseen message sequentially; fails fast on any retr() error (CACHE-02)
- `prune_seen(&mut seen)` removes ghost UIDs from the caller's set in-place and returns the pruned list for logging (CACHE-03)
- 15 new unit tests plus 3 no_run doc-tests; all 139 unit + 2 integration + 30 doc tests pass
- `# Incremental Sync` rustdoc section heading groups all three methods in impl block

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement unseen_uids, fetch_unseen, prune_seen** - `45a2308` (feat)
2. **Task 2: Add unit tests for all three incremental sync methods** - `639c385` (test)

## Files Created/Modified

- `src/client.rs` - Added `use std::collections::HashSet`, three new public async methods with full rustdoc (including Performance note and serde_json round-trip example in prune_seen), and 15 unit tests

## Decisions Made

- Section heading comment immediately precedes the first method's doc comment with no blank line — required to satisfy `clippy::empty_line_after_doc_comments` lint rule
- Function signatures for `unseen_uids` and `prune_seen` were collapsed to single lines by `cargo fmt` (short enough to fit within line width)
- No `require_auth()` calls in wrapper methods — underlying `uidl()` and `retr()` already enforce auth state (per user decision)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Removed blank line between section heading doc comment and first method doc**
- **Found during:** Task 1 (verification — cargo clippy)
- **Issue:** Blank line after `/// capability if you need to verify support before calling.` triggered `clippy::empty_line_after_doc_comments`
- **Fix:** Removed the blank line between the `/// # Incremental Sync` block comment and the `/// Return UIDL entries...` doc comment for `unseen_uids`
- **Files modified:** src/client.rs
- **Verification:** `cargo clippy -- -D warnings` passes
- **Committed in:** `45a2308` (Task 1 commit)

**2. [Rule 1 - Bug] Applied cargo fmt to collapse short function signatures**
- **Found during:** Task 1 (verification — cargo fmt --check)
- **Issue:** `unseen_uids` and `prune_seen` signatures were written across multiple lines; rustfmt collapsed them to single lines
- **Fix:** Ran `cargo fmt` to auto-apply formatting
- **Files modified:** src/client.rs
- **Verification:** `cargo fmt --check` passes
- **Committed in:** `45a2308` (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (both Rule 1 lint/format fixes)
**Impact on plan:** Both auto-fixes are cosmetic/lint correctness. No scope creep.

## Issues Encountered

None — both lint issues were caught by the Task 1 verification step and fixed inline before commit.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- UIDL caching incremental sync API complete; CACHE-01, CACHE-02, CACHE-03 satisfied
- Phase 6 complete — ready for Phase 7 (Reconnection) or Phase 8 (Connection Pooling)
- Callers can now implement email sync loops without downloading full mailbox on each session

---
*Phase: 06-uidl-caching*
*Completed: 2026-03-02*

## Self-Check: PASSED

- `src/client.rs` — FOUND
- `06-01-SUMMARY.md` — FOUND
- Commit `45a2308` (Task 1) — FOUND
- Commit `639c385` (Task 2) — FOUND
