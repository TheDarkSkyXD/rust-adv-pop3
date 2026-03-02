---
phase: 05-pipelining
verified: 2026-03-01T00:00:00Z
status: passed
score: 7/7 must-haves verified
re_verification: false
gaps: []
human_verification: []
---

# Phase 5: Pipelining Verification Report

**Phase Goal:** Callers can send batches of POP3 commands without waiting for individual responses, unlocking high-throughput mail processing while automatically falling back to sequential mode.
**Verified:** 2026-03-01
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| #  | Truth                                                                                               | Status     | Evidence                                                                                      |
|----|-----------------------------------------------------------------------------------------------------|------------|-----------------------------------------------------------------------------------------------|
| 1  | BufWriter wraps Transport writer, enabling batch command accumulation before single flush            | VERIFIED   | `transport.rs:114` — `pub(crate) writer: io::BufWriter<io::WriteHalf<InnerStream>>`          |
| 2  | Transport reader/writer/timeout fields are pub(crate), enabling direct batch method access           | VERIFIED   | `transport.rs:113-115` — all three fields marked `pub(crate)`                                |
| 3  | `Pop3Error::ConnectionClosed` variant exists and is returned on EOF                                  | VERIFIED   | `error.rs:83-84` — variant defined; `transport.rs:386-387` — returned on EOF in `read_line` |
| 4  | `login()` and `apop()` automatically probe CAPA for PIPELINING after successful auth                 | VERIFIED   | `client.rs:421-424` (login), `client.rs:492-494` (apop) — probe runs, sets `is_pipelining`  |
| 5  | `retr_many` and `dele_many` exist as public batch methods with per-item `Vec<Result<T>>` returns     | VERIFIED   | `client.rs:689, 838` — both public async methods with correct signatures                     |
| 6  | Sequential fallback executes when `is_pipelining` is false                                           | VERIFIED   | `client.rs:701-707, 849-855` — loop over single-item calls when not pipelining               |
| 7  | Windowed pipelining sends W=4 commands then drains responses before next window                      | VERIFIED   | `client.rs:71, 720, 868` — `PIPELINE_WINDOW=4`, `ids.chunks(PIPELINE_WINDOW)` in both paths |

**Score:** 7/7 truths verified

### Required Artifacts

| Artifact          | Expected                                          | Status     | Details                                                                 |
|-------------------|---------------------------------------------------|------------|-------------------------------------------------------------------------|
| `src/transport.rs` | BufWriter upgrade, pub(crate) fields, is_closed, ConnectionClosed on EOF | VERIFIED | All present: BufWriter at line 114, pub(crate) fields lines 113-115, is_closed bool at line 117, ConnectionClosed returned at line 387, is_closed() at line 249, set_closed() at line 254 |
| `src/error.rs`    | `Pop3Error::ConnectionClosed` variant             | VERIFIED   | Present at lines 78-84 with full doc comment                            |
| `src/client.rs`   | is_pipelining field, CAPA probe, supports_pipelining(), retr_many, dele_many, windowed private helpers | VERIFIED | All present: field at line 30, PIPELINE_WINDOW const at line 71, supports_pipelining() at line 323, retr_many at line 689, dele_many at line 838, retr_many_pipelined at line 715, dele_many_pipelined at line 863, read_retr_response at line 774 |

### Key Link Verification

| From                        | To                                        | Via                                          | Status  | Details                                                                              |
|-----------------------------|-------------------------------------------|----------------------------------------------|---------|--------------------------------------------------------------------------------------|
| `login()` / `apop()`        | `is_pipelining` field                     | `self.capa().await.unwrap_or_default()`       | WIRED   | Both auth methods set `self.is_pipelining` after CAPA probe (lines 423-424, 493-494) |
| `retr_many()`               | `retr_many_pipelined()` / `retr()` loop   | `if !self.is_pipelining` branch               | WIRED   | Branch at line 701 dispatches to sequential or pipelined path correctly              |
| `dele_many()`               | `dele_many_pipelined()` / `dele()` loop   | `if !self.is_pipelining` branch               | WIRED   | Branch at line 849 dispatches to sequential or pipelined path correctly              |
| `retr_many_pipelined()`     | `transport.writer.write_all()`            | `use tokio::io::AsyncWriteExt` at line 716   | WIRED   | Directly accesses `self.transport.writer` (pub(crate)) and calls `flush()` per window |
| `dele_many_pipelined()`     | `transport.writer.write_all()`            | `use tokio::io::AsyncWriteExt` at line 864   | WIRED   | Same pattern as retr_many_pipelined — direct writer access, single flush per window  |
| `read_line()` EOF path      | `Pop3Error::ConnectionClosed`             | `self.is_closed = true` + `return Err(...)`  | WIRED   | `transport.rs:386-387` — sets flag and returns ConnectionClosed on n==0              |
| `quit()`                    | `transport.set_closed()`                  | After successful QUIT response               | WIRED   | `client.rs:1001` — `this.transport.set_closed()` called before returning Ok(())      |
| `upgrade_in_place()`        | `old_writer.into_inner()`                 | Before `unsplit()`                           | WIRED   | `transport.rs:287` — `old_writer.into_inner()` called to recover WriteHalf           |

