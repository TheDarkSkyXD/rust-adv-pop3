# Roadmap: rust-pop3 v2.0 + v3.0

## Overview

This roadmap covers two milestones:

**v2.0 — Full Async Rewrite (Phases 1–4):** Transform the pop3 crate from a synchronous, panic-prone v1 library into a modern async Rust crate with full protocol coverage, proper error handling, dual TLS backends, and comprehensive tests. The rewrite is structured so that a test safety net is established before any structural refactoring begins, and the async core is solid before TLS complexity is layered on top. The four phases reflect hard dependency constraints, not arbitrary milestones.

**v3.0 — Advanced Features (Phases 5–9):** Add five production-grade capabilities on top of the stable v2.0 async client: RFC 2449 command pipelining for batch throughput, UIDL-based incremental sync for avoiding redundant downloads, automatic reconnection with exponential backoff for network resilience, connection pooling for multi-account scenarios, and optional MIME parsing integration. v3.0 is additive — new source files and composable wrappers that sit above the v2.0 `Client` struct without replacing it.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

**v2.0 Phases:**
- [x] **Phase 1: Foundation** - Fix known bugs, establish error handling, and build test infrastructure
- [x] **Phase 2: Async Core** - Migrate all I/O to async/await, port all v1 commands, set up CI
- [x] **Phase 3: TLS and Publish** - Add dual TLS backends, remaining commands, docs, and ship v2.0.0 (completed 2026-03-01)
- [x] **Phase 4: Protocol Extensions** - Add APOP, RESP-CODES, and builder pattern API (completed 2026-03-02)

**v3.0 Phases:**
- [x] **Phase 5: Pipelining** - Foundation modifications and RFC 2449 command pipelining with windowed send strategy (completed 2026-03-02)
- [x] **Phase 6: UIDL Caching** - UIDL cache and incremental sync helper for avoiding redundant message downloads (completed 2026-03-02)
- [ ] **Phase 7: Reconnection** - Automatic reconnection with exponential backoff and jitter via Decorator pattern
- [ ] **Phase 8: Connection Pooling** - bb8-backed connection pool for multi-account concurrent access
- [ ] **Phase 9: MIME Integration** - Optional MIME parsing via mail-parser behind a feature flag

## Phase Details

### Phase 1: Foundation
**Goal**: The library is a safe, testable base — all known bugs are fixed, all panics are eliminated, and a mock I/O test harness proves the fixes hold
**Depends on**: Nothing (first phase)
**Requirements**: FOUND-01, FOUND-02, FOUND-03, FOUND-04, FIX-01, FIX-02, FIX-03, FIX-04, QUAL-01
**Success Criteria** (what must be TRUE):
  1. `cargo build` succeeds on Rust 2021 edition with no `lazy_static` dependency
  2. Every public method returns `Result<T, Pop3Error>` — calling code can use `?` on all library calls without ever catching a panic
  3. `Pop3Error` enum variants cover I/O, TLS, protocol, authentication, and parse error categories
  4. Unit tests using `tokio_test::io::Builder` mock I/O confirm all four v1 bugs are fixed: RSET sends `RSET\r\n`, NOOP sends `NOOP\r\n`, `is_authenticated` is set only after `+OK` from PASS, and LIST parsing uses a dedicated regex
  5. All response parsing functions have at least one passing unit test exercising the happy path and one exercising an error path
**Plans:** 2 plans
- [x] 01-01-PLAN.md -- Mock transport infrastructure, AuthFailed error variant, and bug-proof tests (FIX-01..04)
- [x] 01-02-PLAN.md -- Complete mock I/O test coverage for all POP3 commands (QUAL-01)

### Phase 2: Async Core
**Goal**: All public API methods are async and work over a plain TCP connection — developers can connect, authenticate, and run every v1.0.6 command against a real server with no blocking calls
**Depends on**: Phase 1
**Requirements**: ASYNC-01, ASYNC-02, ASYNC-03, ASYNC-04, ASYNC-05, API-03, API-04, QUAL-03
**Success Criteria** (what must be TRUE):
  1. A caller can `await` any library method inside a `#[tokio::main]` function with no `block_on` wrappers
  2. All v1.0.6 commands (STAT, LIST, UIDL, RETR, DELE, NOOP, RSET, QUIT) work correctly, confirmed by async tests against tokio_test mock I/O covering happy paths and error paths
  3. Multi-line responses (RETR, LIST all, UIDL all) are correctly dot-unstuffed per RFC 1939
  4. Calling `quit()` consumes the client value — the compiler rejects any further method calls on the same variable after disconnect
  5. GitHub Actions CI passes `cargo test`, `cargo clippy -D warnings`, and `cargo fmt --check` on every push
