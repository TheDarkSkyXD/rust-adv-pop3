# Phase 2: Async Core - Research

**Researched:** 2026-03-01
**Domain:** Async Rust / Tokio — TCP client I/O, async transport layer, session state, timeouts, CI
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Client Ownership & quit()
- quit() takes `self` by value (move semantics) — consuming the client so the compiler rejects any use after disconnect
- No auto-QUIT on Drop — if caller drops without quit(), TCP connection closes silently and DELE marks are NOT committed (safer default)
- connect() and login() remain separate steps — preserves POP3 protocol phase separation and allows callers to call capa() before authenticating
- quit() return type: Claude's Discretion

#### Timeout Configuration
- Pop3Error::Timeout dedicated variant — callers can match specifically on timeout vs other I/O errors for retry logic
- Timeouts set at connect time only, immutable after connection — prevents mid-session confusion
- Single vs separate read/write timeouts: Claude's Discretion
- How timeouts are passed to connect(): Claude's Discretion (must be forward-compatible with Phase 4 builder)

#### Session State
- Public `SessionState` enum: `Connected`, `Authenticated`, `Disconnected` — callers can match on it, forward-compatible with Phase 7 reconnect state surfacing
- Public read-only accessor (e.g., `state() -> SessionState`) on the client
- login() only callable when in Connected state — calling when already authenticated returns an error
- Internal enum shape (whether to add granular RFC 1939 states like Greeting/Update): Claude's Discretion

#### CI Setup
- Phase 2 CI: GitHub Actions with cargo test, cargo clippy -D warnings, cargo fmt --check on default features only
- TLS feature flag matrix deferred to Phase 3 when TLS code exists
- Stable Rust only — MSRV defined in Phase 3 before publish
- Ubuntu runner only — cross-platform matrix added in Phase 3
- No code coverage tooling yet — added in Phase 3

### Claude's Discretion
- Exact async runtime integration details (tokio feature flags, BufReader approach)
- Mock transport design for async tests (tokio_test::io::Builder vs custom)
- Module structure decisions (whether to split client.rs further)
- Whether to keep existing sync test patterns or fully migrate to async mocks
- Exact SessionState enum variant naming and internal transition logic

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| ASYNC-01 | All public API methods are `async fn` using tokio runtime | Tokio 1.x `#[tokio::main]`, `#[tokio::test]` macros; Transport methods become async |
| ASYNC-02 | Reads use `tokio::io::BufReader` with line-oriented buffering | `BufReader<R>` implements `AsyncBufRead` → `AsyncBufReadExt::read_line()` available |
| ASYNC-03 | Multi-line responses correctly handle RFC 1939 dot-unstuffing | Existing `read_multiline()` logic is correct; needs async conversion only |
| ASYNC-04 | Session state tracked via `SessionState` enum (not a public bool field) | Locked decision: `Connected`, `Authenticated`, `Disconnected` variants |
| ASYNC-05 | Connection supports configurable read/write timeouts | `tokio::time::timeout()` wraps any `Future`; returns `Elapsed` on expiry |
| API-03 | All public types derive `Debug` | Types in `types.rs` already derive `Debug`; `SessionState` must also derive it |
| API-04 | `Client` consumes `self` on `quit()` preventing use-after-disconnect | `pub async fn quit(self) -> Result<()>` — borrow checker enforces at compile time |
| QUAL-03 | GitHub Actions CI runs tests, clippy, and format checks | `dtolnay/rust-toolchain@stable` + three jobs: test, clippy (-D warnings), fmt |
| QUAL-04 | CI matrix tests both `rustls-tls` and `openssl-tls` feature flags | DEFERRED to Phase 3 per locked decision; Phase 2 CI uses default features only |
</phase_requirements>

---

## Summary

Phase 2 converts the existing synchronous POP3 client to fully async using Tokio 1.x. The codebase is already well-structured for this migration: the `Transport` struct cleanly encapsulates I/O, the `response` module is pure parsing with no I/O, and all test helpers are in `#[cfg(test)]` blocks. The migration is a mechanical transformation — replace `std::io::BufReader<TcpStream>` with `tokio::io::BufReader<tokio::net::TcpStream>`, add `.await` to all I/O calls, mark all methods `async fn`, and update the mock transport to use `tokio_test::io::Builder`.

