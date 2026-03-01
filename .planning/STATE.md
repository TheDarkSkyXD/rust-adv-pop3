---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: milestone
status: in_progress
last_updated: "2026-03-01T23:30:00.000Z"
progress:
  total_phases: 9
  completed_phases: 3
  total_plans: 13
  completed_plans: 10
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-01)

**Core value:** Provide a correct, async, production-quality POP3 client that handles errors gracefully instead of panicking
**Current focus:** Phase 3 complete — TLS and Publish (all 4 plans done). Ready for Phase 4.

## Current Position

Phase: 3 of 9 (TLS and Publish) — COMPLETE
Plan: 4 of 4 in current phase (just completed)
Status: Phase 3 complete — rustdoc on all public items, CI matrix for dual TLS backends, TLS examples, README, Cargo.toml publish-ready. cargo publish --dry-run passes.
Last activity: 2026-03-01 — Completed 03-04 (comprehensive rustdoc, CI matrix, examples/tls.rs, examples/starttls.rs, README.md rewrite, Cargo.toml metadata update)

Progress: [███░░░░░░░] 33%

## Performance Metrics

**Velocity:**
- Total plans completed: 10
- Average duration: ~11 min
- Total execution time: ~140 min

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 2 | 40 min | 20 min |
| 02-async-core | 4 | ~40 min | ~10 min |
| 03-tls-and-publish | 4 completed | ~60 min | ~15 min |

**Recent Trend:**
- Last 5 plans: ~4 min, 4 min, ~7 min, ~8 min, ~45 min
- Trend: Plan 04 was longer due to context window continuation and comprehensive documentation work

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
- [03-02]: Upgrading variant instead of Option<> fields — keeps Transport struct simple, placeholder only lives during mem::replace
- [03-02]: tls_handshake() private helper — avoids code duplication between connect_tls and upgrade_in_place for both rustls and openssl backends
- [03-02]: stls() RFC 2595 guard checks SessionState::Authenticated — STLS only valid in AUTHORIZATION state before login
- [03-02]: upgrade_in_place rejects non-Plain streams — Mock cannot be upgraded (returns Tls error), tests verify error on mock
- [03-02]: #[allow(dead_code)] on no-TLS stub — stub unreachable without TLS feature, feature-gated client method unavailable
- [03-03]: Mock server uses BufReader::read_line() to read one CRLF-terminated command at a time — prevents TCP coalescing causing empty reads on Windows
- [03-03]: Integration tests split: tests/integration.rs for true public-API-over-TCP tests; src/client.rs for multi-command flows using internal mock infrastructure
- [03-03]: is_encrypted() added as public method on Pop3Client (delegates to Transport::is_encrypted) to satisfy public API interface spec
- [03-04]: required-features on [[example]] TLS entries — prevents build failure when feature not enabled
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
- [Phase 03-04]: no_run for all network doctests — compile-verified without requiring a real POP3 server in CI
- [Phase 03-04]: CI matrix tests rustls-tls and openssl-tls independently, plus plain no-TLS build
- [Phase 03-04]: Repository URL updated to TheDarkSkyXD fork; readme field added to Cargo.toml for crates.io

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 3 - Resolved]: OpenSSL connect_tls and tls_handshake implemented but not integration-tested — CI matrix (Plan 04) tests on ubuntu-latest in GitHub Actions.
- [Phase 3 - Resolved]: OpenSSL build on Windows CI — CI matrix uses libssl-dev install step on ubuntu-latest, gated to openssl-tls matrix leg.
- [Phase 4]: STARTTLS BufReader drain behavior is under-documented in tokio ecosystem. Validate against Outlook and Gmail (known to coalesce TCP segments) during Phase 4, not just mock server.
- [Phase 5]: Windowed pipeline implementation — PITFALLS.md specifies 4-8 command window but exact interleave mechanism (select! vs. send/drain interleave) should be prototyped before final design. Resolve in Phase 5 planning.
- [Phase 8]: bb8 per-account exclusivity enforcement — decide whether each Pop3Pool targets one account (single builder per pool) or supports multi-account with runtime enforcement. Public API decision; resolve before Phase 8 begins.
- [Phase 8]: bb8::ManageConnection async fn in traits — verify whether bb8 0.9 supports native async fn in impls (Rust 1.75+) or requires #[async_trait] macro. Check before Phase 8 planning.
- [Phase 9]: Dot-unstuffing precondition — RESOLVED in 01-02: retr_dot_unstuffing test confirms end-to-end dot-unstuffing works. Phase 9 entry gate satisfied.
- [Environment]: Windows MSVC linker (link.exe) fails when compiling C-using build scripts (ring, getrandom, rustls). Pure-Rust tests and cargo check work fine. cargo clippy requires full recompile when cache is stale — may fail with linker error. This is a pre-existing environment constraint.

## Session Continuity

Last session: 2026-03-01
Stopped at: Completed 03-04-PLAN.md — comprehensive rustdoc on all public items, CI matrix for rustls-tls and openssl-tls backends, examples/tls.rs and examples/starttls.rs, README.md rewrite (async v2 API), Cargo.toml publish prep. All 90 tests pass (66 unit + 2 integration + 22 doc). cargo publish --dry-run passes. Phase 3 (TLS and Publish) is COMPLETE. Ready for Phase 4 planning.
Resume file: None
