---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: milestone
status: unknown
last_updated: "2026-03-02T04:00:38.639Z"
progress:
  total_phases: 9
  completed_phases: 8
  total_plans: 19
  completed_plans: 19
---

---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: milestone
status: unknown
last_updated: "2026-03-02T03:52:32.077Z"
progress:
  total_phases: 9
  completed_phases: 7
  total_plans: 19
  completed_plans: 18
---

---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: milestone
status: unknown
last_updated: "2026-03-02T02:47:23.923Z"
progress:
  total_phases: 9
  completed_phases: 7
  total_plans: 17
  completed_plans: 17
---

---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: milestone
status: unknown
last_updated: "2026-03-02T02:36:10.737Z"
progress:
  total_phases: 9
  completed_phases: 6
  total_plans: 17
  completed_plans: 16
---

---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: milestone
status: unknown
last_updated: "2026-03-02T02:02:55.778Z"
progress:
  total_phases: 9
  completed_phases: 6
  total_plans: 15
  completed_plans: 15
---

---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: milestone
status: unknown
last_updated: "2026-03-02T01:57:47.392Z"
progress:
  total_phases: 9
  completed_phases: 6
  total_plans: 15
  completed_plans: 15
---

---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: milestone
status: unknown
last_updated: "2026-03-02T01:23:56.179Z"
progress:
  total_phases: 9
  completed_phases: 5
  total_plans: 14
  completed_plans: 14
---

---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: milestone
status: in_progress
last_updated: "2026-03-02T01:19:00Z"
progress:
  total_phases: 9
  completed_phases: 5
  total_plans: 13
  completed_plans: 13
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-01)

**Core value:** Provide a correct, async, production-quality POP3 client that handles errors gracefully instead of panicking
**Current focus:** Phase 9 (MIME Integration) — Phase 8 complete. Ready for 09-01.

## Current Position

Phase: 8 of 9 (Connection Pooling) — Complete
Plan: 2 of 2 in current phase (08-02 just completed)
Status: Phase 8 complete — Pop3Pool registry, PoolConfig, PooledConnection alias, RFC 1939 rustdoc, lib.rs re-exports; 25 pool tests passing, all POOL requirements satisfied.
Last activity: 2026-03-02 — Completed 08-02 (Pop3Pool Registry)

Progress: [█████████░] 94%

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
| 04-protocol-extensions | 2 of 2 completed | ~6 min | ~3 min |
| 05-pipelining | 2 of 2 completed | ~8 min | ~4 min |
| 06-uidl-caching | 1 of 1 completed | ~3 min | ~3 min |
| 07-reconnection | 2 of 2 complete | ~7 min | ~4 min |
| 08-connection-pooling | 2 of 2 complete | ~9 min | ~5 min |

**Recent Trend:**
- Last 5 plans: ~4 min, ~2 min, ~4 min, ~3 min, ~5 min
- Trend: 08-01 fast — plan specified exact code, 2 auto-fixes (bb8 parking_lot disabled, is_valid signature)