The key API design challenges are (1) splitting the `TcpStream` into read/write halves so the transport can hold a `BufReader` on the read half while still writing commands, (2) threading `tokio::time::timeout()` around every read operation to enforce the connect-time timeout, and (3) replacing the `authenticated: bool` field with the `SessionState` enum while making `quit()` consume `self`. None of these are novel — they follow well-established Tokio patterns verified against official documentation.

The CI workflow is new (no `.github/workflows/` directory exists yet) and must be created from scratch using `dtolnay/rust-toolchain@stable`, the current community standard for Rust GitHub Actions.

**Primary recommendation:** Use `tokio::io::split()` to produce `(ReadHalf, WriteHalf)`, wrap the read half in `BufReader`, and hold both in the `Transport` struct. Wrap every read call with `tokio::time::timeout()`. Use `tokio_test::io::Builder` for test mocks — it produces a `Mock` implementing `AsyncRead + AsyncWrite`, which can be wrapped in `BufReader` for line reading. Mark all tests `#[tokio::test]`.

---

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.49 (latest) | Async runtime, TcpStream, time, io utilities | De facto standard for async Rust; largest ecosystem |
| tokio (features) | `net`, `io-util`, `time`, `rt-multi-thread`, `macros` | TcpStream networking, BufReader/split, timeout, runtime, #[tokio::main] | Each feature enables one domain of functionality |

### Supporting (dev-dependencies)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio-test | 0.4 | `tokio_test::io::Builder` mock I/O | All async transport tests — replaces the existing sync Cursor mock |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| tokio | async-std | tokio has 10x the downloads, wider TLS ecosystem, and the locked roadmap already chose tokio |
| tokio_test::io::Builder | Custom mock with `Cursor<Vec<u8>>` wrapped in `AsyncRead` impl | tokio_test validates write ordering (panics on unexpected writes), which catches protocol bugs; worth the dep |
| tokio::io::split() | `TcpStream::into_split()` (owned halves) | `io::split()` works on any `AsyncRead+AsyncWrite` including tokio_test Mock; `into_split()` only works on TcpStream. Use `io::split()` for testability. |

**Installation (add to Cargo.toml):**
```toml
[dependencies]
tokio = { version = "1", features = ["net", "io-util", "time", "rt-multi-thread", "macros"] }

[dev-dependencies]
tokio-test = "0.4"
```

Remove existing sync I/O dependencies that become unused:
- `rustls` and `rustls-native-certs` stay for now (TLS logic in client.rs references them; Phase 3 will refactor fully, but Phase 2 keeps plain-TCP only and can leave TLS compile-guarded behind a feature flag).

---

## Architecture Patterns

### Recommended Project Structure

No new directories needed. All changes are within existing files:

```
src/
├── lib.rs          # Add SessionState to pub re-exports
├── client.rs       # Replace bool with SessionState; all methods become async fn; quit(self)
├── transport.rs    # Replace std BufReader/TcpStream with tokio BufReader + split halves
├── error.rs        # Add Pop3Error::Timeout variant
├── response.rs     # No changes — pure parsing, already async-compatible
└── types.rs        # Add Debug derive to any type missing it (already derived on all)
.github/
└── workflows/
    └── ci.yml      # New — three jobs: test, clippy, fmt
```

### Pattern 1: Transport with Split Halves

**What:** `tokio::io::split()` separates a single `AsyncRead+AsyncWrite` into independent `ReadHalf` and `WriteHalf`. Wrap `ReadHalf` in `BufReader` for line-oriented reading. Use `WriteHalf` directly for `write_all()`.

**When to use:** Any time you need buffered line reading AND writing on the same stream. Required because a single `&mut stream` cannot be borrowed for both read and write simultaneously in Rust.

**Why `io::split()` over `TcpStream::into_split()`:** `tokio::io::split()` works on any `AsyncRead+AsyncWrite` value, including `tokio_test::io::Mock`. This makes the transport testable without real TCP sockets. `TcpStream::into_split()` only works on `TcpStream`.

**Example:**
```rust
// Source: https://docs.rs/tokio/latest/tokio/io/index.html + https://tokio.rs/tokio/tutorial/io
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

struct Transport {
    reader: BufReader<io::ReadHalf<TcpStream>>,
    writer: io::WriteHalf<TcpStream>,
}

impl Transport {
    async fn connect_plain(addr: impl tokio::net::ToSocketAddrs) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let (read_half, write_half) = io::split(stream);
        Ok(Transport {
            reader: BufReader::new(read_half),
            writer: write_half,
        })
    }

    async fn send_command(&mut self, cmd: &str) -> Result<()> {
        self.writer.write_all(cmd.as_bytes()).await?;
        self.writer.write_all(b"\r\n").await?;
        self.writer.flush().await?;
        Ok(())
    }

    async fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(Pop3Error::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed",
            )));
        }
        Ok(line)
    }
}
```

