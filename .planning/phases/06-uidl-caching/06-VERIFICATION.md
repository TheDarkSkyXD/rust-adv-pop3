---
phase: 06-uidl-caching
verified: 2026-03-01T00:00:00Z
status: passed
score: 7/7 must-haves verified
re_verification: false
human_verification:
  - test: "Manual smoke-test against a real POP3 server with UIDL support"
    expected: "unseen_uids returns only new messages, fetch_unseen downloads them, prune_seen removes deleted UIDs"
    why_human: "All three methods use tokio_test mock I/O — real server may have different UIDL format quirks"
---

# Phase 6: UIDL Caching Verification Report

**Phase Goal:** Callers can retrieve only messages they have not seen before, and the cache automatically prunes ghost entries so it never incorrectly marks a new message as already seen
**Verified:** 2026-03-01
**Status:** PASSED
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `unseen_uids` exists, has correct signature, and filters by `seen` set | VERIFIED | `src/client.rs:993` — `pub async fn unseen_uids(&mut self, seen: &HashSet<String>) -> Result<Vec<UidlEntry>>` calls `self.uidl(None)` and filters; 5 unit tests pass |
| 2 | `fetch_unseen` exists, delegates to `unseen_uids` (not `uidl` directly), returns `Vec<(UidlEntry, Message)>` | VERIFIED | `src/client.rs:1052-1063` — calls `self.unseen_uids(seen).await?` then loops `self.retr(entry.message_id).await?`; 6 unit tests pass |
| 3 | `prune_seen` mutates the seen set in-place, removes ghost UIDs, returns pruned list | VERIFIED | `src/client.rs:1128-1144` — calls `self.uidl(None)`, builds `HashSet<&str>`, calls `seen.retain(...)`, returns `pruned: Vec<String>`; 5 unit tests pass |
| 4 | No double UIDL round-trip in `fetch_unseen` | VERIFIED | `src/client.rs:1056` — `fetch_unseen` calls `self.unseen_uids(seen).await?` (one call), never calls `self.uidl()` directly |
| 5 | Auth delegation is correct — wrapper methods do not call `require_auth()` | VERIFIED | Grepped `require_auth` usage in all three methods — none found; auth enforced by delegated `uidl()` and `retr()` calls |
| 6 | `# Incremental Sync` rustdoc section heading present, grouping all three methods | VERIFIED | `src/client.rs:950` — `/// # Incremental Sync` section heading with full description block; separator comment at line 946 |
| 7 | `fetch_unseen` rustdoc includes Performance note mentioning `unseen_uids()` + `retr_many()` | VERIFIED | `src/client.rs:1019-1024` — `# Performance` section explicitly references `unseen_uids` and `retr_many` for pipelined use |

**Score:** 7/7 truths verified

---

## Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/client.rs` | Three new public async methods: `unseen_uids`, `fetch_unseen`, `prune_seen` | VERIFIED | All three present at lines 993, 1052, 1128; substantive non-stub implementations; `use std::collections::HashSet` import at line 1 |
| `src/client.rs` (tests) | 15 unit tests for all three methods | VERIFIED | Lines 2477-2729: 5 tests for `unseen_uids`, 6 for `fetch_unseen`, 5 for `prune_seen`; all 15 pass |
| `src/client.rs` (doctests) | 3 `no_run` doc-tests (one per method) | VERIFIED | 3 doc-tests compile successfully; verified by `cargo test --doc` |

---

## Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `fetch_unseen` | `unseen_uids` | `self.unseen_uids(seen).await?` | WIRED | Line 1056 — single delegation, no direct `uidl()` call |
| `fetch_unseen` | `retr()` | `self.retr(entry.message_id).await?` | WIRED | Line 1059 — loop over entries from `unseen_uids` result |
| `unseen_uids` | `uidl()` | `self.uidl(None).await?` | WIRED | Line 994 — single UIDL call |
| `prune_seen` | `uidl()` | `self.uidl(None).await?` | WIRED | Line 1129 — fetches server UID list |
| `prune_seen` | `seen.retain(...)` | closure captures `server_uids` | WIRED | Lines 1135-1141 — `HashSet<&str>` borrow trick; removes ghost UIDs, populates `pruned` |
| Methods | auth enforcement | delegated to `uidl()` / `retr()` | WIRED | No direct `require_auth()` calls in any of the three wrapper methods; enforced by callees |

