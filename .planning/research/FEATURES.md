# Feature Research

**Domain:** Async POP3 client library (Rust crate)
**Researched:** 2026-03-01
**Confidence:** HIGH (protocol specs from RFCs; tokio/TLS patterns from official docs and verified community sources)

---

## Feature Landscape

### Table Stakes (Users Expect These)

These are non-negotiable for v2.0. Any Rust developer who picks up this crate expects all of them. Missing any one makes the crate feel half-finished compared to the existing v1.0.6 baseline or competing crates like `async-pop`.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Async/await API (tokio) | The Rust async ecosystem default; synchronous network I/O is a blocker for production use in any async runtime | HIGH | Full rewrite; all public methods become `async fn` returning `Result<T, Pop3Error>`. Split stream with `into_split()` or use `BufReader` wrapper. |
| Result-based error handling (no panics) | Library code that panics is unusable in production; callers need to handle all failure modes gracefully | MEDIUM | Replace all `panic!` and `unwrap()` in public paths with `thiserror`-derived enum. 13+ call sites in current code. |
| Typed error enum | Callers must be able to match on specific error kinds (auth failure vs. network error vs. parse error) | MEDIUM | `Pop3Error` enum with variants: `Io(io::Error)`, `Tls(...)`, `ProtocolError(String)`, `AuthFailed`, `ParseError`, etc. `thiserror` is the standard tool (already in Cargo.lock). |
| Plain TCP connection | Required for port 110 without TLS, internal networks, testing | LOW | `TcpStream::connect` + wrap in `BufReader`. Trivial with tokio. |
| TLS connection (SSL on connect) | Port 995 (POP3S) is the predominant secure POP3 deployment; users expect it to just work | MEDIUM | Wrapped behind feature flags. See TLS feature entry. |
| USER/PASS authentication | Universal; every POP3 server supports it; already in v1.0.6 | LOW | Port from sync; fix `is_authenticated` flag timing bug (must be set after `+OK` from PASS, not after write). |
| STAT command | Already in v1.0.6; users depend on it | LOW | Port from sync; return typed `StatResponse { message_count: u32, size_octets: u64 }`. |
| LIST command (single + all) | Already in v1.0.6; fundamental mailbox enumeration | LOW | Port from sync; fix the wrong-regex bug in `parse_list_one` (currently reuses STAT_REGEX). |
| UIDL command (single + all) | Already in v1.0.6; required for sync-state tracking across sessions | LOW | Port from sync. |
| RETR command | Already in v1.0.6; retrieving messages is the core operation | LOW | Port from sync; multiline response with dot-unstuffing. |
| DELE command | Already in v1.0.6 | LOW | Port from sync. |
| NOOP command | Already in v1.0.6; fix lowercase bug (`noop\r\n` → `NOOP\r\n`) | LOW | Trivial fix. |
| RSET command | Already in v1.0.6; fix wrong-command bug (currently sends `RETR` instead of `RSET`) | LOW | Trivial fix. |
| QUIT command | Already in v1.0.6 | LOW | Port from sync. |
| Buffered I/O | One-byte-at-a-time reads cause extreme syscall overhead; any production caller will hit this | MEDIUM | Wrap `TcpStream`/TLS stream with `tokio::io::BufReader`. Use `AsyncBufReadExt::read_line()` for line-at-a-time reads. Eliminates the byte loop in `read_response`. |
| Dot-unstuffing on multiline responses | RFC 1939 mandatory: lines beginning with `.` are byte-stuffed by servers; failure to unstuff corrupts message bodies | MEDIUM | Every multiline response (RETR, LIST all, UIDL all, TOP) must strip leading `.` from stuffed lines before returning. Termination is `CRLF.CRLF` (`\r\n.\r\n`). |
| Private `is_authenticated` state | Public mutable field allows callers to bypass auth guards; any library user will rightly call this a security flaw | LOW | Make field private; enforce state through method signatures or internal guards only. |
| Rustdoc with examples | Rust crate conventions; crates.io surfaces doc coverage; `async-pop` scores only 31.25% documentation | MEDIUM | `//!` crate-level doc, `///` on every public item, `# Examples` sections with `#[tokio::main]` examples in doc comments. |
| GitHub Actions CI | Travis CI is non-functional; no CI means no confidence in contributions | LOW | `dtolnay/rust-toolchain@stable` + `cargo test`, `cargo clippy`, `cargo fmt --check`. Separate jobs. Test both feature flag combinations. |

---

### Differentiators (Competitive Advantage)