### Pattern 2: Timeout Wrapping on Every Read

**What:** `tokio::time::timeout(duration, future)` returns `Result<T, Elapsed>`. An `Elapsed` error means the future didn't complete in time. Convert `Elapsed` to `Pop3Error::Timeout`.

**When to use:** Every call to `read_line()` and `read_multiline()`. The timeout is set at connect time and stored in the transport.

**Example:**
```rust
// Source: https://docs.rs/tokio/latest/tokio/time/fn.timeout.html
use tokio::time::{timeout, Duration};

async fn read_line_with_timeout(&mut self) -> Result<String> {
    timeout(self.timeout, self.read_line_inner())
        .await
        .map_err(|_elapsed| Pop3Error::Timeout)?
}
```

### Pattern 3: Consuming `self` on `quit()`

**What:** `pub async fn quit(self) -> Result<()>` takes ownership of the client. After `quit()` returns, the variable is moved and the compiler rejects any further method calls.

**When to use:** Exactly once — the `quit()` method. No other method consumes `self`.

**Example:**
```rust
// Enforced by Rust's ownership/move semantics — no runtime cost
pub async fn quit(self) -> Result<()> {
    // send_and_check on mut self via temporary — need to think about destructuring
    // Pattern: destructure transport out of self to send final command
    let mut client = self;  // or just use self directly
    client.transport.send_command("QUIT").await?;
    let line = client.transport.read_line().await?;
    let _ = response::parse_status_line(&line)?;
    Ok(())
    // client is dropped here, TCP connection closes
}

// Caller gets compile error:
// let client = Pop3Client::connect(...).await?;
// client.quit().await?;
// client.stat().await?;  // error[E0382]: borrow of moved value: `client`
```

### Pattern 4: SessionState Enum replacing `bool`

**What:** Replace `authenticated: bool` with `state: SessionState`. Locked decision: `Connected`, `Authenticated`, `Disconnected` variants.

**When to use:** All state tracking in `Pop3Client`. `require_auth()` checks for `SessionState::Authenticated`. `login()` returns error if already `Authenticated`.

**Example:**
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Connected,
    Authenticated,
    Disconnected,
}

impl Pop3Client {
    fn require_auth(&self) -> Result<()> {
        match self.state {
            SessionState::Authenticated => Ok(()),
            _ => Err(Pop3Error::NotAuthenticated),
        }
    }

    pub async fn login(&mut self, username: &str, password: &str) -> Result<()> {
        if self.state != SessionState::Connected {
            return Err(Pop3Error::NotAuthenticated);  // or new AlreadyAuthenticated variant
        }
        // ... USER/PASS exchange ...
        self.state = SessionState::Authenticated;
        Ok(())
    }
}
```

### Pattern 5: tokio_test::io::Builder for Mock Tests

**What:** `tokio_test::io::Builder` constructs a `Mock` that implements `AsyncRead + AsyncWrite`. Script `.read(bytes)` for server→client data and `.write(bytes)` for client→server expectations. The mock panics if writes don't match expectations.

**When to use:** All `#[tokio::test]` tests replacing the existing sync `Cursor<Vec<u8>>` mock pattern.

**Note on semantics:** In `tokio_test::io::Builder`, `.read(bytes)` means "when the code reads, return these bytes" (server→client). `.write(bytes)` means "expect the code to write exactly these bytes" (client→server). This is the opposite of the intuitive "I'm writing to it" mental model.

**Example:**
```rust
// Source: https://tokio.rs/tokio/topics/testing
// Source: https://docs.rs/tokio-test/latest/tokio_test/io/struct.Builder.html
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_test::io::Builder;

#[tokio::test]
async fn test_stat_command() {
    let mock = Builder::new()
        .write(b"STAT\r\n")           // expect client to write STAT
        .read(b"+OK 5 12345\r\n")     // server responds with stat info
        .build();

    let (read_half, write_half) = tokio::io::split(mock);
    let mut transport = Transport {
        reader: BufReader::new(read_half),
        writer: write_half,
        timeout: Duration::from_secs(30),
    };

    transport.send_command("STAT").await.unwrap();
    let line = transport.read_line().await.unwrap();
    assert!(line.starts_with("+OK 5 12345"));
}
```