---

## Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| CACHE-01 | 06-01-PLAN.md | API to filter UIDL list against a set of previously-seen UIDs | SATISFIED | `unseen_uids(&seen)` filters `uidl(None)` result; 5 tests including `unseen_uids_filters_seen_entries` and `unseen_uids_returns_all_when_seen_is_empty` |
| CACHE-02 | 06-01-PLAN.md | Convenience method returning only unseen messages | SATISFIED | `fetch_unseen(&seen)` returns `Vec<(UidlEntry, Message)>`; 6 tests including `fetch_unseen_returns_new_messages_with_entries` and `fetch_unseen_fails_fast_on_retr_error`. Note: REQUIREMENTS.md names this method `fetch_new()` but CONTEXT.md captures the user decision to rename it `fetch_unseen` — the intent is fully satisfied |
| CACHE-03 | 06-01-PLAN.md | UIDL cache reconciliation prunes ghost entries on each connect | SATISFIED | `prune_seen(&mut seen)` removes ghost UIDs, returns pruned list; 5 tests including `prune_seen_removes_ghost_uids` and `prune_seen_empties_seen_when_server_is_empty` |

**Note on CACHE-02 naming discrepancy:** REQUIREMENTS.md and ROADMAP.md Success Criterion 2 reference `fetch_new()` as the method name. CONTEXT.md documents the user's decision to rename this to `fetch_unseen` for consistency with the "unseen" theme. The PLAN.md and implementation both use `fetch_unseen`. This is a stale documentation artifact — the requirement intent is fully satisfied. No gap.

**Orphaned requirements:** None. All three CACHE-* IDs claimed by 06-01-PLAN.md are implemented and verified.

---

## Anti-Patterns Found

| File | Pattern | Severity | Impact |
|------|---------|----------|--------|
| None | — | — | — |

No TODO/FIXME/placeholder comments, no empty implementations, no stub returns found in any of the three new methods or their tests.

---

## Test Results

**`cargo test` (unit + integration):** 139 unit tests pass, 2 integration tests pass (0 failures)
**`cargo test --doc`:** 30 doc-tests pass, 4 ignored (TLS `no_run` tests skipped by design)
**`cargo clippy -- -D warnings`:** Clean — no warnings
**`cargo fmt --check`:** Clean — no formatting issues
**Commits verified:** `45a2308` (feat: implement methods) and `639c385` (test: unit tests) present in git log

---

## Human Verification Required

### 1. Real-server smoke test

**Test:** Connect to a real POP3 server that supports UIDL. Run `prune_seen` with a populated seen set, then `fetch_unseen`. Verify only unseen messages are returned and ghost UIDs are removed.
**Expected:** `prune_seen` removes UIDs not in server UIDL list; `fetch_unseen` downloads only new messages; no double-download on second run.
**Why human:** All tests use `tokio_test::io::Builder` mock I/O. Real servers may have different UIDL response formatting. Integration tests exist for other commands but not for these three new methods.

---

## Gaps Summary

No gaps. All 7 must-haves are verified. The REQUIREMENTS.md `fetch_new()` name is a pre-context stale artifact — the user explicitly renamed it to `fetch_unseen` in CONTEXT.md before planning began. The implementation correctly follows the CONTEXT.md decision.

**All automated checks pass:**
- 15 unit tests for `unseen_uids`, `fetch_unseen`, `prune_seen` — all pass
- 3 doc-tests (one per method) — all compile
- `cargo clippy -- -D warnings` — clean
- `cargo fmt --check` — clean
- No double UIDL round-trip in `fetch_unseen`
- No spurious `require_auth()` calls in wrapper methods
- `# Incremental Sync` section heading present
- Performance note in `fetch_unseen` rustdoc present
- serde_json persistence example in `prune_seen` rustdoc present

---

_Verified: 2026-03-01_
_Verifier: Claude (gsd-verifier)_
