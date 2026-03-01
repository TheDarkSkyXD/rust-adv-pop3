# Requirements: rust-pop3

**Defined:** 2026-03-01
**Core Value:** Provide a correct, async, production-quality POP3 client that handles errors gracefully instead of panicking

## v1 Requirements

Requirements for v2.0 release. Each maps to roadmap phases.

### Foundation

- [ ] **FOUND-01**: Library compiles with Rust 2021 edition
- [ ] **FOUND-02**: All regex patterns use `std::sync::LazyLock` instead of `lazy_static`
- [ ] **FOUND-03**: All public methods return `Result<T, Pop3Error>` instead of panicking
- [ ] **FOUND-04**: `Pop3Error` typed enum covers I/O, TLS, protocol, authentication, and parse errors

### Bug Fixes

- [ ] **FIX-01**: `rset()` sends `RSET\r\n` (not `RETR\r\n`)
- [ ] **FIX-02**: `noop()` sends `NOOP\r\n` (uppercase)
- [ ] **FIX-03**: `is_authenticated` is set only after server confirms PASS with `+OK`
- [ ] **FIX-04**: `parse_list_one()` uses a dedicated LIST regex, not `STAT_REGEX`

### Async I/O

- [ ] **ASYNC-01**: All public API methods are `async fn` using tokio runtime
- [ ] **ASYNC-02**: Reads use `tokio::io::BufReader` with line-oriented buffering
- [ ] **ASYNC-03**: Multi-line responses correctly handle RFC 1939 dot-unstuffing
- [ ] **ASYNC-04**: Session state tracked via `SessionState` enum (not a public bool field)
- [ ] **ASYNC-05**: Connection supports configurable read/write timeouts

### TLS

- [ ] **TLS-01**: User can connect via TLS-on-connect (port 995) using rustls backend
- [ ] **TLS-02**: User can connect via TLS-on-connect using openssl backend
- [ ] **TLS-03**: TLS backend selected via Cargo feature flags (`rustls-tls`, `openssl-tls`)
- [ ] **TLS-04**: Simultaneous activation of both TLS features produces a compile error
- [ ] **TLS-05**: User can upgrade a plain TCP connection to TLS via STARTTLS (STLS command)
- [ ] **TLS-06**: STARTTLS correctly drains BufReader before stream upgrade

### POP3 Commands

- [ ] **CMD-01**: User can retrieve message headers + N lines via TOP command
- [ ] **CMD-02**: User can query server capabilities via CAPA command
- [ ] **CMD-03**: User can authenticate via APOP (with documented MD5 security caveat)
- [ ] **CMD-04**: Server RESP-CODES are parsed into structured `Pop3Error` variants

### API Design

- [ ] **API-01**: `Pop3ClientBuilder` provides a fluent interface for connection configuration
- [ ] **API-02**: Builder hides TLS feature flag complexity from callers
- [ ] **API-03**: All public types derive `Debug`
- [ ] **API-04**: `Client` consumes `self` on `quit()` preventing use-after-disconnect

### Quality

- [ ] **QUAL-01**: Unit tests cover all response parsing functions via mock I/O
- [ ] **QUAL-02**: Integration tests cover connect, auth, and command flows via mock POP3 server
- [ ] **QUAL-03**: GitHub Actions CI runs tests, clippy, and format checks
- [ ] **QUAL-04**: CI matrix tests both `rustls-tls` and `openssl-tls` feature flags
- [ ] **QUAL-05**: All public items have rustdoc with working doctests

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Extended Protocol

- **EXT-01**: SASL PLAIN authentication mechanism
- **EXT-02**: POP3 command pipelining (RFC 2449)
- **EXT-03**: OAUTH2/XOAUTH2 authentication

### Extended Features

- **FEAT-01**: Automatic reconnection on connection drop
- **FEAT-02**: Connection pooling
- **FEAT-03**: Optional synchronous API wrapper

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| IMAP support | Different protocol, different crate |
| Email/MIME parsing | Use `mailparse` or similar crate; out of scope for transport library |
| Connection pooling | POP3 is inherently single-connection (servers return `[IN-USE]`) |
| Synchronous API | Defeats the purpose of the async rewrite; callers use `block_on` |
| SASL negotiation | Adds RFC compliance surface but minimal practical demand for v2.0 |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| FOUND-01 | — | Pending |
| FOUND-02 | — | Pending |
| FOUND-03 | — | Pending |
| FOUND-04 | — | Pending |
| FIX-01 | — | Pending |
| FIX-02 | — | Pending |
| FIX-03 | — | Pending |
| FIX-04 | — | Pending |
| ASYNC-01 | — | Pending |
| ASYNC-02 | — | Pending |
| ASYNC-03 | — | Pending |
| ASYNC-04 | — | Pending |
| ASYNC-05 | — | Pending |
| TLS-01 | — | Pending |
| TLS-02 | — | Pending |
| TLS-03 | — | Pending |
| TLS-04 | — | Pending |
| TLS-05 | — | Pending |
| TLS-06 | — | Pending |
| CMD-01 | — | Pending |
| CMD-02 | — | Pending |
| CMD-03 | — | Pending |
| CMD-04 | — | Pending |
| API-01 | — | Pending |
| API-02 | — | Pending |
| API-03 | — | Pending |
| API-04 | — | Pending |
| QUAL-01 | — | Pending |
| QUAL-02 | — | Pending |
| QUAL-03 | — | Pending |
| QUAL-04 | — | Pending |
| QUAL-05 | — | Pending |

**Coverage:**
- v1 requirements: 32 total
- Mapped to phases: 0
- Unmapped: 32 ⚠️

---
*Requirements defined: 2026-03-01*
*Last updated: 2026-03-01 after initial definition*
