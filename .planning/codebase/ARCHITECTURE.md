# Architecture

**Analysis Date:** 2026-03-01

## Pattern Overview

**Overall:** Single-module protocol client library (no layered architecture)

**Key Characteristics:**
- All logic lives in a single file `src/pop3.rs` exposed as a Rust library crate
- Public API surface is `POP3Stream` (connection/command struct) and `POP3Result` (return enum)
- Internal implementation details (`POP3Command`, `POP3Response`, `POP3StreamTypes`) are all private
- Synchronous, blocking I/O — no async runtime

## Layers

**Public API Layer:**
- Purpose: Exposes the client interface consumers use
- Location: `src/pop3.rs` (public items only)
- Contains: `POP3Stream` (pub struct), `POP3Result` (pub enum), `POP3EmailMetadata` (pub struct), `POP3EmailUidldata` (pub struct)
- Depends on: Internal command/response layer
- Used by: Library consumers (e.g., `example.rs`)

**Internal Command Layer:**
- Purpose: Encodes POP3 protocol commands as typed variants
- Location: `src/pop3.rs` lines 47–61
- Contains: `POP3Command` (private enum): `Greet`, `User`, `Pass`, `Stat`, `UidlAll`, `UidlOne`, `ListAll`, `ListOne`, `Retr`, `Dele`, `Noop`, `Rset`, `Quit`
- Depends on: Nothing
- Used by: `POP3Stream` methods to dispatch `read_response()`

**Transport Layer:**
- Purpose: Abstracts over plain TCP and TLS streams
- Location: `src/pop3.rs` lines 33–95
- Contains: `POP3StreamTypes` (private enum): `Basic(TcpStream)` and `Ssl(SslStream<TcpStream>)`
- Depends on: `std::net::TcpStream`, `openssl::ssl::SslStream`
- Used by: `POP3Stream::write_str()` and `POP3Stream::read()` — both dispatch over the enum

**Response Parsing Layer:**
- Purpose: Accumulates raw bytes into lines and parses them into typed results
- Location: `src/pop3.rs` lines 384–537
- Contains: `POP3Response` (private struct) with `add_line()` state machine and `parse_*` methods
- Depends on: `lazy_static` regex patterns, `POP3Command` enum for context
- Used by: `POP3Stream::read_response()`

## Data Flow

**Normal Command Flow:**

1. Caller invokes a `POP3Stream` method (e.g., `stat()`, `retr(message_id)`)
2. Method formats the POP3 protocol string (e.g., `"STAT\r\n"`) and writes it via `write_str()`
3. `write_str()` dispatches to the appropriate stream variant (`Basic` or `Ssl`)
4. Method calls `read_response(command_type)` with the matching `POP3Command` variant
5. `read_response()` reads bytes one at a time, accumulating CRLF-terminated lines
6. Each line is passed to `POP3Response::add_line()` which runs a state machine keyed on `POP3Command`
7. When `POP3Response.complete` is true, the boxed response is returned
8. The caller method unwraps `response.result` and returns it as `POP3Result`

**Connection Flow:**

1. `POP3Stream::connect(addr, ssl_context, domain)` opens a `TcpStream`
2. If `ssl_context` is `Some`, upgrades to `SslStream` wrapping the `TcpStream`
3. Reads the server greeting via `read_response(Greet)` to confirm the connection is live
4. Returns `Ok(POP3Stream)` to caller

**Authentication Flow:**

1. Caller calls `login(username, password)`
2. Sends `USER <username>\r\n`, reads response with `read_response(User)`
3. On success sends `PASS <password>\r\n`, sets `is_authenticated = true`
4. Returns `POP3Result::POP3Ok` or `POP3Result::POP3Err`

**State Management:**
- `POP3Stream.is_authenticated: bool` — runtime flag checked at the start of every authenticated command
- `POP3Response.complete: bool` — drives the read loop in `read_response()`
- All state is owned by the `POP3Stream` instance; no global or shared state

## Key Abstractions

**POP3Stream:**
- Purpose: The single connection handle; holds the stream and authentication state
- Examples: `src/pop3.rs` lines 40–351
- Pattern: Builder-style construction via `connect()`, mutable methods for each POP3 command

**POP3Result:**
- Purpose: Typed return value union for all commands — success variants carry parsed data, `POP3Err` signals failure
- Examples: `src/pop3.rs` lines 366–382
- Pattern: Rust `enum` with named fields; callers use `match` exhaustively

**POP3Response (internal):**
- Purpose: Streaming line accumulator that parses raw server text into a `POP3Result` once complete
- Examples: `src/pop3.rs` lines 384–537
- Pattern: State machine — `add_line()` branches on `lines.len() == 0` (status line) vs subsequent lines, uses `complete` flag as termination signal

**Regex Constants:**
- Purpose: Pre-compiled patterns for parsing server responses (STAT, LIST, UIDL, OK/ERR)
- Examples: `src/pop3.rs` lines 21–29
- Pattern: `lazy_static!` macro ensures single compilation; patterns cover `+OK`, `-ERR`, numeric data lines, and the `.\r\n` multiline terminator

## Entry Points

**Library Crate:**
- Location: `src/pop3.rs`
- Triggers: Consumed as a dependency via `extern crate pop3` (Rust 2015 edition) or direct `use`
- Responsibilities: Exports `POP3Stream`, `POP3Result`, `POP3EmailMetadata`, `POP3EmailUidldata`

**Example Binary:**
- Location: `example.rs` (project root)
- Triggers: `cargo run --bin example`
- Responsibilities: Demonstrates connect → login → stat → list → retr → quit workflow against Gmail POP3

## Error Handling

**Strategy:** Mixed — `std::io::Result` for I/O operations, `panic!` for protocol-level failures, `POP3Result::POP3Err` for server-reported errors

**Patterns:**
- `POP3Stream::connect()` returns `Result<POP3Stream>` — caller must handle
- `read_response()` returns `Result<Box<POP3Response>>` — propagated via `?` in `connect()`, matched elsewhere
- Authenticated command methods (e.g., `stat()`, `retr()`) `panic!("login")` if `is_authenticated` is false
- Write failures uniformly `panic!("Error writing")`
- Server `-ERR` responses produce `POP3Result::POP3Err` without panicking

## Cross-Cutting Concerns

**Logging:** None — errors printed to stdout via `println!` in `read_response()` read loop only
**Validation:** Authentication guard (`is_authenticated` check) at the start of each protected method
**Authentication:** Session-level flag on `POP3Stream`; no token, no expiry handling

---

*Architecture analysis: 2026-03-01*