These features go beyond what `async-pop` and other competing crates provide. They align directly with the project's core value: "correct, async, production-quality POP3 client."

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Dual TLS backends via feature flags (openssl + rustls) | Most async Rust crates force one TLS backend; developers deploying on Linux distros need openssl for system cert stores, while pure-Rust builds (musl, CI, WASM-adjacent) need rustls. Offering both with feature flags is rare in the POP3 space. | HIGH | Feature flags: `tls-openssl` (tokio-native-tls + native-tls + openssl) and `tls-rustls` (tokio-rustls + rustls + rustls-native-certs). Must compile-error if both enabled simultaneously (`compile_error!`). Default: neither (plain only). Follows the lettre/reqwest pattern. |
| CAPA command (RFC 2449) | Allows callers to introspect server capabilities before issuing commands; needed for robust STARTTLS negotiation and graceful degradation | MEDIUM | Available in both AUTHORIZATION and TRANSACTION states per RFC 2449. Returns a `HashSet<String>` or typed enum of capabilities. Parse multi-line `+OK` response terminated by `.\r\n`. Capabilities include: TOP, USER, SASL, RESP-CODES, LOGIN-DELAY, PIPELINING, EXPIRE, UIDL, IMPLEMENTATION. |
| TOP command (RFC 1939 optional) | Required for email preview without full download; widely supported but omitted from v1.0.6 and from `async-pop` | LOW | Syntax: `TOP <msg> <n>` where n is number of body lines. Returns headers + blank separator + n body lines as multi-line response. Server must indicate TOP support in CAPA. |
| APOP authentication (RFC 1939) | Challenge-response auth using MD5; avoids sending plaintext password; present in RFC 1939 core spec but absent from v1.0.6 | MEDIUM | Server announces APOP by including a timestamp in the greeting banner (format: `<...@...>`). Client computes `MD5(timestamp + shared_secret)` and sends `APOP user digest`. Security note: MD5 is cryptographically broken for this purpose — APOP is a legacy mechanism, not recommended for new deployments. Implement with a security warning in documentation. Dependency: parse greeting timestamp; use `md5` crate (pure Rust, no C deps). |
| STARTTLS / STLS command (RFC 2595) | Upgrade plain TCP connection on port 110 to TLS in-place; required for servers that only offer opportunistic TLS rather than implicit TLS on port 995 | HIGH | Protocol: send `STLS\r\n`, receive `+OK`, then perform TLS handshake over the same TCP connection. Only valid in AUTHORIZATION state. After upgrade, client MUST re-issue CAPA (per RFC 2595: discard cached capability info to prevent MITM). Requires TLS backend to support stream upgrade (tokio-rustls `TlsConnector::connect()` on an existing stream; tokio-native-tls `TlsConnector::connect()` similarly). This is architecturally complex: the stream type changes mid-connection, requiring enum dispatch or Box<dyn ...> for the underlying I/O. |
| RESP-CODES support (RFC 2449) | Machine-parseable error classification; allows callers to distinguish `[IN-USE]` (mailbox locked) from `[SYS/TEMP]` (temporary server error) from `[AUTH]` (bad credentials) without string parsing | MEDIUM | Servers announce `RESP-CODES` in CAPA. Error responses containing `[CODE]` at start of human-readable text are extended response codes. Parse the bracketed token and expose it as a structured error variant. Key standard codes: `[IN-USE]`, `[LOGIN-DELAY]`, `[SYS/TEMP]`, `[SYS/PERM]`, `[AUTH]`. |
| Unit tests with mock I/O | Zero tests exist in v1.0.6; all bugs went undetected. Mock-based tests make the library trustworthy for contributors and give a safety net for the rewrite. | MEDIUM | Use `tokio_test::io::Builder` to feed scripted POP3 server responses and verify command bytes sent. Requires generic `AsyncRead + AsyncWrite + Unpin` bounds on internal client so real streams and mocks are interchangeable. `#[tokio::test]` macro for test runtime. |
| Idiomatic builder pattern for connection | Discoverability; callers shouldn't need to know which TLS wrapper to construct — a `Pop3Client::builder()` chains configuration and connects | MEDIUM | `Pop3ClientBuilder::new("pop.example.com", 995).tls_rustls().connect().await` vs. `Pop3ClientBuilder::new("pop.example.com", 110).plain().connect().await`. Hides the feature flag complexity from call sites. |

---