### Pattern 6: Generic Transport for Testability

**What:** Make `Transport` generic over `R: AsyncRead + Unpin` and `W: AsyncWrite + Unpin` so tests inject mock halves without type erasure.

**When to use:** If the clean split between `BufReader<ReadHalf>` and `WriteHalf` becomes unwieldy for tests. Alternative: use concrete types with `#[cfg(test)]` mock constructors (mirrors the existing approach in Phase 1).

**Recommendation:** Keep the existing `#[cfg(test)] impl Transport { fn mock(...) }` pattern but replace the internals with `tokio_test::io::Builder`. This minimizes the diff from Phase 1 and avoids introducing generics before Phase 4's builder pattern.

**Concrete approach:**
```rust
// In transport.rs
pub(crate) struct Transport {
    reader: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
    timeout: Duration,
}

#[cfg(test)]
impl Transport {
    pub(crate) fn mock(mock: tokio_test::io::Mock) -> Self {
        let (r, w) = tokio::io::split(mock);
        Transport {
            reader: BufReader::new(Box::new(r)),
            writer: Box::new(w),
            timeout: Duration::from_secs(30),
        }
    }
}
```

**Alternative (simpler, avoids Box):** Use an enum for the stream type, same pattern as Phase 1 but with async variants. This avoids heap allocation and keeps the type system explicit.

```rust
enum Stream {
    Plain {
        reader: BufReader<ReadHalf<TcpStream>>,
        writer: WriteHalf<TcpStream>,
    },
    #[cfg(test)]
    Mock {
        reader: BufReader<ReadHalf<tokio_test::io::Mock>>,
        writer: WriteHalf<tokio_test::io::Mock>,
    },
}
```

**Chosen approach for Phase 2:** The `Box<dyn ...>` approach is simpler to implement, avoids the match-arm duplication of the enum approach, and the heap allocation is negligible for a network client. The planner should use `Box<dyn AsyncRead + Unpin + Send>` and `Box<dyn AsyncWrite + Unpin + Send>`.

### Anti-Patterns to Avoid

- **Blocking in async context:** Never call `std::net::TcpStream::connect()` or any `std::io` blocking read inside `async fn`. Use `tokio::net::TcpStream::connect().await`.
- **Using `tokio::task::block_in_place` or `spawn_blocking`:** These exist for CPU-bound work or legacy blocking code, not for async I/O migration. Don't use them.
- **`TcpStream::into_split()` for tests:** Works only on real `TcpStream`, not mock I/O. Use `tokio::io::split()` instead.
- **Setting socket timeouts via `set_read_timeout`:** The `tokio::net::TcpStream` does NOT support `set_read_timeout()` — it's an async runtime managed stream. Timeouts must be implemented with `tokio::time::timeout()`.
- **Forgetting `Unpin` on generics:** Futures that hold references to `!Unpin` types require `Pin`. `BufReader` from tokio is `Unpin` when the inner reader is `Unpin`. `Box<dyn AsyncRead + Unpin>` is the simplest way to ensure this.
- **Partial reads in `read_line`:** `AsyncBufReadExt::read_line()` returns `usize` (bytes read). If it returns 0, the connection closed. Always check for EOF.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Async line buffering | Custom line buffer accumulator | `tokio::io::BufReader` + `AsyncBufReadExt::read_line()` | Handles partial reads, buffer growth, CRLF correctly |
| Timeouts | `select!` with `sleep()` in every read | `tokio::time::timeout(duration, future)` | Handles cancellation correctly; `select!` with sleep leaks partial read data on cancel |
| Mock async I/O | Custom `AsyncRead` impl with `Cursor` | `tokio_test::io::Builder` | Validates write ordering (panics on unexpected bytes); free with `tokio-test` crate |
| Async runtime setup | Manual executor | `#[tokio::main]` / `#[tokio::test]` macros | Single-threaded vs multi-threaded handled automatically |
| Write ordering enforcement | Manual write buffer inspection | `tokio_test::io::Builder` `.write()` expectations | Builder panics if writes are wrong order or wrong content |