*Updated after each plan completion*
| Phase 06 P01 | 3 | 2 tasks | 1 files |
| Phase 07-reconnection P01 | 3 | 2 tasks | 3 files |
| Phase 08-connection-pooling P01 | 5 | 2 tasks | 3 files |
| Phase 08-connection-pooling P02 | 4 | 2 tasks | 2 files |

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
- [04-01]: parse_resp_code() strips bracket code, keeps text after ] — consistent with ServerError stripping -ERR
- [04-01]: [AUTH] RESP-CODE maps to AuthFailed (not a new variant) — merges into existing semantic error
- [04-01]: Unknown RESP-CODEs fall through to ServerError with full text preserved
- [04-01]: apop() returns ServerError immediately if no timestamp in greeting — no silent fallback
- [04-01]: apop() map_err promotes ServerError->AuthFailed; RESP-CODE variants pass through unchanged
- [04-01]: apop() deprecated with note referencing login() as the preferred alternative
- [04-02]: Pop3ClientBuilder::new(hostname) only — no convenience ::plain()/::tls() constructors; chain is fluent enough
- [04-02]: Internal TlsMode and AuthMode enums are private — only builder methods are public API surface
- [04-02]: Last-wins semantics for both TLS mode and auth mode — consistent, predictable behavior
- [04-02]: Builder derives Debug and Clone — Clone required by Phase 8 connection pooling (bb8 reuse)
- [05-01]: InnerStream promoted to pub(crate) to satisfy private_interfaces lint — reader/writer pub(crate) fields require their type to also be pub(crate); no external API change
- [05-01]: BufWriter default buffer size (8 KB) used — commands are ~15 bytes each, no tuning needed
- [05-01]: ConnectionClosed wording is "connection closed" matching legacy EOF error message
- [05-01]: is_closed is private bool on Transport, set by read_line() on EOF and by set_closed() after quit()
- [05-02]: CAPA probe runs after every successful login() and apop() — automatic, errors silently suppressed; not all POP3 servers support CAPA (RFC 1939)
- [05-02]: PIPELINE_WINDOW = 4 — conservative window preventing TCP send-buffer deadlock with large RETR responses
- [05-02]: Per-item results Vec<Result<T>> for batch methods — individual -ERR does not abort batch; I/O errors fill remaining with ConnectionClosed
- [05-02]: retr_many_pipelined/dele_many_pipelined are private — only retr_many/dele_many are public API; read_retr_response() private helper avoids parsing duplication
- [Phase 06-01]: No require_auth() calls in wrapper methods — auth enforced by delegated uidl() and retr()
- [Phase 06-01]: fetch_unseen fails fast on first retr() error — no partial results returned
- [Phase 06-01]: Section heading doc comment immediately precedes first method doc to satisfy clippy::empty_line_after_doc_comments
- [Phase 07-01]: ReconnectCallback type alias for Option<Box<dyn FnMut(u32, &Pop3Error) + Send>> avoids clippy::type_complexity on both builder and client structs
- [Phase 07-01]: is_retryable covers Io, ConnectionClosed, Timeout, SysTemp — AuthFailed excluded to prevent account lockout risk
- [Phase 07-01]: Credentials passed to connect() not stored on builder — minimises time credentials exist in plain-text builder fields
- [Phase 07-02]: fetch_unseen wraps Vec<(UidlEntry, Message)> — matches actual Pop3Client return type; plan template showed Vec<Message> which was incorrect
- [Phase 07-02]: pub(crate) build_authenticated_mock_client added to client.rs — cleanest way to share mock construction across modules without exposing private Pop3Client struct fields
- [Phase 07-02]: Full reconnect round-trip tests deferred to integration tests — do_reconnect() calls Pop3ClientBuilder::connect() requiring real TCP, not unit-testable
- [Phase 07-02]: Best-effort quit silently swallows retryable errors — only non-transient errors propagate from ReconnectingClient::quit()
- [Phase 08-connection-pooling]: bb8 added with default-features=false to disable parking_lot (dlltool.exe Windows GNU constraint)
- [Phase 08-connection-pooling]: Pop3PoolError separate from Pop3Error — pool-level errors conceptually distinct from POP3 protocol errors
- [Phase 08-connection-pooling]: is_valid() returns conn.noop() directly — avoids redundant async block and lifetime annotation issues
- [Phase 08-connection-pooling]: std::sync::RwLock used for pool registry (not tokio RwLock) — never held across await, allows synchronous add_account/remove_account
- [Phase 08-connection-pooling]: checkout() uses get_owned() returning PooledConnection<'static> — Arc cloned before lock released, allowing 'static lifetime without holding read lock across await
- [Phase 08-connection-pooling]: #[tokio::test] required for tests calling add_account — bb8 build_unchecked starts internal Tokio interval timer requiring runtime context

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

Last session: 2026-03-02
Stopped at: Completed 08-02-PLAN.md — Pop3Pool registry struct with PoolConfig, PooledConnection alias, checkout/add_account/remove_account/accounts API; RFC 1939 rustdoc; lib.rs re-exports behind pool feature; 25 pool tests passing. Phase 8 (Connection Pooling) complete. Ready for Phase 9 (MIME Integration).
Resume file: None
