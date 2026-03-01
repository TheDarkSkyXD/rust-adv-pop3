# Architecture Research

**Domain:** Async Rust POP3 client library with dual TLS backends
**Researched:** 2026-03-01
**Confidence:** HIGH (tokio/TLS patterns from official docs; lettre/reqwest/async-imap patterns from verified source)

## Context: What Changes in v2.0

The existing architecture is a single-file synchronous library (`src/pop3.rs`). The v2.0 rewrite replaces it entirely — the fundamental I/O model, TLS abstraction, error handling, and module layout all change. This is a full rewrite, not incremental patching. Nothing from `src/pop3.rs` survives as-is. The response parsing logic and command dispatch logic are salvageable as new implementations of the same concepts.

**What gets deleted:**
- `POP3StreamTypes` enum (sync `TcpStream` / `SslStream<TcpStream>`) — replaced by async stream abstraction
- `POP3Stream` struct as a sync I/O handle — replaced by async `Client`
- Byte-at-a-time `read_response()` — replaced by `BufReader` + `read_line()`
- All `panic!` calls — replaced by `Result` propagation
- `lazy_static` regex block — replaced by `std::sync::LazyLock`
- `extern crate` declarations — removed (edition 2021)
- `is_authenticated: pub bool` — made private, exposed as method

**What gets adapted (concepts survive, implementation changes):**
- `POP3Command` → becomes response parsing context (internal enum, same variants plus new ones)
- `POP3Response::add_line()` state machine → new response parser in `src/response.rs`
- `parse_stat()`, `parse_list_all()`, etc. → ported to new parser module with `?` instead of `unwrap()`
- `POP3Result` variants → renamed to idiomatic Rust, returned from typed methods

## Standard Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         PUBLIC API LAYER                             │
│                                                                      │
│   Client::builder()                                                  │
│       .connect(host, port) -> Result<Client>                         │
│       .connect_tls(host, port, domain) -> Result<Client>             │
│                                                                      │
│   client.stat() -> Result<StatResponse>                              │
│   client.list(Option<u32>) -> Result<ListResponse>                   │
│   client.retr(u32) -> Result<String>                                 │
│   client.top(u32, u32) -> Result<String>                             │
│   client.uidl(Option<u32>) -> Result<UidlResponse>                   │
│   client.capa() -> Result<Vec<String>>                               │
│   client.apop(user, digest) -> Result<()>                            │
│   client.dele(u32) / noop() / rset() / quit() -> Result<()>          │
│                                                                      │
│   Types: StatResponse, MessageMeta, UidEntry, Pop3Error              │
├─────────────────────────────────────────────────────────────────────┤
│                       CONNECTION LAYER                               │
│                                                                      │
│   ┌─────────────────────────────────────────────────────────┐       │
│   │  src/client.rs  — Client struct                          │       │
│   │  - Owns: AsyncStream (feature-gated enum)               │       │
│   │  - Owns: BufReader wrapping write half                  │       │
│   │  - Tracks: session state (authorization / transaction)  │       │
│   │  - Methods: send_command(), read_response()             │       │
│   └─────────────────────────────────────────────────────────┘       │
├─────────────────────────────────────────────────────────────────────┤
│                       TRANSPORT LAYER                                │
│                                                                      │
│   src/tls/mod.rs                                                     │
│   ┌──────────────────┐   ┌──────────────────────────────────┐        │
│   │ #[cfg(feature =  │   │ #[cfg(feature = "openssl-tls")]  │        │
│   │ "rustls-tls")]   │   │                                  │        │
│   │ AsyncStream::    │   │ AsyncStream::                    │        │
│   │ Rustls(...)      │   │ OpenSsl(...)                     │        │
│   └──────────────────┘   └──────────────────────────────────┘        │
│                 + AsyncStream::Plain(TcpStream)                      │
│                                                                      │
│   All variants impl AsyncRead + AsyncWrite (tokio)                   │
├─────────────────────────────────────────────────────────────────────┤
│                      PROTOCOL LAYER                                  │
│                                                                      │
│   src/response.rs  — response parsing                                │
│   src/command.rs   — command formatting                              │
│   src/error.rs     — Pop3Error enum                                  │
└─────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Responsibility | Replaces v1 Element |
|-----------|----------------|---------------------|
| `src/lib.rs` | Crate root; re-exports public API | `src/pop3.rs` crate root declarations |
| `src/client.rs` | Session state, command dispatch, public API methods | `impl POP3Stream` (lines 63–351) |
| `src/tls/mod.rs` | `AsyncStream` enum + TLS connector factory functions | `POP3StreamTypes` enum + `POP3Stream::connect()` |
| `src/response.rs` | Line-by-line parser; typed response structs | `POP3Response` + `parse_*` methods |
| `src/command.rs` | Format command strings; `Command` enum | `POP3Command` enum + ad-hoc `format!` calls |
| `src/error.rs` | `Pop3Error` typed error enum (via `thiserror`) | Mixed `panic!` / `POP3Result::POP3Err` / `std::io::Error` |
| `examples/connect.rs` | Usage example | `example.rs` at project root |
| `tests/mock.rs` | Mock server + integration tests | Nothing (zero tests in v1) |