**Key insight:** `tokio::net::TcpStream` does NOT provide `set_read_timeout()`. Any code that tries to set a socket-level timeout via `set_read_timeout` will not compile. All timeouts must go through `tokio::time::timeout()`.

---

## Common Pitfalls

### Pitfall 1: tokio::net::TcpStream Has No set_read_timeout

**What goes wrong:** Developer copies the existing `set_timeouts()` helper from `transport.rs` which calls `tcp.set_read_timeout()` and `tcp.set_write_timeout()`. This calls `std::net::TcpStream` methods that don't exist on `tokio::net::TcpStream`.
**Why it happens:** `tokio::net::TcpStream` wraps `std::net::TcpStream` but deliberately hides the blocking timeout API.
**How to avoid:** Delete `set_timeouts()` entirely. Implement timeouts by wrapping every `read_line().await` and `read_multiline()` call in `tokio::time::timeout(self.timeout, ...)`.
**Warning signs:** `error[E0599]: no method named set_read_timeout found for struct tokio::net::TcpStream`

### Pitfall 2: Using `tokio_test::io::Builder` .read/.write Semantics Backward

**What goes wrong:** Developer writes `.read(b"STAT\r\n")` expecting the mock to "read what the client writes" and `.write(b"+OK\r\n")` expecting to "write server data to client". This is backwards.
**Why it happens:** The builder's perspective is the stream's perspective, not the client's. `.read()` = "data available for the code to read FROM" (server→client). `.write()` = "data the code should write TO" (client→server, validated against expected bytes).
**How to avoid:** Mnemonic: "read = what the test code gets back; write = what the test code sends out (and must match exactly)."
**Warning signs:** Mock panics with "unexpected write" or test hangs waiting for data.

### Pitfall 3: Cancelling read_line with timeout Loses Partial Data

**What goes wrong:** Using `tokio::select!` with a sleep branch alongside `read_line()`. If the sleep fires first, the `read_line()` future is dropped. Any bytes already buffered in `BufReader` that formed a partial line are lost.
**Why it happens:** `BufReader` has consumed those bytes from the kernel buffer into its internal buffer. Dropping the future doesn't un-consume them.
**How to avoid:** Use `tokio::time::timeout()` which wraps the entire `read_line()` call. On timeout, the whole transport becomes unusable anyway (we return `Pop3Error::Timeout`). Since `Timeout` is a terminal error for this session (per the locked decision that timeouts are set at connect-time and immutable), this data loss is acceptable — the caller should reconnect.

### Pitfall 4: quit(self) Requires mut Methods Cannot Borrow

**What goes wrong:** `quit(self)` takes ownership, but internal methods like `send_and_check` take `&mut self`. After moving `self` into `quit`, you can't call `self.transport.send_command()`.
**Why it happens:** You can't borrow `self` after moving it.
**How to avoid:** Inside `quit(self)`, operate on the moved value directly:
```rust
pub async fn quit(self) -> Result<()> {
    let mut this = self;  // shadow for mut access
    this.transport.send_command("QUIT").await?;
    let line = this.transport.read_line().await?;
    response::parse_status_line(&line)?;
    this.state = SessionState::Disconnected;
    Ok(())
    // this is dropped, TCP connection closes
}
```
**Warning signs:** `error[E0505]: cannot move out of self because it is borrowed`

### Pitfall 5: Missing Tokio Features in Cargo.toml

**What goes wrong:** `tokio::net::TcpStream`, `tokio::io::BufReader`, or `tokio::time::timeout` compile with "use of undeclared crate or module" or "no method found" errors despite tokio being in dependencies.
**Why it happens:** Tokio uses feature flags to keep compile times manageable. Each feature enables a different part of the API.
**How to avoid:** Use exactly these features:
```toml
tokio = { version = "1", features = ["net", "io-util", "time", "rt-multi-thread", "macros"] }
```
- `net` → `tokio::net::TcpStream`
- `io-util` → `BufReader`, `split()`, `AsyncBufReadExt`, `AsyncWriteExt`
- `time` → `tokio::time::timeout`, `Duration`
- `rt-multi-thread` → multi-threaded runtime (needed for `#[tokio::main]` with default settings)
- `macros` → `#[tokio::main]`, `#[tokio::test]`

### Pitfall 6: `async fn` on pub Methods Breaks the lib.rs Doctest

