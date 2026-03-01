# Requirements: rust-pop3

**Defined:** 2026-03-01
**Core Value:** Provide a correct, async, production-quality POP3 client that handles errors gracefully instead of panicking

## v2.0 Requirements

Requirements for v2.0 release. Each maps to roadmap phases.

### Foundation

- [x] **FOUND-01**: Library compiles with Rust 2021 edition
- [x] **FOUND-02**: All regex patterns use `std::sync::LazyLock` instead of `lazy_static`
- [x] **FOUND-03**: All public methods return `Result<T, Pop3Error>` instead of panicking
- [x] **FOUND-04**: `Pop3Error` typed enum covers I/O, TLS, protocol, authentication, and parse errors

### Bug Fixes

- [x] **FIX-01**: `rset()` sends `RSET\r\n` (not `RETR\r\n`)
- [x] **FIX-02**: `noop()` sends `NOOP\r\n` (uppercase)
- [x] **FIX-03**: `is_authenticated` is set only after server confirms PASS with `+OK`
- [x] **FIX-04**: `parse_list_one()` uses a dedicated LIST regex, not `STAT_REGEX`

### Async I/O

- [x] **ASYNC-01**: All public API methods are `async fn` using tokio runtime
- [x] **ASYNC-02**: Reads use `tokio::io::BufReader` with line-oriented buffering
- [x] **ASYNC-03**: Multi-line responses correctly handle RFC 1939 dot-unstuffing
- [x] **ASYNC-04**: Session state tracked via `SessionState` enum (not a public bool field)
- [x] **ASYNC-05**: Connection supports configurable read/write timeouts

### TLS

- [x] **TLS-01**: User can connect via TLS-on-connect (port 995) using rustls backend
- [x] **TLS-02**: User can connect via TLS-on-connect using openssl backend
- [x] **TLS-03**: TLS backend selected via Cargo feature flags (`rustls-tls`, `openssl-tls`)
- [x] **TLS-04**: Simultaneous activation of both TLS features produces a compile error
- [x] **TLS-05**: User can upgrade a plain TCP connection to TLS via STARTTLS (STLS command)
- [x] **TLS-06**: STARTTLS correctly drains BufReader before stream upgrade

### POP3 Commands

- [x] **CMD-01**: User can retrieve message headers + N lines via TOP command
- [x] **CMD-02**: User can query server capabilities via CAPA command
- [ ] **CMD-03**: User can authenticate via APOP (with documented MD5 security caveat)
- [ ] **CMD-04**: Server RESP-CODES are parsed into structured `Pop3Error` variants

### API Design

- [ ] **API-01**: `Pop3ClientBuilder` provides a fluent interface for connection configuration
- [ ] **API-02**: Builder hides TLS feature flag complexity from callers
- [x] **API-03**: All public types derive `Debug`
- [x] **API-04**: `Client` consumes `self` on `quit()` preventing use-after-disconnect

### Quality

- [x] **QUAL-01**: Unit tests cover all response parsing functions via mock I/O
- [x] **QUAL-02**: Integration tests cover connect, auth, and command flows via mock POP3 server
- [x] **QUAL-03**: GitHub Actions CI runs tests, clippy, and format checks
- [x] **QUAL-04**: CI matrix tests both `rustls-tls` and `openssl-tls` feature flags
- [x] **QUAL-05**: All public items have rustdoc with working doctests

## v3.0 Requirements

Requirements for v3.0 release. Each maps to roadmap phases 5+.

### Pipelining

- [ ] **PIPE-01**: Client can send multiple POP3 commands without waiting for each response when server advertises PIPELINING (RFC 2449)
- [ ] **PIPE-02**: Client automatically detects pipelining support via CAPA after authentication
- [ ] **PIPE-03**: Client falls back to sequential mode when server does not advertise PIPELINING
- [ ] **PIPE-04**: Pipelined commands use a windowed send strategy to prevent TCP send-buffer deadlock
- [ ] **PIPE-05**: Client provides batch methods (`retr_many`, `dele_many`) that pipeline automatically

### UIDL Caching

- [ ] **CACHE-01**: Client provides an API to filter the UIDL list against a set of previously-seen UIDs
- [ ] **CACHE-02**: Client provides a `fetch_new()` convenience method returning only unseen messages
- [ ] **CACHE-03**: UIDL cache reconciliation prunes ghost entries (UIDs no longer on server) on each connect

### Reconnection

- [ ] **RECON-01**: Client provides automatic reconnection with exponential backoff on connection drop
- [ ] **RECON-02**: Reconnection retries only on I/O errors — authentication failures propagate immediately
- [ ] **RECON-03**: Reconnection explicitly surfaces session-state loss (DELE marks are not preserved) to caller
- [ ] **RECON-04**: Backoff uses jitter to prevent thundering herd