### Anti-Features (Commonly Requested, Often Problematic)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Synchronous (blocking) API wrapper | Existing users of v1.0.6 want zero migration friction | Adding a sync wrapper doubles the surface area, complicates the internal design, and defeats the purpose of the async rewrite. The tokio ecosystem already provides `block_on` for callers who need sync. | Document a migration guide. Callers use `tokio::runtime::Runtime::block_on()` or `#[tokio::main]`. |
| OAUTH2 / XOAUTH2 authentication | Modern email providers (Gmail, Outlook) require OAuth for new app registrations | XOAUTH2 requires external OAuth token refresh flows that are completely outside the POP3 protocol scope. Adding it entangles the library with HTTP client dependencies and per-provider token APIs. | Explicitly out of scope for v2.0. Document that callers who need OAUTH2 should obtain the token externally and pass it as a credential string if the server's SASL mechanism accepts it. |
| Email parsing / MIME decoding | `RETR` returns raw message bytes; callers expect structured email objects | Email parsing is a domain-specific problem solved by dedicated crates (`mailparse`, `mail-parser`). Bundling it increases binary size, adds dependencies, and competes with crates that do it better. | Return raw `Vec<String>` or `String` from RETR/TOP. Recommend `mailparse` in documentation. |
| Connection pooling | High-throughput POP3 clients want to reuse connections | POP3 is a sequential, stateful protocol. Each session locks the mailbox (servers return `[IN-USE]` to concurrent connections). Pooling is incorrect for POP3 — it's not HTTP. | Document that POP3 sessions are inherently single-connection. Application-level retry logic is the correct approach. |
| Automatic reconnect / retry | Network resilience | Hidden reconnection changes session state invisibly; deleted messages might not be deleted, or new messages might appear. The caller must own reconnect logic to reason about session consistency. | Expose connection errors transparently. Document that callers should reconnect and re-authenticate if desired. |
| SASL authentication (full) | RFC 5034 defines SASL for POP3; SASL PLAIN is widely supported | Full SASL adds a significant state machine for negotiating mechanisms. Most deployments only need PLAIN (which is equivalent to USER/PASS over TLS). SASL GSSAPI/Kerberos is enterprise-only and wildly out of scope. | Implement SASL PLAIN as a thin wrapper if needed in a future milestone. Leave other mechanisms for community contribution. |
| Implicit TLS by default | Security-first defaults are good | Silently switching port 110 to TLS would break existing configurations. Feature flags gate TLS entirely — callers must opt in explicitly. | Require callers to choose their connection type explicitly via builder method. |

---

## Feature Dependencies

```
[Async API (tokio)]
    └──required by──> ALL commands (STAT, LIST, UIDL, RETR, DELE, TOP, CAPA, etc.)
    └──required by──> [Buffered I/O]
    └──required by──> [Mock I/O testing]
    └──required by──> [STARTTLS]

[Typed error enum (thiserror)]
    └──required by──> ALL public methods
    └──enhanced by──> [RESP-CODES] (adds structured error codes as enum variants)

[Plain TCP connection]
    └──required by──> [STARTTLS] (STARTTLS upgrades a plain connection to TLS)
    └──required by──> [TLS on connect] (wraps a TCP stream before connect)

[TLS on connect (feature flag)]
    └──conflicts with──> [STARTTLS using different backend] (same stream, one approach)
    └──requires one of──> [tls-openssl feature] or [tls-rustls feature]

[CAPA command]
    └──required by──> [STARTTLS] (must check CAPA for STLS before issuing it)
    └──enhances──> [RESP-CODES] (CAPA advertises RESP-CODES capability)
    └──enhances──> [TOP] (CAPA advertises TOP capability)

[STARTTLS / STLS]
    └──requires──> [Plain TCP connection] (upgrade in place)
    └──requires──> [CAPA command] (check for STLS before issuing)
    └──requires──> [TLS backend feature flag] (needs TLS connector to do handshake)
    └──requires──> [Typed error enum] (STLS failure must propagate, not panic)

[APOP authentication]
    └──requires──> [Parse greeting banner] (timestamp extraction from +OK greeting)
    └──requires──> [md5 crate] (new dependency; pure Rust)
    └──conflicts with──> [USER/PASS] (use one or the other per session)

[Buffered I/O]
    └──required by──> [Dot-unstuffing] (line-at-a-time reads make unstuffing clean)
    └──required by──> [RETR], [TOP], [LIST all], [UIDL all] (all multiline responses)

[Dot-unstuffing]
    └──required by──> [RETR], [TOP], [LIST all], [UIDL all], [CAPA]

[Mock I/O testing]
    └──requires──> [Generic AsyncRead+AsyncWrite bounds on client internals]
    └──requires──> [tokio_test dev-dependency]

[Builder pattern]
    └──enhances──> [TLS feature flags] (hides connector construction from callers)
    └──enhances──> [Plain TCP] (unified entry point regardless of TLS choice)

[GitHub Actions CI]
    └──requires──> [Unit tests] (nothing to run otherwise)
    └──requires──> [Feature flags] (CI matrix must test both tls-openssl and tls-rustls)
```