**What goes wrong:** The existing lib.rs doctest uses `fn main() -> pop3::Result<()>` (synchronous). After the async migration, all public methods are `async fn` so the doctest fails to compile.
**Why it happens:** Doctests run as standalone programs. An `async fn main()` requires a runtime.
**How to avoid:** Update the lib.rs doctest to use `#[tokio::main]`:
```rust
/// ```no_run
/// #[tokio::main]
/// async fn main() -> pop3::Result<()> {
///     let client = pop3::Pop3Client::connect(("pop.example.com", 110)).await?;
///     client.login("user", "pass").await?;
///     // ...
/// }
/// ```
```

---

## Code Examples

Verified patterns from official Tokio documentation:

### Tokio TCP Connect + BufReader + Write Half
```rust
// Source: https://tokio.rs/tokio/tutorial/io + https://docs.rs/tokio/latest/tokio/io/index.html
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

let stream = TcpStream::connect("pop.example.com:110").await?;
let (read_half, write_half) = io::split(stream);
let mut reader = BufReader::new(read_half);
let mut writer = write_half;

// Write a command
writer.write_all(b"STAT\r\n").await?;
writer.flush().await?;

// Read response line
let mut line = String::new();
reader.read_line(&mut line).await?;
```

### tokio::time::timeout Around a Read
```rust
// Source: https://docs.rs/tokio/latest/tokio/time/fn.timeout.html
use tokio::time::{timeout, Duration};
use std::io;

async fn read_with_timeout(
    reader: &mut BufReader<impl AsyncBufRead + Unpin>,
    duration: Duration,
) -> Result<String, Pop3Error> {
    let mut line = String::new();
    timeout(duration, reader.read_line(&mut line))
        .await
        .map_err(|_| Pop3Error::Timeout)?  // Elapsed -> Timeout
        .map_err(Pop3Error::Io)?;           // io::Error -> Io
    if line.is_empty() {
        return Err(Pop3Error::Io(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "connection closed",
        )));
    }
    Ok(line)
}
```

### tokio_test Mock Transport for Async Tests
```rust
// Source: https://docs.rs/tokio-test/latest/tokio_test/io/struct.Builder.html
// Source: https://tokio.rs/tokio/topics/testing
use tokio_test::io::Builder;
use tokio::io::{self, BufReader};

fn build_mock_transport(server_script: &[(&str, &str)]) -> Transport {
    let mut builder = Builder::new();
    for (client_sends, server_responds) in server_script {
        builder.write(client_sends.as_bytes());
        builder.read(server_responds.as_bytes());
    }
    let mock = builder.build();
    let (r, w) = io::split(mock);
    Transport {
        reader: BufReader::new(Box::new(r)),
        writer: Box::new(w),
        timeout: Duration::from_secs(30),
    }
}

#[tokio::test]
async fn stat_sends_correct_command() {
    let mock = Builder::new()
        .write(b"STAT\r\n")
        .read(b"+OK 5 12345\r\n")
        .build();
    // wrap mock and test ...
}
```

### GitHub Actions CI Workflow (minimal, Ubuntu, stable)
```yaml
# .github/workflows/ci.yml
# Source: https://dev.to/bampeers/rust-ci-with-github-actions-1ne9
#         https://gist.github.com/LukeMathWalker/5ae1107432ce283310c3e601fac915f3
name: CI

on:
  push:
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    name: Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test

  clippy:
    name: Clippy
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - run: cargo clippy -- -D warnings

  fmt:
    name: Format
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --check
```

### SessionState Enum + state() Accessor
```rust
// Derived from locked decisions in CONTEXT.md
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Connected,
    Authenticated,
    Disconnected,
}

pub struct Pop3Client {
    transport: Transport,
    greeting: String,
    state: SessionState,
}

impl Pop3Client {
    pub fn state(&self) -> SessionState {
        self.state.clone()
    }
}
```

### Pop3Error::Timeout Variant Addition
```rust
// In error.rs — add to existing Pop3Error enum
/// The operation timed out waiting for the server.
#[error("timed out")]
Timeout,
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `std::io::BufReader<TcpStream>` + blocking reads | `tokio::io::BufReader` + `AsyncBufReadExt::read_line().await` | Tokio 1.0 (2020) | `.await` instead of blocking; must use `io::split()` for concurrent read/write |
| `std::net::TcpStream::set_read_timeout()` | `tokio::time::timeout(dur, future)` wrapping each read | Tokio 1.0 | Socket-level timeouts not supported; must wrap futures |
| `Cursor<Vec<u8>>` + `std::io::BufReader` for mock tests | `tokio_test::io::Builder` producing `Mock: AsyncRead+AsyncWrite` | tokio-test 0.4 | Validates write expectations; works with `BufReader<ReadHalf<Mock>>` |
| `actions-rs/*` GitHub Actions | `dtolnay/rust-toolchain@stable` | ~2022 | `actions-rs` is unmaintained; `dtolnay/rust-toolchain` is the current standard |

