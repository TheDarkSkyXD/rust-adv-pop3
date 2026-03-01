# Phase 1: Foundation - Research

**Researched:** 2026-03-01
**Domain:** Rust synchronous-to-async migration, typed error handling, mock I/O testing for network protocols
**Confidence:** HIGH

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| FOUND-01 | Library compiles with Rust 2021 edition | Already satisfied — `Cargo.toml` sets `edition = "2021"`. No work needed. |
| FOUND-02 | All regex patterns use `std::sync::LazyLock` instead of `lazy_static` | Already satisfied — v2 rewrite eliminated all regex; parsers use whitespace splitting. No `lazy_static` dep exists. |
| FOUND-03 | All public methods return `Result<T, Pop3Error>` instead of panicking | Already satisfied — all public API returns `Result<T, Pop3Error>`. `unwrap()`/`panic!` appear only in test assertions, not production paths. |
| FOUND-04 | `Pop3Error` typed enum covers I/O, TLS, protocol, authentication, and parse errors | Already satisfied — `Pop3Error` in `src/error.rs` has `Io`, `Tls`, `InvalidDnsName`, `ServerError`, `Parse`, `NotAuthenticated`, `InvalidInput` variants via `thiserror 2`. |
| FIX-01 | `rset()` sends `RSET\r\n` (not `RETR\r\n`) | Already satisfied — `client.rs` line 148 sends `"RSET"` correctly. |
| FIX-02 | `noop()` sends `NOOP\r\n` (uppercase) | Already satisfied — `client.rs` line 155 sends `"NOOP"` (uppercase). |
| FIX-03 | `is_authenticated` is set only after server confirms PASS with `+OK` | Already satisfied — `login()` calls `send_and_check()` for USER then PASS; only sets `self.authenticated = true` on line 79 after both succeed. |
| FIX-04 | `parse_list_one()` uses a dedicated LIST regex, not `STAT_REGEX` | Already satisfied — `parse_list_single()` delegates to `parse_list_entry()` which uses whitespace splitting, completely independent of stat parsing. |
| QUAL-01 | Unit tests cover all response parsing functions via mock I/O | NOT YET SATISFIED. `src/response.rs` has 20 unit tests covering all parse functions (happy path + error path), and `src/transport.rs` has 1 unit test for dot-unstuffing logic. MISSING: end-to-end mock I/O tests that exercise the full send-command/read-response cycle against a scripted fake server to prove the four bugs are fixed. The transport is currently not generic — it hardcodes `TcpStream`. Making `Transport` generic over `R: BufRead + Write` enables `BufReader<Cursor<Vec<u8>>>` as a test mock without any async dependency. |
</phase_requirements>

---

## Summary

The v2.0-async-rewrite branch has already implemented the vast majority of Phase 1's requirements. The multi-module rewrite in `src/` uses `thiserror 2`, `edition = "2021"`, returns `Result<T, Pop3Error>` everywhere, eliminates all `lazy_static` usage (replaced by direct whitespace parsing — no regex at all), and fixes all four known v1 bugs (rset, noop, auth flag timing, list parsing). The build passes cleanly: `cargo build`, `cargo test` (23 tests), and `cargo clippy` all succeed with zero warnings.

The single unmet requirement is QUAL-01: there are no end-to-end mock I/O tests that drive the full send-command/read-response cycle via a scripted fake server. The existing 23 tests are all unit tests against pure parsing functions (`response.rs`) or inline logic (`transport.rs`). They do not exercise the `Transport` struct's `send_command()` + `read_line()` + `read_multiline()` pipeline with controllable server responses. Proving the four bugs are fixed requires scripted tests that replay real server interactions.

The concrete gap: `Transport` is not generic — it hardcodes `TcpStream`. The fix is to introduce a type parameter `R: BufRead + Write` on `Transport` (or add a `new_from_reader_writer` constructor used only in tests). For Phase 1 this uses `std::io::Cursor` as the mock (sync I/O, no async dependencies). Phase 2 will replace the transport with async I/O and `tokio_test::io::Builder` will become applicable then. The requirements mention `tokio_test::io::Builder` — this applies to the async tests in Phase 2; for Phase 1's sync transport, `Cursor`-based tests are correct and sufficient.

