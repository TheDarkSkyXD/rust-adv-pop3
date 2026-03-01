# Roadmap: rust-pop3 v2.0

## Overview

Transform the pop3 crate from a synchronous, panic-prone v1 library into a modern async Rust crate with full protocol coverage, proper error handling, dual TLS backends, and comprehensive tests. The rewrite is structured so that a test safety net is established before any structural refactoring begins, and the async core is solid before TLS complexity is layered on top. The four phases reflect hard dependency constraints, not arbitrary milestones.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Foundation** - Fix known bugs, establish error handling, and build test infrastructure
- [ ] **Phase 2: Async Core** - Migrate all I/O to async/await, port all v1 commands, set up CI
- [ ] **Phase 3: TLS and Publish** - Add dual TLS backends, remaining commands, docs, and ship v2.0.0
- [ ] **Phase 4: Protocol Extensions** - Add APOP, RESP-CODES, and builder pattern API

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
**Plans**: TBD

### Phase 2: Async Core
**Goal**: All public API methods are async and work over a plain TCP connection — developers can connect, authenticate, and run every v1.0.6 command against a real server with no blocking calls
**Depends on**: Phase 1
**Requirements**: ASYNC-01, ASYNC-02, ASYNC-03, ASYNC-04, ASYNC-05, API-03, API-04, QUAL-03, QUAL-04
**Success Criteria** (what must be TRUE):
  1. A caller can `await` any library method inside a `#[tokio::main]` function with no `block_on` wrappers
  2. All v1.0.6 commands (STAT, LIST, UIDL, RETR, DELE, NOOP, RSET, QUIT) work correctly over a plain TCP connection confirmed by integration tests against a mock server
  3. Multi-line responses (RETR, LIST all, UIDL all) are correctly dot-unstuffed per RFC 1939
  4. Calling `quit()` consumes the client value — the compiler rejects any further method calls on the same variable after disconnect
  5. GitHub Actions CI passes `cargo test`, `cargo clippy -D warnings`, and `cargo fmt --check` on every push
**Plans**: TBD

### Phase 3: TLS and Publish
**Goal**: Library users can connect to port 995 TLS servers using either rustls or openssl, CAPA and TOP work, docs are complete, and v2.0.0 is published to crates.io
**Depends on**: Phase 2
**Requirements**: TLS-01, TLS-02, TLS-03, TLS-04, TLS-05, TLS-06, CMD-01, CMD-02, QUAL-02, QUAL-05
**Success Criteria** (what must be TRUE):
  1. A user can connect to a port 995 POP3 server by selecting either `--features rustls-tls` or `--features openssl-tls` — only one backend is needed, and both produce identical public API behaviour
  2. Activating both TLS feature flags simultaneously produces a `compile_error!` at build time (not a runtime error)
  3. STARTTLS upgrades a plain TCP connection to TLS without data loss — the `BufReader` buffer is drained before stream upgrade
  4. `CAPA` and `TOP` commands work and are covered by integration tests against a mock server
  5. Every public type, function, and method has a rustdoc comment with a working doctest (`cargo test --doc` passes)
  6. The CI matrix tests both `rustls-tls` and `openssl-tls` feature flags
**Plans**: TBD

### Phase 4: Protocol Extensions
**Goal**: The library supports APOP authentication, structured RESP-CODES error parsing, and a fluent builder API — rounding out the v2.x feature set
**Depends on**: Phase 3
**Requirements**: CMD-03, CMD-04, API-01, API-02
**Success Criteria** (what must be TRUE):
  1. A caller can authenticate using `Pop3ClientBuilder` with a fluent API — no direct TLS feature flag handling required in application code
  2. APOP authentication works and its rustdoc prominently documents the MD5 security caveat
  3. Server RESP-CODES (`[IN-USE]`, `[LOGIN-DELAY]`, etc.) are parsed into named `Pop3Error` enum variants rather than generic string errors
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation | 0/? | Not started | - |
| 2. Async Core | 0/? | Not started | - |
| 3. TLS and Publish | 0/? | Not started | - |
| 4. Protocol Extensions | 0/? | Not started | - |