**Plans:** 4/4 plans complete
- [x] 02-01-PLAN.md — Tokio dependencies, Pop3Error::Timeout, and async transport rewrite (ASYNC-02, ASYNC-03, ASYNC-05)
- [x] 02-02-PLAN.md — Async Pop3Client with SessionState, quit(self), and test migration (ASYNC-01, ASYNC-04, API-03, API-04)
- [x] 02-03-PLAN.md — GitHub Actions CI workflow (QUAL-03)
- [x] 02-04-PLAN.md — Phase 2 verification gap closure: examples/basic.rs fix validated, integration test criterion decision documented (QUAL-03)

### Phase 3: TLS and Publish
**Goal**: Library users can connect to port 995 TLS servers using either rustls or openssl, CAPA and TOP work, docs are complete, and v2.0.0 is published to crates.io
**Depends on**: Phase 2
**Requirements**: TLS-01, TLS-02, TLS-03, TLS-04, TLS-05, TLS-06, CMD-01, CMD-02, QUAL-02, QUAL-04, QUAL-05
**Success Criteria** (what must be TRUE):
  1. A user can connect to a port 995 POP3 server by selecting either `--features rustls-tls` or `--features openssl-tls` — only one backend is needed, and both produce identical public API behaviour
  2. Activating both TLS feature flags simultaneously produces a `compile_error!` at build time (not a runtime error)
  3. STARTTLS upgrades a plain TCP connection to TLS without data loss — the `BufReader` buffer is drained before stream upgrade
  4. `CAPA` and `TOP` commands work and are covered by integration tests against a mock server
  5. Every public type, function, and method has a rustdoc comment with a working doctest (`cargo test --doc` passes)
  6. The CI matrix tests both `rustls-tls` and `openssl-tls` feature flags
**Plans:** 4/4 plans complete
- [x] 03-01-PLAN.md — Feature flags, InnerStream enum, error refactor, rustls connect_tls, Pop3Client TLS methods (TLS-01, TLS-03, TLS-04)
- [x] 03-02-PLAN.md — OpenSSL backend, STARTTLS upgrade_in_place, stls() method (TLS-02, TLS-05, TLS-06)
- [x] 03-03-PLAN.md — Integration tests for full POP3 flows, TOP and CAPA coverage (CMD-01, CMD-02, QUAL-02)
- [x] 03-04-PLAN.md — Rustdoc with doctests, CI matrix, README, examples, publish prep (QUAL-04, QUAL-05)

### Phase 4: Protocol Extensions
**Goal**: The library supports APOP authentication, structured RESP-CODES error parsing, and a fluent builder API — rounding out the v2.x feature set
**Depends on**: Phase 3
**Requirements**: CMD-03, CMD-04, API-01, API-02
**Success Criteria** (what must be TRUE):
  1. A caller can authenticate using `Pop3ClientBuilder` with a fluent API — no direct TLS feature flag handling required in application code
  2. APOP authentication works and its rustdoc prominently documents the MD5 security caveat
  3. Server RESP-CODES (`[IN-USE]`, `[LOGIN-DELAY]`, etc.) are parsed into named `Pop3Error` enum variants rather than generic string errors
**Plans:** 2/2 plans complete
- [x] 04-01-PLAN.md — RESP-CODE parsing (MailboxInUse, LoginDelay, SysTemp, SysPerm variants), APOP auth with MD5, #[deprecated] + security warning (CMD-03, CMD-04)
- [x] 04-02-PLAN.md — Pop3ClientBuilder fluent API with smart port defaults, auto-auth, feature-gated TLS methods (API-01, API-02)

### Phase 5: Pipelining
**Goal**: Callers can send batches of POP3 commands without waiting for individual responses, unlocking high-throughput mail processing while automatically falling back to sequential mode on servers that do not support pipelining
**Depends on**: Phase 4
**Requirements**: PIPE-01, PIPE-02, PIPE-03, PIPE-04, PIPE-05
**Success Criteria** (what must be TRUE):
  1. A caller can call `retr_many(&[msg_ids])` or `dele_many(&[msg_ids])` and receive all responses in one batch without writing a send/receive loop
  2. After authentication, the client checks CAPA and enables pipelining automatically — the caller does not configure this
  3. Connecting to a mock server that does not advertise PIPELINING causes the batch methods to fall back to sequential execution silently — no error is raised
  4. A test confirms that sending all commands in an unbounded batch does not deadlock — the windowed send strategy keeps at most N commands outstanding at a time
  5. `src/client.rs` exposes `pub(crate)` reader/writer fields and an `is_closed() -> bool` method — `Pop3ClientBuilder` derives `Clone`
**Plans**: 2/2 plans complete
- [x] 05-01-PLAN.md — BufWriter on Transport writer, pub(crate) reader/writer/timeout, ConnectionClosed variant, is_closed()/set_closed() on Transport and Pop3Client
- [x] 05-02-PLAN.md — CAPA-based pipelining detection in login/apop, supports_pipelining() accessor, retr_many/dele_many batch methods with windowed send (PIPE-01..05)