## Recommended Project Structure

```
src/
├── lib.rs                  # Crate root: pub use, feature-gated re-exports
├── client.rs               # Client struct + all POP3 command methods (pub)
├── command.rs              # Command enum + wire-format serialization (private)
├── response.rs             # Response parser + typed response structs (pub structs, private parser)
├── error.rs                # Pop3Error enum using thiserror (pub)
└── tls/
    ├── mod.rs              # AsyncStream enum + connect() factory (pub(crate))
    ├── plain.rs            # Plain TcpStream wrapping (always compiled)
    ├── openssl.rs          # #[cfg(feature = "openssl-tls")] connect logic
    └── rustls.rs           # #[cfg(feature = "rustls-tls")] connect logic

examples/
└── connect.rs              # End-to-end example (replaces example.rs at root)

tests/
├── mock_server.rs          # Minimal in-process mock POP3 server
└── integration.rs          # Tests against mock server

Cargo.toml                  # Feature flags: openssl-tls, rustls-tls; edition = "2021"
build.rs                    # Optional: compile error if both TLS features enabled together
```

### Structure Rationale

- **`src/tls/`:** Isolates all feature-gated compilation. Both TLS branches live here; `client.rs` never needs `#[cfg(...)]` attributes because the `AsyncStream` enum absorbs the variation. This keeps client logic readable.
- **`src/response.rs`:** Separates parsing from I/O. The parser takes `&str` lines, making it testable without a real connection.
- **`src/command.rs`:** Keeps POP3 wire format in one place. Adding TOP, CAPA, APOP means adding variants here, not hunting through `client.rs`.
- **`src/error.rs`:** Single error type propagated everywhere. `thiserror` derives `Display` and `From` automatically.
- **`tests/mock_server.rs`:** Critical for zero-dependency testing. The v1 codebase had zero tests because the architecture made testing impossible without a live server. Separating parsing from I/O makes unit testing possible.

## Architectural Patterns

### Pattern 1: AsyncStream Enum (feature-gated transport abstraction)

**What:** An enum whose variants correspond to each TLS backend, compiled conditionally. Each variant holds the backend-specific stream type. The enum itself implements `AsyncRead` and `AsyncWrite` by delegating to the active variant.

**When to use:** When you need to support N mutually exclusive stream types without runtime allocation (`Box<dyn Trait>` overhead) and without infecting every call site with generic type parameters.

**Trade-offs:** Avoids trait-object heap allocation; adds a match arm per variant when new backends are added. For 2–3 backends this is manageable.

**Example:**
```rust
// src/tls/mod.rs

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) enum AsyncStream {
    Plain(tokio::net::TcpStream),
    #[cfg(feature = "rustls-tls")]
    Rustls(tokio_rustls::client::TlsStream<tokio::net::TcpStream>),
    #[cfg(feature = "openssl-tls")]
    OpenSsl(tokio_openssl::SslStream<tokio::net::TcpStream>),
}

impl AsyncRead for AsyncStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            AsyncStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "rustls-tls")]
            AsyncStream::Rustls(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "openssl-tls")]
            AsyncStream::OpenSsl(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

// AsyncWrite impl mirrors AsyncRead above
```

### Pattern 2: BufReader Split for Bidirectional Async I/O

