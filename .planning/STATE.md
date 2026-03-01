# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-01)

**Core value:** Provide a correct, async, production-quality POP3 client that handles errors gracefully instead of panicking
**Current focus:** Phase 1 — Foundation

## Current Position

Phase: 1 of 9 (Foundation)
Plan: 1 of 2 in current phase
Status: In progress
Last activity: 2026-03-01 — Completed 01-01 (AuthFailed variant, mock transport, 14 bug-proof tests)

Progress: [█░░░░░░░░░] 6%

## Performance Metrics

**Velocity:**
- Total plans completed: 1
- Average duration: 25 min
- Total execution time: 25 min

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 1 | 25 min | 25 min |

**Recent Trend:**
- Last 5 plans: 25 min
- Trend: establishing baseline

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [01-01]: AuthFailed(String) variant added; login() converts ServerError->AuthFailed for semantic auth failure reporting
- [01-01]: Stream::Mock uses Rc<RefCell<Vec<u8>>> not Arc<Mutex> — tests are single-threaded, no overhead needed
- [01-01]: Stream::Mock confined entirely to #[cfg(test)] — no public API leakage, no type parameter on Pop3Client
- [Roadmap]: Async with tokio — industry standard, largest ecosystem
- [Roadmap]: Dual TLS via feature flags (openssl + rustls) — mutual exclusion enforced by compile_error!
- [Roadmap]: Major version bump to v2.0 — API breaking changes justify semver major
- [Roadmap]: Drop sync API — async-only; sync callers use block_on
- [Codebase]: Single-file library at src/pop3.rs (~537 lines), zero existing tests, four known bugs
- [Milestone]: v3.0 Advanced Features defined from GitHub issue #2 — pipelining, UIDL caching, reconnection, pooling, mailparse
- [Roadmap v3.0]: Pipelining is Phase 5 anchor — drives client.rs modifications that Phase 7 and Phase 8 depend on
- [Roadmap v3.0]: UIDL Caching (Phase 6) is independent of Phase 5 changes — can proceed in parallel
- [Roadmap v3.0]: Reconnection (Phase 7) uses Decorator pattern wrapping Client; requires ConnectionClosed error from Phase 5
- [Roadmap v3.0]: Connection Pooling (Phase 8) uses bb8; depends on is_closed() and builder Clone from Phase 5
- [Roadmap v3.0]: MIME Integration (Phase 9) is fully independent from Phases 5-8; can defer to v3.1 without blocking release
- [Roadmap v3.0]: backon 1.6 is unconditional dependency; bb8, serde/serde_json, mail-parser all behind feature flags
- [Roadmap v3.0]: Pool enforces max 1 connection per mailbox (RFC 1939 exclusive lock) — multi-account only, not same-mailbox concurrent

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 3]: OpenSSL build on Windows CI is documented as problematic — may need `vendored` feature or limit openssl support to Linux/macOS. Decide in Phase 3.
- [Phase 4]: STARTTLS BufReader drain behavior is under-documented in tokio ecosystem. Validate against Outlook and Gmail (known to coalesce TCP segments) during Phase 4, not just mock server.
- [Phase 5]: Windowed pipeline implementation — PITFALLS.md specifies 4-8 command window but exact interleave mechanism (select! vs. send/drain interleave) should be prototyped before final design. Resolve in Phase 5 planning.
- [Phase 8]: bb8 per-account exclusivity enforcement — decide whether each Pop3Pool targets one account (single builder per pool) or supports multi-account with runtime enforcement. Public API decision; resolve before Phase 8 begins.
- [Phase 8]: bb8::ManageConnection async fn in traits — verify whether bb8 0.9 supports native async fn in impls (Rust 1.75+) or requires #[async_trait] macro. Check before Phase 8 planning.
- [Phase 9]: Dot-unstuffing precondition — if retr() dot-unstuffing is not covered by a v2.0 test, add a regression test with a dot-stuffed message body as Phase 9 entry gate before any MIME code is written.

## Session Continuity

Last session: 2026-03-01
Stopped at: Completed 01-01-PLAN.md — AuthFailed variant, mock transport infrastructure, 14 bug-proof tests proving FIX-01..FIX-04. Ready for Plan 02.
Resume file: None