**Primary recommendation:** Add `pub(crate) fn new_from_reader` constructor to `Transport` that accepts `BufReader<R>` for any `R: Read + Write`, write QUAL-01 tests in a `#[cfg(test)]` module using `std::io::Cursor`, and confirm all four bug-fix scenarios via scripted server interactions. Total new code estimate: ~150 lines.

---

## Standard Stack

### Core (Already in Cargo.toml)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `thiserror` | 2.0.18 | Derive macro for typed `Pop3Error` enum | Gold standard for library error types in Rust ecosystem. `#[derive(thiserror::Error)]` generates `Display` + `From` impls. v2 is the current major (released 2024). |
| `rustls` | 0.23 | TLS-on-connect for port 995 (current only backend) | Pure-Rust TLS; no C deps; default features disabled allows fine-grained feature control. Already in `Cargo.toml`. |
| `rustls-native-certs` | 0.8 | Load system CA certs for TLS verification | Required companion to rustls for validating server certificates against OS trust store. Already in `Cargo.toml`. |

### Testing Stack (Phase 1 Additions)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `std::io::Cursor` | std | In-memory mock stream for sync I/O tests | Phase 1 only — transport is synchronous; `Cursor<Vec<u8>>` implements `Read + Write + BufRead`. No dependency. |
| `tokio-test` | 0.4 | Mock async I/O via `io::Builder` | Phase 2+ when transport becomes async. NOT needed for Phase 1. Add to `[dev-dependencies]` in Phase 2. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `thiserror 2` | `anyhow` | `anyhow` is for applications (opaque errors). Libraries MUST use `thiserror` to expose typed error variants callers can match on. Already decided. |
| `std::io::Cursor` for Phase 1 tests | `tokio_test::io::Builder` | `tokio_test::io::Builder` implements `AsyncRead + AsyncWrite` — requires async transport. The current transport is synchronous. `Cursor` is correct for Phase 1. Phase 2 switches to `tokio_test`. |
| `rustls 0.23` | `openssl 0.10` | `openssl` requires a C toolchain. Both are gated behind feature flags in Phase 3. For Phase 1, `rustls` is the only TLS backend (already wired). |

**Installation (Phase 1 only adds to `[dev-dependencies]`):**
```bash
# No new dependencies for Phase 1 production code.
# Test mock uses std::io::Cursor (no new dep needed).
# Cargo.toml dev-dependencies to add for Phase 2 (not Phase 1):
# tokio-test = "0.4"
```

---

## Architecture Patterns

### Current Project Structure

```
src/
├── lib.rs          # Crate root — re-exports public types
├── client.rs       # Pop3Client struct + all POP3 command methods
├── error.rs        # Pop3Error enum (thiserror derive)
├── response.rs     # Pure-function response parsers
├── transport.rs    # Transport struct wrapping BufReader<TcpStream or TLS>
└── types.rs        # Stat, ListEntry, UidlEntry, Message, Capability types

examples/
└── basic.rs        # Basic connect/auth/stat/quit example
```

### Pattern 1: Generic Transport for Testability

**What:** Make `Transport` generic over its inner reader/writer type so unit tests can substitute `BufReader<Cursor<Vec<u8>>>` instead of a live `TcpStream`.

**When to use:** Any time a transport-layer struct embeds a concrete I/O type that prevents substitution in tests.

**Why it matters for Phase 1:** The current `Transport` has no test-injectable constructor. Adding one (or making the type generic) lets tests script exact server responses and verify the client sends the right commands.

**Minimal approach (preferred for Phase 1):** Add a `pub(crate)` test constructor that accepts a pre-built `BufReader<R>` and a `Box<dyn Write>`. This avoids exposing generics on the public API.

**Example (Phase 1 test mock setup):**
```rust
// In transport.rs — add test-only constructor
#[cfg(test)]
impl Transport {
    pub(crate) fn from_bufread_write<R: Read + 'static>(
        reader: BufReader<R>,
        writer: Box<dyn Write>,
    ) -> Self {
        // Store reader and writer using a trait-object or enum variant for test use
    }
}
```