### Requirements Coverage

| Requirement | Source Plan | Description                                                                              | Status    | Evidence                                                                                  |
|-------------|-------------|------------------------------------------------------------------------------------------|-----------|-------------------------------------------------------------------------------------------|
| PIPE-01     | 05-02       | Client can send multiple commands without waiting for each response (RFC 2449)           | SATISFIED | `retr_many_pipelined` and `dele_many_pipelined` send all commands in window before reading |
| PIPE-02     | 05-02       | Client automatically detects pipelining support via CAPA after authentication            | SATISFIED | CAPA probe in both `login()` (line 421-424) and `apop()` (line 492-494); auto, no config  |
| PIPE-03     | 05-02       | Client falls back to sequential mode when server does not advertise PIPELINING           | SATISFIED | Sequential loop in `retr_many` (line 701-707) and `dele_many` (line 849-855)             |
| PIPE-04     | 05-02       | Pipelined commands use a windowed send strategy to prevent TCP send-buffer deadlock      | SATISFIED | `PIPELINE_WINDOW=4` constant; `ids.chunks(PIPELINE_WINDOW)` — send window, then drain    |
| PIPE-05     | 05-01, 05-02 | Client provides batch methods (`retr_many`, `dele_many`) that pipeline automatically    | SATISFIED | Both methods public at `client.rs:689` and `client.rs:838`                               |

**Requirements from plan 05-01:** PIPE-05 (infrastructure enabling batch access via pub(crate) fields)
**Requirements from plan 05-02:** PIPE-01, PIPE-02, PIPE-03, PIPE-04, PIPE-05

All 5 PIPE requirements accounted for. No orphaned requirements in REQUIREMENTS.md for Phase 5 (traceability table maps all 5 to Phase 5, all marked Complete).

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| None | — | — | — | — |

No TODOs, FIXMEs, placeholders, empty implementations, or console-log-only stubs detected in phase 5 modified files (`src/transport.rs`, `src/error.rs`, `src/client.rs`).

### Human Verification Required

None. All observable behaviors are verifiable via unit tests with mock I/O. The pipelining detection, batch execution, sequential fallback, windowed strategy, per-item error independence, and order preservation are all covered by 124 unit tests that pass deterministically.

### Test Execution Summary

```
test result: ok. 124 passed; 0 failed; 0 ignored  (unit tests)
test result: ok. 2 passed; 0 failed; 0 ignored    (integration tests)
test result: ok. 27 passed; 0 failed; 4 ignored   (doc tests)
cargo clippy -- -D warnings: PASSED (zero warnings)
cargo fmt --check: PASSED
```

Key phase-5 test coverage:
- `pipelining_detected_via_capa` — PIPE-02 happy path
- `pipelining_not_detected_without_capa_entry` — PIPE-02 no-PIPELINING cap
- `pipelining_false_when_capa_fails` — PIPE-02 CAPA error is silently ignored
- `supports_pipelining_false_before_login` — accessor default value
- `retr_many_sequential_fallback` — PIPE-03 sequential path
- `dele_many_sequential_fallback` — PIPE-03 sequential path
- `retr_many_pipelined_path` — PIPE-01, PIPE-04 windowed path (3 msgs, 1 window)
- `dele_many_pipelined_path` — PIPE-01, PIPE-04 windowed path
- `retr_many_pipelined_per_item_error` — per-item error independence
- `dele_many_pipelined_per_item_error` — per-item error independence
- `retr_many_rejects_zero_id` / `dele_many_rejects_zero_id` — upfront validation
- `retr_many_empty_ids_returns_empty` — empty slice shortcircuit
- `retr_many_requires_auth` / `dele_many_requires_auth` — auth guard
- `retr_many_windowed_8_messages` — PIPE-04 multi-window deadlock prevention (2 windows of 4)
- `is_closed_false_initially` / `is_closed_true_after_eof` (transport + client) — ConnectionClosed infrastructure

### Gaps Summary

No gaps. All 7 must-haves are verified at all three levels (exists, substantive, wired). All 5 PIPE requirements are satisfied. Tests pass with zero failures. No anti-patterns detected.

---

_Verified: 2026-03-01_
_Verifier: Claude (gsd-verifier)_