### Dependency Notes

- **STARTTLS requires CAPA:** RFC 2595 states clients MUST check that the server supports STLS before issuing it, and MUST re-issue CAPA after TLS upgrade to protect against MITM. CAPA therefore cannot be deferred to a later phase if STARTTLS is in scope.
- **STARTTLS is architecturally complex:** The stream changes type mid-connection. The client struct must use an internal enum (`enum Pop3Stream { Plain(BufReader<TcpStream>), Tls(BufReader<TlsStream<TcpStream>>) }`) or box the stream behind `Box<dyn AsyncRead + AsyncWrite + Unpin + Send>`. The boxed trait object approach is simpler to implement but incurs a heap allocation and dynamic dispatch per read/write.
- **TLS feature flags conflict:** `tls-openssl` and `tls-rustls` should not both be active. Use `compile_error!` in a `cfg` check to give a clear build-time message. This is the same pattern used by reqwest, lettre, and rust-s3.
- **APOP depends on MD5:** The `md5` crate on crates.io provides MD5 digest computation in pure Rust. It is a new dependency not currently in Cargo.toml. APOP is a legacy mechanism — its MD5 use is cryptographically broken (practical key recovery attacks are documented). Implement but document the security caveat prominently.
- **Buffered I/O is prerequisite for correct dot-unstuffing:** The current single-byte read loop cannot cleanly detect `\r\n.\r\n`. `AsyncBufReadExt::read_line()` returns complete lines, making the termination and unstuffing logic straightforward.
- **Generic I/O bounds enable mocking:** If the client struct holds `Box<dyn AsyncRead + AsyncWrite + Unpin + Send>` or is generic over `<S: AsyncRead + AsyncWrite + Unpin>`, then `tokio_test::io::Builder` can supply a mock stream in tests. If it holds a concrete `TcpStream`, unit testing without a live server is impossible.

---

## MVP Definition

This is a subsequent milestone — not a greenfield MVP. v1.0.6 is already published. The "MVP" here is the minimum scope that justifies publishing v2.0.0 as a semver-major release with a credible upgrade path.

### Launch With (v2.0)

- [x] Async/await API using tokio — core reason for the major version bump
- [x] Typed error enum, no panics — production-usable for the first time
- [x] Buffered I/O + correct dot-unstuffing — correctness and performance baseline
- [x] All v1.0.6 commands ported (STAT, LIST, UIDL, RETR, DELE, NOOP, RSET, QUIT) — backwards feature parity
- [x] All known v1.0.6 bugs fixed (rset, noop, is_authenticated, parse_list_one)
- [x] Plain TCP and TLS-on-connect (at least one TLS backend, behind feature flag)
- [x] TOP command — widely expected, was optional in RFC 1939 but now table stakes
- [x] CAPA command — enables capability detection; prerequisite for STARTTLS
- [x] Unit tests with mock I/O — CI would be meaningless without them
- [x] GitHub Actions CI (cargo test, clippy, fmt) — replaces broken Travis CI
- [x] Rustdoc with examples — crates.io display and user discoverability

### Add After Core Is Stable (v2.x)

- [ ] Second TLS backend (if only one ships in v2.0) — once CI matrix is validated with first
- [ ] STARTTLS / STLS — architecturally complex; safe to defer if port 110 + STARTTLS is rare in target deployments
- [ ] RESP-CODES — polishes error handling but not blocking; add when error type design is locked
- [ ] APOP — legacy; document security caveat; implement once error enum is final

### Future Consideration (v3+)

