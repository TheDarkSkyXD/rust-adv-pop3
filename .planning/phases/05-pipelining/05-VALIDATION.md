---
phase: 5
slug: pipelining
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-01
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust inline `#[cfg(test)]` modules with `tokio_test::io::Builder`) |
| **Config file** | none — inline test modules in source files |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo test && cargo clippy -- -D warnings && cargo fmt --check` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo test && cargo clippy -- -D warnings && cargo fmt --check`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 05-01-01 | 01 | 1 | PIPE-05 (infra) | unit | `cargo test is_closed` | ❌ W0 | ⬜ pending |
| 05-01-02 | 01 | 1 | PIPE-05 (infra) | unit | `cargo test builder_is_clone` | ❌ W0 | ⬜ pending |
| 05-02-01 | 02 | 1 | PIPE-02 | unit | `cargo test pipelining_detected_via_capa` | ❌ W0 | ⬜ pending |
| 05-02-02 | 02 | 1 | PIPE-03 | unit | `cargo test retr_many_sequential_fallback` | ❌ W0 | ⬜ pending |
| 05-03-01 | 03 | 2 | PIPE-01, PIPE-04 | unit | `cargo test retr_many_pipelined` | ❌ W0 | ⬜ pending |
| 05-03-02 | 03 | 2 | PIPE-05 | unit | `cargo test dele_many_pipelined` | ❌ W0 | ⬜ pending |
| 05-03-03 | 03 | 2 | PIPE-04 | unit | `cargo test retr_many_large_batch_no_deadlock` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending / ✅ green / ❌ red / ⚠️ flaky*

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements. The project already has:
- `tokio_test::io::Builder` mock infrastructure in `src/client.rs`
- `build_test_client()` and `build_authenticated_test_client()` helpers
- Inline `#[cfg(test)]` module pattern

No new test framework setup needed. New tests follow the established pattern.

---

## Manual-Only Verifications

All phase behaviors have automated verification.

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
