---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: milestone
status: in_progress
last_updated: "2026-03-01T22:30:00.000Z"
progress:
  total_phases: 9
  completed_phases: 2
  total_plans: 10
  completed_plans: 7
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-01)

**Core value:** Provide a correct, async, production-quality POP3 client that handles errors gracefully instead of panicking
**Current focus:** Phase 3 — TLS and Publish (plan 01 of 04 complete)

## Current Position

Phase: 3 of 9 (TLS and Publish)
Plan: 1 of 4 in current phase (just completed)
Status: Phase 3 plan 01 complete — TLS foundation (InnerStream enum, feature flags, rustls connect_tls, Pop3Client TLS methods, STARTTLS scaffolding forward-ported)
Last activity: 2026-03-01 — Completed 03-01 (TLS foundation: InnerStream enum, tokio-rustls 0.26 with ring backend, Pop3Client::connect_tls/connect_tls_default/is_encrypted/stls() methods)

Progress: [███░░░░░░░] 22%

## Performance Metrics

**Velocity:**
- Total plans completed: 7
- Average duration: ~10 min
- Total execution time: ~87 min

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 2 | 40 min | 20 min |
| 02-async-core | 4 | ~40 min | ~10 min |
| 03-tls-and-publish | 1 completed | ~7 min | ~7 min |

**Recent Trend:**
- Last 5 plans: 4 min, ~5 min, ~4 min, 4 min, ~7 min
- Trend: consistently fast (4-7 min range)

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [01-01]: AuthFailed(String) variant added; login() converts ServerError->AuthFailed for semantic auth failure reporting
- [01-01]: Stream::Mock uses Rc<RefCell<Vec<u8>>> not Arc<Mutex> — tests are single-threaded, no overhead needed
- [01-01]: Stream::Mock confined entirely to #[cfg(test)] — no public API leakage, no type parameter on Pop3Client
- [01-02]: UidlEntry field is unique_id not uid — corrected from plan template, confirmed by compiler
- [01-02]: capa() and quit() use build_test_client (not authenticated) as production code does not call require_auth() for these
- [02-03]: CI uses dtolnay/rust-toolchain@stable (not actions-rs/*) on ubuntu-latest only — cross-platform deferred to Phase 3
- [02-03]: TlsMode enum removed from Phase 2 public API — Phase 3 reintroduces TLS connection methods
- [02-03]: quit(self) consumes the client — move semantics provide compile-time use-after-disconnect prevention
- [02-03]: SessionState replaces authenticated: bool — enables callers to match on Connected/Authenticated/Disconnected
- [02-03]: login() returns NotAuthenticated if state != Connected — prevents double-login bugs
- [02-04]: ROADMAP criterion #2 "integration tests against a mock server" satisfied by existing 57 tokio_test::io::Builder tests — these exercise the full client->transport->mock I/O path for all commands; no separate tests/ integration suite required
- [02-04]: examples/basic.rs fixed (commit 7cfd455) — now uses async v2 API with #[tokio::main], .await, and correct connect signature
- [03-01]: Use ring crypto backend for tokio-rustls (not aws-lc-rs default) — aws-lc-sys requires dlltool.exe on Windows; ring builds cleanly everywhere
- [03-01]: Box<TlsStream<TcpStream>> in InnerStream::RustlsTls — reduces enum size from 1104 bytes to pointer size, satisfies clippy::large_enum_variant
- [03-01]: compile_error! positioned after //! crate doc block — inner doc comments must precede all items including #[cfg(...)]
- [03-01]: Pop3Error::Tls(String) replaces #[from] rustls::Error — backend-agnostic, no rustls type in public API
- [03-01]: Upgrading variant added to InnerStream for STARTTLS placeholder swap — upgrade_in_place forward-ported from Plan 02
- [03-03]: Mock server uses BufReader::read_line() to read one CRLF-terminated command at a time — prevents TCP coalescing causing empty reads on Windows
- [03-03]: Integration tests split: tests/integration.rs for true public-API-over-TCP tests; src/client.rs for multi-command flows using internal mock infrastructure
- [03-03]: is_encrypted() added as public method on Pop3Client (delegates to Transport::is_encrypted) to satisfy public API interface spec
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

- [Phase 3]: OpenSSL connect_tls forward-ported but not tested — validate on Linux/macOS in Phase 3 plan 02.
- [Phase 3]: OpenSSL build on Windows CI is documented as problematic — may need `vendored` feature or limit openssl support to Linux/macOS. Decide in Phase 3.
- [Phase 4]: STARTTLS BufReader drain behavior is under-documented in tokio ecosystem. Validate against Outlook and Gmail (known to coalesce TCP segments) during Phase 4, not just mock server.
- [Phase 5]: Windowed pipeline implementation — PITFALLS.md specifies 4-8 command window but exact interleave mechanism (select! vs. send/drain interleave) should be prototyped before final design. Resolve in Phase 5 planning.
- [Phase 8]: bb8 per-account exclusivity enforcement — decide whether each Pop3Pool targets one account (single builder per pool) or supports multi-account with runtime enforcement. Public API decision; resolve before Phase 8 begins.
- [Phase 8]: bb8::ManageConnection async fn in traits — verify whether bb8 0.9 supports native async fn in impls (Rust 1.75+) or requires #[async_trait] macro. Check before Phase 8 planning.
- [Phase 9]: Dot-unstuffing precondition — RESOLVED in 01-02: retr_dot_unstuffing test confirms end-to-end dot-unstuffing works. Phase 9 entry gate satisfied.
- [Environment]: Windows MSVC linker (link.exe) fails when compiling C-using build scripts (ring, getrandom, rustls). Pure-Rust tests and cargo check work fine. cargo clippy requires full recompile when cache is stale — may fail with linker error. This is a pre-existing environment constraint.

## Session Continuity

Last session: 2026-03-01
Stopped at: Completed 03-01-PLAN.md — TLS foundation complete. InnerStream enum, feature flags (rustls-tls default, openssl-tls optional), tokio-rustls 0.26 with ring backend, Pop3Client::connect_tls/connect_tls_default/is_encrypted/stls() methods. upgrade_in_place STARTTLS method forward-ported. All 76 tests pass (66 unit + 2 integration + 8 doc). Ready for Phase 3 plan 02 (STARTTLS).
Resume file: None
