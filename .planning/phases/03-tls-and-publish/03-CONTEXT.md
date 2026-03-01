# Phase 3: TLS and Publish - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Library users can connect to port 995 TLS servers using either rustls or openssl (feature-gated), upgrade plain connections via STARTTLS, CAPA and TOP work with integration tests, docs are complete, and CI tests both TLS backends. Publishing to crates.io is the final deliverable.

</domain>

<decisions>
## Implementation Decisions

### TLS Connection API
- Separate `connect_tls(addr, hostname, timeout)` method alongside existing `connect()`
- `hostname` parameter is `&str` — DNS name validation happens internally, returns `Pop3Error::InvalidDnsName` on bad input
- Uses system trust store via `rustls-native-certs` — no custom TLS config parameter (simplest API, covers 95% of use cases)
- Add `connect_tls_default(addr, hostname)` convenience method with 30s timeout — matches existing `connect_default()` symmetry

### Feature Flag Design
- `rustls-tls` is the default feature — `pop3 = "2.0"` gets TLS out of the box with no system deps
- `openssl-tls` is an opt-in feature requiring system OpenSSL
- `connect_tls()` and `stls()` are conditionally compiled — they only exist when a TLS feature is active (no dead code / misleading API)
- `compile_error!` when both `rustls-tls` and `openssl-tls` are active simultaneously
- Error type: single `Pop3Error::Tls(String)` variant — converts both rustls and openssl errors to string messages, no backend types leak into public API

### STARTTLS Upgrade Flow
- Explicit `stls()` method on `Pop3Client` — maps 1:1 to POP3 STLS command
- Pre-auth only per RFC 2595 — returns error if already authenticated (STLS valid only in AUTHORIZATION state)
- No SessionState change for TLS — TLS is a transport concern, not session state. Separate `is_encrypted()` method reports TLS status

### Documentation
- Full crate-level documentation with examples — user landing on docs.rs understands the library in 60 seconds
- One example file per connection mode: `examples/basic.rs` (plain), `examples/tls.rs` (TLS-on-connect), `examples/starttls.rs` (upgrade)
- Integration tests cover full connect-auth-command flows against a mock POP3 server (not just TLS-specific scenarios)
- Every public type, function, and method has a rustdoc comment with a working doctest

### Claude's Discretion
- OpenSSL async integration approach (tokio-openssl vs manual wrapping)
- STARTTLS hostname parameter design (pass explicitly to stls() vs store from connect)
- README update timing (now vs at publish time)
- Integration test mock server implementation details

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Transport` struct (transport.rs): Uses `Box<dyn AsyncRead>` / `Box<dyn AsyncWrite>` trait objects — TLS streams slot in without restructuring
- `Pop3Error::Tls` variant (error.rs): Already exists with `rustls::Error` — needs to change to `Tls(String)` for backend-agnostic errors
- `Pop3Error::InvalidDnsName` variant (error.rs): Already exists for hostname validation errors
- `connect_tls` stub (transport.rs:37-46): Placeholder returning "not yet supported" — ready to implement
- `TOP` command (client.rs:184-191): Already implemented, needs integration tests and doctest
- `CAPA` command (client.rs:194-198): Already implemented, needs integration tests and doctest
- Mock test infrastructure (client.rs): `build_test_client()` and `build_authenticated_test_client()` using `tokio_test::io::Builder`

### Established Patterns
- Transport uses `tokio::io::split()` to split streams into read/write halves — TLS must preserve this
- BufReader wraps the read half — STARTTLS must drain the BufReader buffer before replacing the stream
- All commands go through `send_and_check()` helper — single point for command dispatch
- `SessionState` enum tracks auth state — TLS status is orthogonal (transport concern)
- CRLF injection protection via `check_no_crlf()` on all user inputs

### Integration Points
- `lib.rs` re-exports: New public items (`connect_tls`, `stls`, `is_encrypted`) must be re-exported
- `Cargo.toml`: rustls/openssl deps move behind feature flags, `[features]` section added
- CI workflow (`.github/workflows/`): Matrix needs `rustls-tls` and `openssl-tls` runs
- `examples/`: New TLS and STARTTLS example files

</code_context>

<specifics>
## Specific Ideas

- Public API should feel identical regardless of TLS backend — same method names, same error types, same behavior
- The existing `rustls` hard dependency in Cargo.toml becomes feature-gated (breaking change for anyone depending on the rustls re-export)

</specifics>

<deferred>
## Deferred Ideas

- Custom TLS configuration (client certificates, custom root CAs) — future enhancement
- Pop3ClientBuilder fluent interface — Phase 4

</deferred>

---

*Phase: 03-tls-and-publish*
*Context gathered: 2026-03-01*