**What:** Split the `AsyncStream` into read and write halves using `tokio::io::split()`. Wrap the read half with `BufReader`. This gives cheap buffered line-reads without buffering writes (which POP3 commands don't need).

**When to use:** Any protocol that is line-oriented for reads (POP3 responses are CRLF-terminated lines) but not necessarily buffered for writes (commands are short strings sent once).

**Trade-offs:** `tokio::io::split()` introduces a `Mutex` internally. For a single-connection POP3 client this cost is negligible and the simplicity is worth it.

**Example:**
```rust
// src/client.rs

use tokio::io::{AsyncWriteExt, BufReader, AsyncBufReadExt, split};

pub struct Client {
    reader: BufReader<tokio::io::ReadHalf<tls::AsyncStream>>,
    writer: tokio::io::WriteHalf<tls::AsyncStream>,
    state: SessionState,
}

impl Client {
    async fn read_line(&mut self) -> Result<String, Pop3Error> {
        let mut line = String::new();
        self.reader.read_line(&mut line).await
            .map_err(Pop3Error::Io)?;
        Ok(line)
    }

    async fn send_command(&mut self, cmd: &str) -> Result<(), Pop3Error> {
        self.writer.write_all(cmd.as_bytes()).await
            .map_err(Pop3Error::Io)?;
        self.writer.flush().await.map_err(Pop3Error::Io)
    }
}
```

### Pattern 3: Typed Error Enum with thiserror

**What:** Replace the v1 mix of `panic!`, `POP3Result::POP3Err`, and `std::io::Error` with a single `Pop3Error` enum. Use `thiserror` for `Display` and `From` derivation.

**When to use:** Library crates always. Application code would use `anyhow`; library crates expose typed variants so callers can match on specific failure modes.

**Trade-offs:** Requires enumerating all error categories at design time. The benefit is callers can distinguish an authentication failure from a network error from a malformed server response.

**Example:**
```rust
// src/error.rs

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Pop3Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("Server returned error: {0}")]
    ServerError(String),

    #[error("Authentication failed")]
    AuthFailed,

    #[error("Not authenticated: call login() first")]
    NotAuthenticated,

    #[error("Malformed server response: {0}")]
    ParseError(String),

    #[error("Invalid message number: {0}")]
    InvalidMessage(u32),
}
```

### Pattern 4: Session State Machine (enum, not bool flag)

**What:** Replace `is_authenticated: pub bool` with a private `SessionState` enum. The enum encodes the POP3 session states from RFC 1939: Authorization, Transaction, Update (post-QUIT).

**When to use:** When the boolean flag anti-pattern exists in v1. A `bool` cannot represent three states cleanly and is easily bypassed (was `pub` in v1).

**Trade-offs:** Slightly more verbose state transitions, but enforces protocol correctness at compile time for some patterns.

**Example:**
```rust
// src/client.rs

#[derive(Debug, PartialEq)]
enum SessionState {
    Authorization,   // Before successful USER+PASS or APOP
    Transaction,     // After successful auth; commands available
    Update,          // After QUIT; session ended
}

impl Client {
    fn require_transaction(&self) -> Result<(), Pop3Error> {
        if self.state != SessionState::Transaction {
            Err(Pop3Error::NotAuthenticated)
        } else {
            Ok(())
        }
    }
}
```

### Pattern 5: Feature Flag Mutual Exclusivity via build.rs

**What:** Use a `build.rs` script to emit a compile-time error if both `openssl-tls` and `rustls-tls` features are enabled simultaneously. Cargo does not natively support mutually exclusive features.

**When to use:** When enabling both backends simultaneously would cause link conflicts or ambiguous behavior. For this crate the `AsyncStream` enum would grow variants for both, which is technically valid but confusing — only one TLS backend should be active per build.

**Trade-offs:** A `build.rs` adds build complexity but is the standard pattern (used by sqlx, lettre, and others for the same problem). The compile error message is the only signal to library users.

**Example:**
```rust
// build.rs

fn main() {
    let has_openssl = std::env::var_os("CARGO_FEATURE_OPENSSL_TLS").is_some();
    let has_rustls = std::env::var_os("CARGO_FEATURE_RUSTLS_TLS").is_some();
    if has_openssl && has_rustls {
        eprintln!(
            "error: pop3 features `openssl-tls` and `rustls-tls` \
             are mutually exclusive. Enable only one."
        );
        std::process::exit(1);
    }
}
```

**Cargo.toml feature declaration:**
```toml
[features]
default = ["rustls-tls"]
rustls-tls = ["dep:tokio-rustls", "dep:rustls", "dep:rustls-native-certs"]
openssl-tls = ["dep:tokio-openssl", "dep:openssl"]

[dependencies]
tokio = { version = "1", features = ["net", "io-util", "rt"] }
thiserror = "2"

# TLS backends — exactly one enabled at a time (enforced by build.rs)
tokio-rustls = { version = "0.26", optional = true }
rustls = { version = "0.23", optional = true }
rustls-native-certs = { version = "0.8", optional = true }
tokio-openssl = { version = "0.6", optional = true }
openssl = { version = "0.10", optional = true }
```

## Data Flow

### Command Flow (v2 async)

```
caller: client.stat().await
    |
    v
Client::stat()                          [src/client.rs]
    |-- require_transaction()?
    |-- send_command("STAT\r\n").await?
    |       |
    |       v
    |   writer.write_all(b"STAT\r\n").await  [AsyncWrite on WriteHalf<AsyncStream>]
    |   writer.flush().await
    |
    |-- read_single_line().await?
    |       |
    |       v
    |   BufReader::read_line(&mut line).await  [AsyncBufRead on ReadHalf<AsyncStream>]
    |
    |-- response::parse_stat(&line)?
    |       |
    |       v
    |   Regex match OR manual parse of "+OK <n> <size>\r\n"
    |   Returns StatResponse { message_count: u32, mailbox_size: u64 }
    |
    v
Ok(StatResponse)  returned to caller
```

### Multi-Line Command Flow (RETR, LIST, UIDL all)

```
caller: client.retr(1).await
    |
    v
Client::retr(msg_num)
    |-- send_command("RETR 1\r\n").await?
    |-- read_single_line().await?   -- status line "+OK" or "-ERR"
    |       parse_ok_or_err(&status)?
    |-- read_until_dot_crlf().await?
    |       |
    |       v
    |   loop: BufReader::read_line() until line == ".\r\n"
    |   accumulates Vec<String>
    |
    v
Ok(lines joined as String)
```

### Connection Establishment Flow

```
Client::connect(host, port)
    |
    v
tokio::net::TcpStream::connect((host, port)).await?
    |
    v
AsyncStream::Plain(tcp_stream)
    |
    v
tokio::io::split(stream) -> (read_half, write_half)
BufReader::new(read_half)
    |
    v
Client { reader: BufReader, writer, state: Authorization }
    |-- read server greeting line
    |-- parse_greeting()?     -- check for "+OK"
    v
Ok(Client)  -- caller then calls client.login(user, pass).await
```

### TLS Connection Establishment Flow

```
Client::connect_tls(host, port, domain)
    |
    v
tokio::net::TcpStream::connect((host, port)).await?
    |
    v
[#[cfg(feature = "rustls-tls")]]
tls::rustls::connect(tcp, domain).await?
    |-- TlsConnector::from(Arc<ClientConfig>).connect(domain, tcp).await?
    |-- Returns tokio_rustls::client::TlsStream<TcpStream>
    v
AsyncStream::Rustls(tls_stream)

  OR

[#[cfg(feature = "openssl-tls")]]
tls::openssl::connect(tcp, domain).await?
    |-- SslConnector::builder(SslMethod::tls())?...build()
    |-- tokio_openssl::connect(ssl, domain, tcp).await?
    v
AsyncStream::OpenSsl(ssl_stream)
    |
    v
tokio::io::split(async_stream) -> (read_half, write_half)
BufReader::new(read_half)
Client { reader, writer, state: Authorization }
    |-- read greeting, parse_greeting()?
    v
Ok(Client)
```

### State Transitions

```
Initial: SessionState::Authorization
    |
    | login(user, pass) -> Ok(())     [USER+PASS exchange both succeed]
    | OR apop(user, digest) -> Ok(())
    v
SessionState::Transaction
    |
    | stat() / list() / retr() / dele() / noop() / rset() / uidl() / top() / capa()
    | (all require Transaction state)
    v
SessionState::Transaction  (unchanged unless quit is called)
    |
    | quit() -> Ok(())
    v
SessionState::Update  (session is over; no further commands valid)
```

## Integration Points: New vs Modified

### New Components (did not exist in v1)

| Component | File | Purpose | Integration Point |
|-----------|------|---------|-------------------|
| `Pop3Error` | `src/error.rs` | Typed error enum for all failure modes | All `?` propagations; returned from every public method |
| `AsyncStream` | `src/tls/mod.rs` | Feature-gated transport enum | Constructed in `connect()` / `connect_tls()`; owned by `Client` |
| `rustls` connect fn | `src/tls/rustls.rs` | TLS handshake via tokio-rustls | Called by `Client::connect_tls()` when `rustls-tls` feature active |
| `openssl` connect fn | `src/tls/openssl.rs` | TLS handshake via tokio-openssl | Called by `Client::connect_tls()` when `openssl-tls` feature active |
| `SessionState` | `src/client.rs` | Replaces `is_authenticated: bool` | Checked at start of every authenticated command |
| `StatResponse` | `src/response.rs` | Typed return from `stat()` | Returned from `Client::stat()` |
| `MessageMeta` | `src/response.rs` | Typed list/uidl entry | Returned in `Vec<MessageMeta>` from `list()` / `uidl()` |
| Mock server | `tests/mock_server.rs` | In-process POP3 server for testing | Used by all integration tests |
| `build.rs` | `build.rs` | Enforces mutually exclusive TLS features | Runs at compile time |

### Modified Components (v1 concept rewritten for v2)

| v1 Element | v2 Element | What Changes |
|------------|------------|--------------|
| `POP3Stream::connect()` (sync) | `Client::connect()` async fn | Returns `Result<Client, Pop3Error>`; uses tokio TCP |
| `POP3StreamTypes` enum | `AsyncStream` enum in `src/tls/` | Variants are async stream types; impl `AsyncRead` + `AsyncWrite` |
| `read_response()` byte-at-a-time | `BufReader::read_line().await` | Eliminates syscall-per-byte; async |
| `POP3Command` enum | `Command` enum in `src/command.rs` | Gains `Top`, `Capa`, `Apop`, `StartTls` variants |
| `POP3Response::add_line()` | `parse_*()` functions in `src/response.rs` | Pure functions taking `&[String]`; return `Result` not `panic!` |
| `POP3Result` enum | Per-method return structs | `stat()` returns `StatResponse`, `retr()` returns `String` |
| `lazy_static!` regex block | `std::sync::LazyLock<Regex>` statics | No external dependency; same semantics |
| `login()` panic-on-write-failure | `async fn login()` returning `Result` | All write/read errors propagated via `?` |
| `is_authenticated: pub bool` | `state: SessionState` (private) | Not publicly accessible; `SessionState::Transaction` required |

## Scaling Considerations

This is a single-connection client library — "scaling" applies to how the library composes in large async applications, not server load.

| Concern | Approach | Notes |
|---------|----------|-------|
| Concurrent mailbox access | Caller creates N `Client` instances | Library is not `Clone`; one connection per `Client` |
| Connection reuse | Caller holds `Client` across commands | Session persists until `quit()` or drop |
| Timeout handling | Caller wraps commands in `tokio::time::timeout()` | Library does not mandate timeouts; v1 had none either |
| Large message retrieval | BufReader buffers at 8KB default | `RETR` on large emails stays fully async with no blocking |
| Compile-time binary size | Feature flags eliminate unused TLS backend | Pick one TLS backend per binary |

## Anti-Patterns

### Anti-Pattern 1: Panic on Protocol Error

**What people do:** Use `unwrap()` on regex captures or server responses (v1 did this throughout).
**Why it's wrong:** A malformed or non-RFC-compliant server response crashes the caller's process. Library code must never panic on external input.
**Do this instead:** All parse functions return `Result<T, Pop3Error>`. Use `.ok_or_else(|| Pop3Error::ParseError(...))?` on captures. Never `unwrap()` on anything that depends on server-provided data.

### Anti-Pattern 2: Public Authentication State Flag

**What people do:** Expose `is_authenticated: pub bool` and check it with `if !self.is_authenticated { panic!(...) }` (v1 did this).
**Why it's wrong:** Callers can set the flag to `true` and bypass guards. Authentication-failed sessions still get `is_authenticated = true` because v1 set the flag before reading the server's response to PASS.
**Do this instead:** Make `state: SessionState` private. Transition to `Transaction` only after the server returns `+OK` to PASS (or APOP). Expose state through a read-only accessor if needed.

### Anti-Pattern 3: Monolithic Single-File Architecture

**What people do:** Put all types, parsing, I/O, and error handling in one file (v1 had 537 lines in `src/pop3.rs`).
**Why it's wrong:** The response parser cannot be unit-tested independently from the I/O layer. Adding new POP3 commands requires editing every layer in one file. Feature-gated TLS code pollutes command handling code.
**Do this instead:** Split by concern: `client.rs` for I/O and commands, `response.rs` for parsing, `tls/` for transport, `error.rs` for error types. Each module is independently testable.

### Anti-Pattern 4: Box<dyn AsyncRead + AsyncWrite + Unpin> for Stream Abstraction

**What people do:** Use a trait object to erase the concrete TLS stream type, avoiding the enum approach.
**Why it's wrong:** Adds a heap allocation per connection, prevents stack pinning in some cases, and requires `Unpin` bounds that interact awkwardly with `Pin<Box<dyn ...>>`. For a library with 2–3 fixed backends, the enum approach has zero runtime cost.
**Do this instead:** Use the feature-gated `AsyncStream` enum. Let the compiler monomorphize correctly for the active backend.

### Anti-Pattern 5: Regex for All Response Parsing

**What people do:** Compile regexes for simple fixed-format lines like `+OK 3 1234\r\n`.
**Why it's wrong:** The `+OK`, `-ERR` status prefix check and numeric field extraction in STAT/LIST/UIDL responses are simple enough for manual parsing with `str::split_whitespace()`. Regex adds the `regex` dependency and is overkill for fixed-format protocol lines.
**Do this instead:** Parse status lines manually: check prefix, split on whitespace, parse fields with `str::parse::<u32>()`. Reserve `regex` only if response formats are genuinely complex — which they are not in RFC 1939.

## Build Order for Implementation

Dependencies between components determine which must be built first:

1. **`src/error.rs`** — No dependencies on other new modules. Everything else uses `Pop3Error`.
2. **`src/command.rs`** — No dependencies on other new modules. Defines the `Command` enum; client and tests use it.
3. **`src/response.rs`** — Depends on `error.rs`. Parsers must exist before `client.rs` can call them.
4. **`src/tls/mod.rs`, `tls/plain.rs`, `tls/rustls.rs`, `tls/openssl.rs`** — Depends on `error.rs`. Transport layer needed by `client.rs`.
5. **`src/client.rs`** — Depends on all above. Implements the public API.
6. **`src/lib.rs`** — Thin re-export layer, written last.
7. **`tests/mock_server.rs`** — Written alongside `response.rs` so parsers can be tested immediately.
8. **`tests/integration.rs`** — Written alongside `client.rs` to validate end-to-end flows.
9. **`examples/connect.rs`** — Written last; validates that the public API is ergonomic.

## Sources

- [tokio-rustls docs.rs](https://docs.rs/tokio-rustls/latest/tokio_rustls/) — TlsConnector, TlsStream types and AsyncRead/AsyncWrite impl (HIGH confidence)
- [tokio-openssl docs.rs](https://docs.rs/tokio-openssl/latest/tokio_openssl/) — SslStream AsyncRead/AsyncWrite (HIGH confidence)
- [tokio::io::BufReader docs.rs](https://docs.rs/tokio/latest/tokio/io/struct.BufReader.html) — BufReader, AsyncBufReadExt, read_line pattern (HIGH confidence)
- [lettre TLS abstraction source](https://github.com/lettre/lettre/blob/master/src/transport/smtp/client/tls.rs) — InnerTlsParameters enum pattern for multi-backend TLS (MEDIUM confidence — verified from source)
- [Cargo features — advanced usage](https://blog.turbo.fish/cargo-features/) — mutually exclusive feature flag pattern via build.rs (MEDIUM confidence)
- [Rust Internals: mutually exclusive feature flags](https://internals.rust-lang.org/t/mutually-exclusive-feature-flags/8601) — confirms Cargo does not natively support mutual exclusion (HIGH confidence — official forum)
- [async-pop library structure](https://docs.rs/async-pop) — comparison POP3 async library design: separate error/request/response/sasl modules (MEDIUM confidence)
- [reqwest Cargo.toml TLS features](https://github.com/seanmonstar/reqwest/blob/master/Cargo.toml) — industry pattern for openssl vs rustls feature flag naming (MEDIUM confidence)

---

*Architecture research for: async POP3 client library, v2.0 rewrite*
*Researched: 2026-03-01*
