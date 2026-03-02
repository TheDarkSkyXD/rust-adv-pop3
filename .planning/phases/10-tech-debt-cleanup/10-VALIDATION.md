---
phase: 10
slug: tech-debt-cleanup
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-01
---

# Phase 10 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`#[test]` / `#[tokio::test]`) |
| **Config file** | none — inline `#[cfg(test)]` modules |
| **Quick run command** | `cargo test --features pool` |
| **Full suite command** | `cargo test --features rustls-tls,pool,mime && cargo test --no-default-features --features pool` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo clippy --features rustls-tls,pool -- -D warnings`
- **After every plan wave:** Run `cargo test --features rustls-tls,pool,mime`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 10-01-01 | 01 | 1 | SC-2 | compile | `cargo clippy --features rustls-tls,pool -- -D warnings` | ✅ | ⬜ pending |
| 10-01-02 | 01 | 1 | SC-2b | compile | `cargo build --no-default-features --features pool` | ✅ | ⬜ pending |
| 10-01-03 | 01 | 1 | SC-3 | static | `grep -rn "Plan 0\|Plan 1" src/` | ✅ | ⬜ pending |
| 10-01-04 | 01 | 2 | SC-1 | unit | `cargo test --features pool connect_skips_login` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/pool.rs` — new test `connect_skips_login_when_already_authenticated` (covers SC-1)

*Existing infrastructure covers all other phase requirements.*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