### Connection Pooling

- [ ] **POOL-01**: Client provides a connection pool for multi-account scenarios via `bb8`
- [ ] **POOL-02**: Pool enforces max 1 connection per mailbox (RFC 1939 exclusive lock)
- [ ] **POOL-03**: Pool documentation prominently warns that POP3 forbids concurrent access to the same mailbox

### MIME Integration

- [ ] **MIME-01**: Client provides `retr_parsed()` method behind a `mime` feature flag
- [ ] **MIME-02**: MIME integration uses `mail-parser` crate (zero external deps, RFC 5322 + MIME conformant)

## v4.0 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Extended Protocol

- **EXT-01**: SASL PLAIN authentication mechanism
- **EXT-03**: OAUTH2/XOAUTH2 authentication

### Extended Features

- **FEAT-03**: Optional synchronous API wrapper

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| IMAP support | Different protocol, different crate |
| Synchronous API | Defeats the purpose of the async rewrite; callers use `block_on` |
| SASL negotiation | Adds RFC compliance surface but minimal practical demand |
| Same-mailbox concurrent connections | RFC 1939 mandates exclusive mailbox lock; protocol-impossible |
| Built-in UIDL persistence to disk | Transport library provides the data; caller owns persistence strategy |
| Transparent auto-reconnect with silent DELE re-issue | Creates invisible data inconsistency; caller must own reconnect decisions |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| FOUND-01 | Phase 1 | Complete (01-02) |
| FOUND-02 | Phase 1 | Complete (01-02) |
| FOUND-03 | Phase 1 | Complete (01-02) |
| FOUND-04 | Phase 1 | Complete (01-01) |
| FIX-01 | Phase 1 | Complete (01-01) |
| FIX-02 | Phase 1 | Complete (01-01) |
| FIX-03 | Phase 1 | Complete (01-01) |
| FIX-04 | Phase 1 | Complete (01-01) |
| QUAL-01 | Phase 1 | Complete (01-02) |
| ASYNC-01 | Phase 2 | Complete |
| ASYNC-02 | Phase 2 | Complete |
| ASYNC-03 | Phase 2 | Complete |
| ASYNC-04 | Phase 2 | Complete |
| ASYNC-05 | Phase 2 | Complete |
| API-03 | Phase 2 | Complete |
| API-04 | Phase 2 | Complete |
| QUAL-03 | Phase 2 | Complete |
| TLS-01 | Phase 3 | Complete (03-01) |
| TLS-02 | Phase 3 | Complete (03-02) |
| TLS-03 | Phase 3 | Complete (03-01) |
| TLS-04 | Phase 3 | Complete (03-01) |
| TLS-05 | Phase 3 | Complete (03-02) |
| TLS-06 | Phase 3 | Complete (03-02) |
| CMD-01 | Phase 3 | Complete (03-03) |
| CMD-02 | Phase 3 | Complete (03-03) |
| QUAL-02 | Phase 3 | Complete (03-03) |
| QUAL-04 | Phase 3 | Complete |
| QUAL-05 | Phase 3 | Complete |
| CMD-03 | Phase 4 | Pending |
| CMD-04 | Phase 4 | Pending |
| API-01 | Phase 4 | Pending |
| API-02 | Phase 4 | Pending |
| PIPE-01 | Phase 5 | Pending |
| PIPE-02 | Phase 5 | Pending |
| PIPE-03 | Phase 5 | Pending |
| PIPE-04 | Phase 5 | Pending |
| PIPE-05 | Phase 5 | Pending |
| CACHE-01 | Phase 6 | Pending |
| CACHE-02 | Phase 6 | Pending |
| CACHE-03 | Phase 6 | Pending |
| RECON-01 | Phase 7 | Pending |
| RECON-02 | Phase 7 | Pending |
| RECON-03 | Phase 7 | Pending |
| RECON-04 | Phase 7 | Pending |
| POOL-01 | Phase 8 | Pending |
| POOL-02 | Phase 8 | Pending |
| POOL-03 | Phase 8 | Pending |
| MIME-01 | Phase 9 | Pending |
| MIME-02 | Phase 9 | Pending |

**Coverage:**
- v2.0 requirements: 32 total, mapped to phases 1-4: 32
- v3.0 requirements: 17 total, mapped to phases 5-9: 17
- Unmapped: 0

---
*Requirements defined: 2026-03-01*
*Last updated: 2026-03-01 after Phase 2 plan revision — QUAL-04 moved from Phase 2 to Phase 3 (TLS feature flag matrix requires TLS code)*