### Phase 6: UIDL Caching
**Goal**: Callers can retrieve only messages they have not seen before, and the cache automatically prunes ghost entries so it never incorrectly marks a new message as already seen
**Depends on**: Phase 4 (can proceed in parallel with Phase 5 — no dependency on Phase 5 changes)
**Requirements**: CACHE-01, CACHE-02, CACHE-03
**Success Criteria** (what must be TRUE):
  1. A caller can pass a set of previously-seen UIDs to the client API and receive back only the UIDs not in that set — no manual set subtraction required
  2. Calling `fetch_new(seen)` returns full message content for only unseen messages in a single method call
  3. After connecting, the client reconciles the cached UID set against the server's current UIDL list — UIDs that no longer exist on the server are removed from the cache
**Plans**: TBD

### Phase 7: Reconnection
**Goal**: The client automatically recovers from dropped connections using exponential backoff with jitter, while making session-state loss explicit so callers cannot accidentally re-issue DELE marks against a fresh session
**Depends on**: Phase 5 (requires `ConnectionClosed` error variant and `Clone` on builder added in Phase 5)
**Requirements**: RECON-01, RECON-02, RECON-03, RECON-04
**Success Criteria** (what must be TRUE):
  1. A caller using `ReconnectingClient` continues working after a simulated I/O drop — the client reconnects and re-authenticates without any caller intervention
  2. An authentication failure during reconnection is propagated immediately to the caller — the client does not retry on `AuthFailed` errors
  3. After a reconnect, the caller receives an explicit signal that session state (including any pending DELE marks) has been lost — the API does not silently discard this information
  4. Consecutive reconnection attempts use increasing wait intervals with random jitter — two concurrent clients do not produce synchronized retry storms
**Plans:** 2 plans
- [ ] 07-01-PLAN.md — backon dependency, SessionReset ZST, ReconnectingClientBuilder, ReconnectingClient struct + retry infrastructure (RECON-01, RECON-02, RECON-04)
- [ ] 07-02-PLAN.md — All delegating command methods, lib.rs re-exports, tests covering all four RECON requirements (RECON-01, RECON-02, RECON-03, RECON-04)

### Phase 8: Connection Pooling
**Goal**: Callers can manage multiple POP3 accounts concurrently using a pool that enforces the RFC 1939 exclusive-lock constraint at the type level and in documentation
**Depends on**: Phase 5 (requires `is_closed()` and builder `Clone`), Phase 7 (requires stable error types and reconnect-aware connection lifecycle)
**Requirements**: POOL-01, POOL-02, POOL-03
**Success Criteria** (what must be TRUE):
  1. A caller can check out a live `Client` connection from the pool by account key and return it after use — the pool manages connection health via NOOP probes
  2. Attempting to create a second concurrent connection to the same mailbox via the pool blocks until the first connection is returned — no two connections to the same account are active simultaneously
  3. The `Pop3Pool` rustdoc prominently documents that POP3 forbids concurrent access to the same mailbox per RFC 1939, and explains the per-account exclusivity model
**Plans**: TBD

### Phase 9: MIME Integration
**Goal**: Callers can retrieve and parse a message's MIME structure in one call without manually passing raw RFC 5322 bytes to a third-party parser
**Depends on**: Phase 4 (requires `retr()` guaranteeing dot-unstuffed output — verifiable as a v2.0 postcondition; fully independent from Phases 5–8)
**Requirements**: MIME-01, MIME-02
**Success Criteria** (what must be TRUE):
  1. Calling `retr_parsed(msg_id)` with the `mime` feature flag enabled returns a structured `ParsedMessage` value — the caller never handles raw RFC 5322 bytes
  2. The `mime` feature flag is opt-in — projects that do not activate it compile with no dependency on `mail-parser` and no increase in binary size
**Plans**: TBD

## Progress

**Execution Order:**
v2.0 phases execute in numeric order: 1 → 2 → 3 → 4
v3.0 phases execute in order: 5 → 6 → 7 → 8 → 9 (Phase 6 can run in parallel with Phase 5; Phase 9 can be deferred to v3.1)

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation | 2/2 | Complete | 2026-03-01 |
| 2. Async Core | 4/4 | Complete (verified) | 2026-03-01 |
| 3. TLS and Publish | 4/4 | Complete   | 2026-03-01 |
| 4. Protocol Extensions | 2/2 | Complete | 2026-03-02 |
| 5. Pipelining | 2/2 | Complete | 2026-03-02 |
| 6. UIDL Caching | 1/1 | Complete   | 2026-03-02 |
| 7. Reconnection | 0/? | Not started | - |
| 8. Connection Pooling | 0/? | Not started | - |
| 9. MIME Integration | 0/? | Not started | - |