**Alternative — simpler for Phase 1:** Test the response parsing functions directly (they're pure functions) and add integration-style tests in `client.rs` using a custom `Transport` variant that wraps `Cursor`. Since the four bugs are all in command sending (wrong command text) or parsing (wrong regex/logic), the parser unit tests already cover FOUND-02/FIX-04 and the command text is verifiable through the Transport's write path.

**Recommended approach for Phase 1 QUAL-01:** Use the simplest approach that works within the sync architecture:

```rust
// In transport.rs — a test-only stream variant using Cursor
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // Build a fake server interaction: write server responses into the read side,
    // capture what the client sends via a Vec<u8> on the write side.
    fn mock_transport(server_responses: &[u8]) -> (Transport, Vec<u8>) {
        // See Code Examples section for full pattern
    }
}
```

### Pattern 2: thiserror Error Enum Structure

**What:** `Pop3Error` is a typed enum with `#[derive(thiserror::Error)]`. Each variant represents a distinct failure category. Callers match on variants for programmatic error handling.

**Current coverage:**
- `Io(#[from] io::Error)` — network/socket errors
- `Tls(#[from] rustls::Error)` — TLS handshake failures
- `InvalidDnsName(String)` — hostname validation
- `ServerError(String)` — server returned `-ERR`
- `Parse(String)` — response format unexpected
- `NotAuthenticated` — command requires auth
- `InvalidInput` — CRLF injection attempt

**Phase 1 evaluation:** This enum already satisfies FOUND-04. No changes needed.

**Example (verified in `src/error.rs`):**
```rust
// Source: src/error.rs (confirmed as of 2026-03-01)
#[derive(Debug, thiserror::Error)]
pub enum Pop3Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),
    #[error("server error: {0}")]
    ServerError(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("not authenticated")]
    NotAuthenticated,
    #[error("invalid input: CRLF injection detected")]
    InvalidInput,
}
```

### Pattern 3: Pure-Function Parsers for Full Testability

**What:** All response parsing in `src/response.rs` is implemented as pure free functions (`parse_stat`, `parse_list_entry`, `parse_uidl_entry`, etc.) that take `&str` and return `Result<T, Pop3Error>`. No `self` parameter, no I/O.

**Why this matters:** Pure parsing functions can be unit-tested with string literals — no mock I/O required. The existing 20 tests in `response.rs` exercise all parsers (both happy path and error path), satisfying QUAL-01 for the parsing layer.

**What's still missing for QUAL-01:** Tests that prove the correct *command text* is sent (e.g., `rset()` sends `RSET\r\n` not `RETR\r\n`). These require inspecting what the client writes to the wire, which requires a writable mock.

### Anti-Patterns to Avoid

- **Mutable global state for parsers:** The v1 `STAT_REGEX` was a global static regex shared across `STAT` and `LIST` parsing — caused FIX-04. The v2 whitespace-split approach is correct; do not introduce regex again.
- **Returning `Option` instead of `Result`:** All parser functions must return `Result<T, Pop3Error>` (with descriptive error text), not `Option<T>`. Callers need to distinguish "missing" from "malformed".
- **Panicking in production code:** No `unwrap()`, `expect()`, or `panic!()` in `src/` outside of `#[cfg(test)]` blocks. The current code is clean — maintain this.
- **Test assertions that panic silently:** Prefer `assert_eq!(result.unwrap(), expected)` in tests that should succeed, and `result.unwrap_err()` in tests that should fail. The current test patterns in `response.rs` are correct.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Error types with Display + From impls | Manual `impl Display for Pop3Error` + manual `impl From<io::Error> for Pop3Error` | `thiserror 2` with `#[derive(thiserror::Error)]` | Gets Display, From, and Error trait correct; handles `#[source]` and `#[from]` chain automatically; eliminates ~50 lines of boilerplate per error type |
| In-memory I/O mock for sync transport tests | Custom `MockStream` struct | `std::io::Cursor<Vec<u8>>` | `Cursor<Vec<u8>>` implements `Read + Write + BufRead + Seek` — it is the stdlib-blessed in-memory byte buffer for testing I/O without network |
| Whitespace-split parser | Regex patterns | `str::split_whitespace()` + `str::parse::<u32>()` | POP3 response fields are space-separated ASCII integers; a regex adds a compile-time dependency and runtime overhead for a pattern that `split_whitespace()` handles correctly and safely |
| CRLF termination | Manual byte manipulation | String concatenation `format!("{cmd}\r\n")` or `write_all(b"\r\n")` | The two-step write in `send_command()` is idiomatic and avoids format! allocation on the hot path |

**Key insight:** The v2 rewrite already avoided all these hand-roll traps. The only hand-roll risk remaining in Phase 1 is test infrastructure — use `Cursor` rather than writing a custom `MockStream`.

---

## Common Pitfalls

### Pitfall 1: Confusing "tokio_test::io::Builder required" with "must use async in Phase 1"

**What goes wrong:** The QUAL-01 description says "unit tests using `tokio_test::io::Builder` mock I/O" — a planner interprets this as requiring `tokio_test` to be added in Phase 1 and the transport to be async.

**Why it happens:** `tokio_test::io::Builder` implements `AsyncRead + AsyncWrite`. The current transport is synchronous (`std::io::BufReader<TcpStream>`). Adding `tokio_test` to Phase 1 would require the transport to accept async types — which is Phase 2's job.

**How to avoid:** Phase 1 tests use `std::io::Cursor<Vec<u8>>` as the mock, which is the sync equivalent. The requirement text anticipates the final async architecture. The intent is "prove the bugs are fixed via scripted I/O" — not "use this specific crate right now." `tokio_test` is added to `[dev-dependencies]` in Phase 2 when the transport becomes async.

**Warning signs:** If a Phase 1 task adds `tokio-test` to `Cargo.toml` and changes `send_command` to `async fn`, that task has gone out of scope into Phase 2.

### Pitfall 2: The Transport's write path is not tested by any existing test

**What goes wrong:** The existing 23 tests all call parser functions directly or test dot-unstuffing logic inline. None of them call `Transport::send_command()`. A bug in `send_command()` (e.g., missing `\r\n`, wrong encoding) would pass all current tests.

**Why it happens:** The pure-function parser tests are easy to write and provide great coverage. The command-send path requires a writable mock and is overlooked.

**How to avoid:** QUAL-01 tests must use a writable `Cursor<Vec<u8>>` on the write side and read back what was written to confirm the exact bytes sent. For example, after calling `rset()`, the write buffer should contain exactly `b"RSET\r\n"`.

**Warning signs:** If Phase 1 tasks only add more parser tests without any tests that inspect `Transport::send_command()` output, QUAL-01 is not satisfied.

### Pitfall 3: Making Transport Generic Breaks the Public API

**What goes wrong:** To make Transport testable, a developer adds a type parameter `Transport<R: BufRead + Write>` which propagates to `Pop3Client<R>` which propagates to the public `lib.rs` exports. Now callers must specify the type parameter.

**Why it happens:** Naively making a struct generic infects its owner struct, which infects its owner, until it reaches the public API.

**How to avoid:** Limit the generic constructor to `#[cfg(test)]`. The production constructors (`connect_plain`, `connect_tls`) continue to return concrete `Transport` (wrapping enum variants). In tests, use a trait-object trick or a dedicated test-only variant of the `Stream` enum:

```rust
// In transport.rs — test-only addition, never in public API
#[cfg(test)]
enum Stream {
    Plain(BufReader<TcpStream>),
    Tls(Box<BufReader<StreamOwned<ClientConnection, TcpStream>>>),
    Mock { reader: BufReader<Cursor<Vec<u8>>>, writer: Vec<u8> },  // test-only variant
}
```

This approach requires no type parameter, no public API change, and no `Box<dyn ...>` overhead in production.

**Warning signs:** `Pop3Client` gains a type parameter visible in `lib.rs` exports.

### Pitfall 4: Asserting `is_authenticated = true` Without Scripting the Server Response

**What goes wrong:** A test for FIX-03 calls `login()` but the mock server doesn't return `+OK` for PASS. The test expects `is_authenticated` to be true but the call returns an error. The test accidentally passes because the assertion is never reached.

**Why it happens:** The mock is set up for the happy path but the `send_and_check()` path requires a `+OK` response to proceed past the server error check.

**How to avoid:** Always script both USER and PASS responses in the mock. For FIX-03, test two scenarios:
1. Happy path: USER gets `+OK`, PASS gets `+OK` → `is_authenticated` becomes true
2. FIX-03 bug scenario: USER gets `+OK`, PASS gets `-ERR` → `is_authenticated` stays false, method returns `Err(Pop3Error::ServerError(...))`

---

## Code Examples

Verified patterns from the current codebase and standard Rust practices:

### Mock Transport Setup for Phase 1 Tests

```rust
// Source: Standard Rust std::io::Cursor pattern (stdlib, no new dependency)
// Place in: src/transport.rs #[cfg(test)] block

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    /// Build a fake TCP exchange: server_script contains the bytes the "server" will return,
    /// returns (Transport, written_bytes_capture) so tests can inspect what was sent.
    ///
    /// NOTE: The Stream enum needs a Mock variant (cfg(test) only) for this to compile.
    /// Mock variant: Stream::Mock { reader: BufReader<Cursor<Vec<u8>>>, written: Vec<u8> }
    fn make_mock_transport(server_bytes: &[u8]) -> Transport {
        let reader = BufReader::new(Cursor::new(server_bytes.to_vec()));
        let writer = Vec::new();
        Transport {
            stream: Stream::Mock { reader, writer },
        }
    }

    #[test]
    fn rset_sends_correct_command() {
        // Script: server immediately accepts connection (no greeting needed here),
        // then returns +OK to RSET.
        let mut t = make_mock_transport(b"+OK reset\r\n");
        t.send_command("RSET").unwrap();
        // Read back what was written:
        match &t.stream {
            Stream::Mock { writer, .. } => {
                assert_eq!(writer, b"RSET\r\n");
            }
            _ => panic!("expected Mock stream"),
        }
    }

    #[test]
    fn noop_sends_uppercase_command() {
        let mut t = make_mock_transport(b"+OK\r\n");
        t.send_command("NOOP").unwrap();
        match &t.stream {
            Stream::Mock { writer, .. } => {
                assert_eq!(writer, b"NOOP\r\n");
            }
            _ => panic!("expected Mock stream"),
        }
    }
}
```

### Full Bug-Proof Test: FIX-03 (auth flag timing)

```rust
// Source: Derived from current src/client.rs login() implementation
// Tests that is_authenticated is NOT set until PASS returns +OK

#[cfg(test)]
mod login_tests {
    use super::*;

    #[test]
    fn authenticated_only_after_pass_ok() {
        // Script: USER gets +OK, PASS gets +OK
        // Build client with scripted transport
        let mut client = build_test_client(b"+OK\r\n+OK logged in\r\n");
        client.login("user", "pass").unwrap();
        assert!(client.authenticated, "should be authenticated after +OK PASS");
    }

    #[test]
    fn not_authenticated_when_pass_fails() {
        // Script: USER gets +OK, PASS gets -ERR
        let mut client = build_test_client(b"+OK\r\n-ERR invalid password\r\n");
        let result = client.login("user", "wrongpass");
        assert!(result.is_err(), "login should fail");
        assert!(!client.authenticated, "should NOT be authenticated after -ERR PASS");
    }
}
```

### Full Bug-Proof Test: FIX-04 (list parsing uses dedicated parser)

```rust
// Source: Derived from src/response.rs parse_list_single/parse_list_entry

#[test]
fn list_single_parses_correctly() {
    // The LIST single response returns "msg_num size" in status text
    // Historically this used STAT_REGEX which captured "count total_size" — wrong fields
    let entry = parse_list_single("1 1234").unwrap();
    assert_eq!(entry.message_id, 1);
    assert_eq!(entry.size, 1234);
}

#[test]
fn list_single_rejects_stat_format() {
    // "2 messages 5 octets" is STAT format — list parser must not accept it
    // (If FIX-04 regression: this would accidentally parse "2" and fail on "messages")
    let result = parse_list_single("2 messages 5 octets");
    assert!(result.is_err(), "LIST parser must reject STAT-formatted input");
}
```

### thiserror Error Enum (Current, Confirmed)

```rust
// Source: src/error.rs (confirmed as of 2026-03-01)
use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Pop3Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),

    #[error("invalid DNS name: {0}")]
    InvalidDnsName(String),

    #[error("server error: {0}")]
    ServerError(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("not authenticated")]
    NotAuthenticated,

    #[error("invalid input: CRLF injection detected")]
    InvalidInput,
}

pub type Result<T> = std::result::Result<T, Pop3Error>;
```

---

## State of the Art

| Old Approach (v1) | Current Approach (v2) | When Changed | Impact |
|---|---|---|---|
| `lazy_static! { static ref STAT_REGEX: Regex = ... }` | `str::split_whitespace()` + `parse::<u32>()` | v2 rewrite | No `regex` dep, no `lazy_static` dep, no `LazyLock` needed — whitespace splitting is simpler and sufficient |
| `unwrap()` / `panic!()` in production methods | All public methods return `Result<T, Pop3Error>` | v2 rewrite | Callers can use `?` operator; no unexpected panics in library code |
| Concrete `String`-based error returns | Typed `Pop3Error` enum with `thiserror` derive | v2 rewrite | Callers can pattern-match on error variants |
| `is_authenticated = true` set before `PASS` response received | `is_authenticated = true` set only after both `send_and_check("USER")` and `send_and_check("PASS")` succeed | v2 rewrite | Authentication state accurately reflects server confirmation |
| `rset()` sends `"RETR\r\n"` (bug) | `rset()` sends `"RSET\r\n"` | v2 rewrite | Correct RFC 1939 reset behavior |
| `noop()` sends `"noop\r\n"` (lowercase, bug) | `noop()` sends `"NOOP\r\n"` | v2 rewrite | RFC 1939 requires uppercase POP3 commands |
| `parse_list_one()` uses `STAT_REGEX` (wrong fields) | `parse_list_single()` uses `parse_list_entry()` (dedicated, correct) | v2 rewrite | LIST responses now correctly parsed |
| Single flat `src/pop3.rs` (537 lines) | Multi-module `src/` (client, error, response, transport, types) | v2 rewrite | Each concern is testable in isolation |
| Zero tests | 23 unit tests across `client.rs`, `response.rs`, `transport.rs` | v2 rewrite | Parsing and I/O logic covered |

**Still outdated (not fixed yet — the Phase 1 gap):**

- **End-to-end mock I/O tests:** There are no tests that script a server interaction and verify what the client sends. This is the only remaining Phase 1 gap.

---

## Open Questions

1. **How to add a Mock stream variant to Transport without breaking encapsulation**
   - What we know: `Stream` is a private enum in `transport.rs`. Adding a `Mock` variant behind `#[cfg(test)]` is a clean pattern that does not leak into the public API.
   - What's unclear: Whether the `impl Transport` methods (`send_command`, `read_line`, `read_multiline`) need match arm additions for the `Mock` variant — they do, but only in `#[cfg(test)]` builds.
   - Recommendation: Add `Stream::Mock { reader: BufReader<Cursor<Vec<u8>>>, writer: Vec<u8> }` as a `#[cfg(test)]`-gated variant. Add matching arms to all three methods (also `#[cfg(test)]`-gated). This is a ~40-line addition.

2. **Should `tokio-test` be added to `[dev-dependencies]` in Phase 1?**
   - What we know: `tokio_test::io::Builder` implements `AsyncRead + AsyncWrite`. The current transport is synchronous. Using it in Phase 1 would require the transport to be async.
   - What's unclear: Whether the requirements intend Phase 1 tests to be async-first (anticipating Phase 2) or to work with the current sync transport.
   - Recommendation: Do NOT add `tokio-test` in Phase 1. Use `std::io::Cursor` (zero dependency, stdlib). Phase 2 adds `tokio-test` when the transport becomes async. This keeps Phase 1 focused and avoids premature async introduction.

3. **Does the `lib.rs` doctest need updating?**
   - What we know: The current `lib.rs` doctest shows a sync API (`fn main() -> pop3::Result<()>`) with `client.login()?`. This passes currently as a `compile` test.
   - What's unclear: Whether this doctest should show the v2 async API (which Phase 2 will build) or continue showing the sync API for Phase 1.
   - Recommendation: Leave the doctest as-is for Phase 1. It accurately reflects the current API. Phase 2 will update it to `async fn main()`.

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness (`#[test]` + `cargo test`) |
| Config file | None — Rust built-in, no config file needed |
| Quick run command | `cargo test` |
| Full suite command | `cargo test -- --include-ignored` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| FOUND-01 | Library compiles with edition 2021 | build | `cargo build` | ✅ — build passes now |
| FOUND-02 | No `lazy_static`, no regex | static | `grep -r "lazy_static" src/` returns nothing | ✅ — no regex in codebase |
| FOUND-03 | All public methods return `Result<T, Pop3Error>` | compile | `cargo build` with no `#[allow(unused_must_use)]` | ✅ — enforced by type system |
| FOUND-04 | `Pop3Error` covers I/O, TLS, protocol, auth, parse | unit | `cargo test response::tests::test_error_display` | ✅ — `src/response.rs` line 287 |
| FIX-01 | `rset()` sends `RSET\r\n` | unit (mock I/O) | `cargo test rset_sends_correct_command` | ❌ Wave 0 — needs mock transport |
| FIX-02 | `noop()` sends `NOOP\r\n` | unit (mock I/O) | `cargo test noop_sends_uppercase_command` | ❌ Wave 0 — needs mock transport |
| FIX-03 | `is_authenticated` set after PASS `+OK` only | unit (mock I/O) | `cargo test not_authenticated_when_pass_fails` | ❌ Wave 0 — needs mock transport |
| FIX-04 | `parse_list_one()` uses dedicated LIST parser | unit | `cargo test response::tests::test_parse_list_single` | ✅ — passes now |
| QUAL-01 | All response parsing functions have happy+error tests | unit | `cargo test response::tests` | ✅ (20 tests cover all parsers) + ❌ (no mock I/O command tests) |

### Sampling Rate

- **Per task commit:** `cargo test`
- **Per wave merge:** `cargo test && cargo clippy -D warnings && cargo fmt --check`
- **Phase gate:** All 23 existing tests pass + new mock I/O tests (FIX-01, FIX-02, FIX-03) pass + `cargo clippy -D warnings` clean

### Wave 0 Gaps

- [ ] `src/transport.rs` — Add `Stream::Mock` variant (`#[cfg(test)]`) with `reader` and `writer` fields, plus matching arms in `send_command`, `read_line`, `read_multiline`
- [ ] `src/transport.rs` — Add `make_mock_transport(server_bytes: &[u8]) -> Transport` helper (in `#[cfg(test)]` block)
- [ ] `src/client.rs` or `src/transport.rs` — Add mock transport tests proving FIX-01 (`rset` → `RSET\r\n`), FIX-02 (`noop` → `NOOP\r\n`), FIX-03 (auth flag after PASS `+OK`)

*(FIX-04 / QUAL-01 parsing tests already exist in `src/response.rs` — 20 tests pass)*

---

## Sources

### Primary (HIGH confidence)

- `src/error.rs` (read 2026-03-01) — confirmed `Pop3Error` variants, `thiserror 2` usage
- `src/client.rs` (read 2026-03-01) — confirmed FIX-01/02/03 all implemented; confirmed all public methods return `Result`
- `src/response.rs` (read 2026-03-01) — confirmed 20 unit tests covering all parsers (happy+error paths); FIX-04 confirmed via `parse_list_entry()` using whitespace split
- `src/transport.rs` (read 2026-03-01) — confirmed sync transport, 1 existing unit test, no testable write-path mock
- `Cargo.toml` (read 2026-03-01) — confirmed `edition = "2021"`, `thiserror = "2"`, no `lazy_static` dep, no `tokio-test` dev-dep
- `cargo test` output (run 2026-03-01) — confirmed 23 tests pass, 0 failures
- `cargo build` output (run 2026-03-01) — confirmed clean build, no warnings
- `cargo clippy` output (run 2026-03-01) — confirmed no warnings
- [docs.rs/tokio-test — io::Builder](https://docs.rs/tokio-test/latest/tokio_test/io/struct.Builder.html) — confirmed `AsyncRead + AsyncWrite` only (not sync); `Cursor` is the sync equivalent
- [tokio.rs testing guide](https://tokio.rs/tokio/topics/testing) — confirmed `tokio_test::io::Builder` pattern for async I/O mocking
- [thiserror 2.0.18 on crates.io](https://crates.io/crates/thiserror) — confirmed current version 2.0.18 (2026-01-18), features: `std` default

### Secondary (MEDIUM confidence)

- [Rust std::io::Cursor docs](https://doc.rust-lang.org/std/io/struct.Cursor.html) — confirmed `Cursor<Vec<u8>>` implements `Read + Write + BufRead + Seek`

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — directly verified from `Cargo.toml` and source files
- Architecture patterns: HIGH — directly verified from source code, not inferred
- Pitfalls: HIGH — derived from concrete code analysis (specific line numbers cited)
- Mock I/O approach: HIGH — `std::io::Cursor` is stdlib, verified against tokio-test docs confirming it is async-only

**Research date:** 2026-03-01
**Valid until:** 2026-04-01 (stable ecosystem; `thiserror 2` / `rustls 0.23` are stable; no expected breaking changes in 30 days)