**Deprecated/outdated:**
- `actions-rs/toolchain@v1`: Unmaintained since 2022. Use `dtolnay/rust-toolchain@stable` instead.
- `actions-rs/clippy-check`: Unmaintained. Use `cargo clippy -- -D warnings` directly.
- `tokio::io::lines()` (returning a `Stream`): Still valid but requires extra dependency on `futures::StreamExt`. The `AsyncBufReadExt::read_line()` loop pattern is simpler for our use case.

---

## Open Questions

1. **Single timeout vs separate read/write timeouts (Claude's Discretion)**
   - What we know: Locked decision says timeouts are set at connect-time only; single vs separate is discretionary
   - What's unclear: Whether separate read/write timeouts add any value for a request-response protocol like POP3 (each command is write-then-read, so a single "operation timeout" covers both)
   - Recommendation: Use a single `Duration` stored in `Transport`. Apply it to every `read_line()` call. Write commands are fast (local kernel buffer); only reads block on network. Name the field `read_timeout` to be explicit. Phase 4 builder can expose it as `with_timeout(Duration)`.

2. **How timeouts are passed to connect() (Claude's Discretion)**
   - What we know: Must be forward-compatible with Phase 4 builder; builder hasn't been designed yet
   - What's unclear: Whether `connect(addr, timeout)` should take an `Option<Duration>` or a plain `Duration`
   - Recommendation: `pub async fn connect(addr: impl ToSocketAddrs, timeout: Duration) -> Result<Self>` with a pub const `DEFAULT_TIMEOUT: Duration = Duration::from_secs(30)`. Callers pass the timeout explicitly; a convenience `connect_default(addr)` can use the constant. The Phase 4 builder will call `connect()` internally with a builder-configured value.

3. **quit() return type — Ok(()) or Ok(String) (Claude's Discretion)**
   - What we know: quit() must consume self; it sends QUIT and reads the server farewell message
   - What's unclear: Whether callers ever care about the server's farewell string
   - Recommendation: `-> Result<()>`. The farewell text is informational only. If a caller needs it (unlikely), they can log the raw server response before calling quit. Matches the existing sync behavior.

4. **Internal SessionState granularity (Claude's Discretion)**
   - What we know: Public enum has `Connected`, `Authenticated`, `Disconnected`. Locking into three states.
   - What's unclear: Whether RFC 1939's "AUTHORIZATION" vs "TRANSACTION" vs "UPDATE" states should be tracked internally
   - Recommendation: No internal granularity beyond the three public states for Phase 2. The `Disconnected` state is set after `quit(self)` consumes the client — at that point the client is dropped anyway, so it serves mainly as documentation of the terminal state.

---

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness + `#[tokio::test]` attribute macro |
| Config file | None — `cargo test` discovers `#[test]` and `#[tokio::test]` automatically |
| Quick run command | `cargo test` |
| Full suite command | `cargo test` (all 52 existing tests pass; Phase 2 adds async tests) |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| ASYNC-01 | All public methods are `async fn` | Compile check | `cargo test` (fails to compile if not async) | ❌ Wave 0 — migrate existing tests to `#[tokio::test]` |
| ASYNC-02 | `tokio::io::BufReader` used for reads | Unit (transport) | `cargo test transport` | ❌ Wave 0 — rewrite `transport.rs` mock |
| ASYNC-03 | Dot-unstuffing works in async read_multiline | Unit (transport) | `cargo test retr_dot_unstuffing` | Partial — sync test exists; needs async version |
| ASYNC-04 | `SessionState` enum tracks state correctly | Unit (client) | `cargo test session_state` | ❌ Wave 0 — new tests needed |
| ASYNC-05 | Timeout returns `Pop3Error::Timeout` on expiry | Unit (transport) | `cargo test timeout` | ❌ Wave 0 — new tests needed |
| API-03 | All public types derive `Debug` | Compile check | `cargo test` (doc-tests exercise Debug formatting) | Partial — types.rs already has Debug; SessionState needs it |
| API-04 | `quit(self)` prevents use after disconnect | Compile check | `cargo test` (compile error if violated) | ❌ Wave 0 — test that quit consumes self |
| QUAL-03 | CI passes `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check` | CI smoke test | `.github/workflows/ci.yml` | ❌ Wave 0 — create CI workflow |
| QUAL-04 | CI tests TLS feature flags | CI | N/A — deferred to Phase 3 | N/A |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `cargo test && cargo clippy -- -D warnings && cargo fmt --check`
- **Phase gate:** Full suite green + CI passing before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] Update `transport.rs` — replace `Stream` enum with `Box<dyn AsyncRead+Unpin+Send>` / `Box<dyn AsyncWrite+Unpin+Send>` + add timeout field; all methods become `async fn`
- [ ] Add `tokio` dependency to `Cargo.toml` (`net`, `io-util`, `time`, `rt-multi-thread`, `macros` features)
- [ ] Add `tokio-test = "0.4"` to `[dev-dependencies]` in `Cargo.toml`
- [ ] Update `#[cfg(test)] impl Transport { fn mock(...) }` to use `tokio_test::io::Builder` → `io::split(mock)` → `Box<dyn ...>` pattern
- [ ] Migrate all `#[test]` tests in `client.rs` to `#[tokio::test]`
- [ ] Create `.github/workflows/ci.yml` with three jobs: test, clippy, fmt

---

## Sources

### Primary (HIGH confidence)
- [https://docs.rs/tokio/latest/tokio/io/struct.BufReader.html](https://docs.rs/tokio/latest/tokio/io/struct.BufReader.html) — BufReader API, AsyncBufRead, AsyncBufReadExt relationship
- [https://docs.rs/tokio/latest/tokio/time/fn.timeout.html](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html) — timeout() signature, Elapsed error type
- [https://docs.rs/tokio-test/latest/tokio_test/io/struct.Builder.html](https://docs.rs/tokio-test/latest/tokio_test/io/struct.Builder.html) — Builder methods: .read(), .write(), .build()
- [https://tokio.rs/tokio/tutorial/io](https://tokio.rs/tokio/tutorial/io) — io::split() pattern, AsyncWriteExt::write_all(), TcpStream split
- [https://tokio.rs/tokio/topics/testing](https://tokio.rs/tokio/topics/testing) — tokio_test::io::Builder usage, #[tokio::test] attribute

### Secondary (MEDIUM confidence)
- [https://biriukov.dev/docs/async-rust-tokio-io/3-tokio-io-patterns/](https://biriukov.dev/docs/async-rust-tokio-io/3-tokio-io-patterns/) — io::split() for request-response TCP clients; generic AsyncRead+AsyncWrite approach
- [https://dev.to/bampeers/rust-ci-with-github-actions-1ne9](https://dev.to/bampeers/rust-ci-with-github-actions-1ne9) — GitHub Actions workflow structure for Rust (dtolnay/rust-toolchain)
- [https://gist.github.com/LukeMathWalker/5ae1107432ce283310c3e601fac915f3](https://gist.github.com/LukeMathWalker/5ae1107432ce283310c3e601fac915f3) — Luke Mathwalker's recommended minimal Rust CI workflow
- `cargo search tokio` output — confirmed tokio 1.49 is current latest (2026-03-01)

### Tertiary (LOW confidence)
- [https://users.rust-lang.org/t/how-can-set-a-timeout-when-reading-data-using-tokio-bufreader/39347](https://users.rust-lang.org/t/how-can-set-a-timeout-when-reading-data-using-tokio-bufreader/39347) — Community confirmation that tokio::time::timeout() is the correct approach (no socket-level read timeout in tokio)

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — tokio 1.49 verified via `cargo search`; feature flags verified against official docs
- Architecture: HIGH — split pattern, BufReader, timeout wrapping all verified against official tokio docs
- Mock testing: HIGH — tokio_test::io::Builder API verified against official crate docs
- CI workflow: MEDIUM — dtolnay/rust-toolchain confirmed as community standard; workflow structure verified
- Pitfalls: HIGH — tokio's lack of `set_read_timeout` is a documented fact; other pitfalls derive from the above verified patterns

**Research date:** 2026-03-01
**Valid until:** 2026-04-01 (tokio 1.x API is stable; unlikely to change)