- [ ] SASL PLAIN — thin wrapper over USER/PASS semantics; adds RFC compliance surface
- [ ] Pipelining (RFC 2449 PIPELINING capability) — requires sending multiple commands without awaiting each; complicates state machine significantly
- [ ] Full SASL negotiation — enterprise-only; out of scope unless driven by user demand

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Async/await API | HIGH | HIGH | P1 |
| Typed error enum (thiserror) | HIGH | MEDIUM | P1 |
| Buffered I/O + dot-unstuffing | HIGH | MEDIUM | P1 |
| Port all v1.0.6 commands | HIGH | MEDIUM | P1 |
| Fix known v1.0.6 bugs | HIGH | LOW | P1 |
| Plain TCP + TLS-on-connect | HIGH | MEDIUM | P1 |
| Unit tests (mock I/O) | HIGH | MEDIUM | P1 |
| GitHub Actions CI | HIGH | LOW | P1 |
| Rustdoc with examples | HIGH | MEDIUM | P1 |
| TOP command | HIGH | LOW | P1 |
| CAPA command | HIGH | MEDIUM | P1 |
| Dual TLS backends (both flags) | MEDIUM | HIGH | P2 |
| STARTTLS / STLS | MEDIUM | HIGH | P2 |
| RESP-CODES | MEDIUM | MEDIUM | P2 |
| APOP authentication | LOW | MEDIUM | P2 |
| Builder pattern | MEDIUM | LOW | P2 |
| SASL PLAIN | LOW | MEDIUM | P3 |
| Pipelining | LOW | HIGH | P3 |

---

## Competitor Feature Analysis

| Feature | async-pop (crates.io) | v1.0.6 (this crate) | v2.0 Target |
|---------|----------------------|---------------------|-------------|
| Async/await | Yes (tokio + async-std) | No (sync) | Yes (tokio) |
| TLS backend | async-native-tls only | openssl only | openssl + rustls (feature flags) |
| CAPA | Unclear (31% doc coverage) | No | Yes |
| TOP | Unclear | No | Yes |
| APOP | Unclear | No | Yes (with caveat) |
| STARTTLS | Unclear | No | Yes |
| RESP-CODES | Unclear | No | Yes |
| Typed errors | Has error module | No (panics) | Yes (thiserror) |
| Dot-unstuffing | Unknown | Partially (termination logic fragile) | Yes (correct) |
| Unit tests | Unknown | Zero | Yes (mock I/O) |
| CI | Unknown | Broken (Travis) | Yes (GitHub Actions) |
| Rustdoc | 31% coverage | Minimal | Full coverage |
| Builder pattern | No | No | Yes |

---

## Sources

- RFC 1939 (POP3 core): https://www.rfc-editor.org/rfc/rfc1939.txt — HIGH confidence (authoritative)
- RFC 2449 (POP3 Extension Mechanism, CAPA, RESP-CODES): https://www.rfc-editor.org/rfc/rfc2449.html — HIGH confidence (authoritative)
- RFC 2595 (STLS/STARTTLS for POP3): https://www.rfc-editor.org/rfc/rfc2595.html — HIGH confidence (authoritative)
- tokio BufReader + AsyncBufReadExt: https://docs.rs/tokio/latest/tokio/io/struct.BufReader.html — HIGH confidence (official docs)
- tokio_test::io::Builder for mock testing: https://tokio.rs/tokio/topics/testing — HIGH confidence (official)
- tokio-rustls (async TLS for tokio): https://github.com/rustls/tokio-rustls — HIGH confidence (official rustls org)
- tokio-native-tls: https://docs.rs/tokio-native-tls — HIGH confidence (official tokio-rs)
- thiserror for error enums: https://github.com/dtolnay/thiserror — HIGH confidence (widely used, official)
- async-pop competitor analysis: https://docs.rs/async-pop/latest/async_pop/ — MEDIUM confidence (low doc coverage; features inferred)
- Mutually exclusive Cargo feature flags: https://doc.rust-lang.org/cargo/reference/features.html — HIGH confidence (official Cargo docs)
- APOP security status (MD5 broken): https://who.rocq.inria.fr/Gaetan.Leurent/files/APOP_IJACT.pdf — HIGH confidence (peer-reviewed academic paper)
- GitHub Actions Rust CI pattern: https://dev.to/bampeers/rust-ci-with-github-actions-1ne9 — MEDIUM confidence (community; verified pattern matches dtolnay/rust-toolchain)
- openssl Windows CI build issues: https://github.com/sfackler/rust-openssl/issues/2197 — MEDIUM confidence (GitHub issue tracker)
- rustls vs openssl 2024: https://users.rust-lang.org/t/rustls-vs-openssl-2024/111754 — MEDIUM confidence (community forum)
- POP3 dot-stuffing (RFC 1939 section on multi-line responses): confirmed in RFC 1939 spec — HIGH confidence

---

*Feature research for: Rust async POP3 client library (v2.0 milestone)*
*Researched: 2026-03-01*
