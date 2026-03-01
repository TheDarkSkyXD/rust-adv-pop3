# Coding Conventions

**Analysis Date:** 2026-03-01

## Naming Patterns

**Types and Enums:**
- PascalCase with a `POP3` prefix for all public types: `POP3Stream`, `POP3Result`, `POP3Response`, `POP3EmailMetadata`, `POP3EmailUidldata`
- Enum variants also carry the `POP3` prefix: `POP3Ok`, `POP3Err`, `POP3Stat`, `POP3List`, `POP3Message`, `POP3Uidl`
- Internal enum types (not pub) also use PascalCase with domain prefix: `POP3StreamTypes`, `POP3Command`
- Command variants are abbreviated PascalCase matching POP3 protocol names: `Greet`, `User`, `Pass`, `Stat`, `UidlAll`, `UidlOne`, `ListAll`, `ListOne`, `Retr`, `Dele`, `Noop`, `Rset`, `Quit`

**Functions and Methods:**
- `snake_case` for all functions and methods: `write_str`, `read_response`, `parse_stat`, `parse_uidl_all`, `parse_list_one`, `add_line`
- Public API methods named after POP3 protocol commands in lowercase: `login`, `stat`, `uidl`, `list`, `retr`, `dele`, `rset`, `quit`, `noop`
- Private helpers prefixed with action verbs: `parse_stat`, `parse_message`, `parse_uidl_all`, `parse_uidl_one`, `parse_list_all`, `parse_list_one`

**Variables:**
- `snake_case` for all locals: `tcp_stream`, `ssl_context`, `line_buffer`, `byte_buffer`, `user_command`, `pass_command`
- Command locals named `{verb}_command`: `stat_command`, `uidl_command`, `list_command`, `retr_command`, `dele_command`, `quit_command`, `noop_command`
- `snake_case` for struct fields: `is_authenticated`, `emails_metadata`, `message_id`, `message_size`, `message_uid`, `num_email`, `mailbox_size`

**Constants / Statics:**
- `SCREAMING_SNAKE_CASE` for `lazy_static!` regex constants: `ENDING_REGEX`, `OK_REGEX`, `ERR_REGEX`, `STAT_REGEX`, `MESSAGE_DATA_UIDL_ALL_REGEX`, `MESSAGE_DATA_UIDL_ONE_REGEX`, `MESSAGE_DATA_LIST_ALL_REGEX`

**Files:**
- Single file library: `src/pop3.rs`
- Example binary at crate root: `example.rs`

## Code Style

**Formatting:**
- No `rustfmt.toml` detected; formatting is inconsistent in the existing code
- Mixed indentation: some blocks use tabs (most of the file), some blocks use 4-space indentation (the `uidl` method and UIDL parse functions added later)
- Inconsistent alignment in `match` arms: some branches align arms, others do not
- No enforced formatter configuration in the repository

**Linting:**
- No `clippy.toml` detected; no explicit Clippy configuration
- Crate-level attributes at file top: `#![crate_name = "pop3"]`, `#![crate_type = "lib"]`
- Derives used selectively: `#[derive(Debug)]`, `#[derive(Clone)]`, `#[derive(Clone,Copy,Debug)]`, `#[derive(Clone,Debug)]`, `#[derive(Default)]`

## Import Organization

**Order:**
1. `extern crate` declarations (old Rust 2015 edition style): `openssl`, `regex`, `lazy_static`
2. Enum variant glob imports for ergonomics: `use POP3StreamTypes::{Basic, Ssl}`, `use POP3Command::{Greet, User, ...}`
3. Standard library imports: `std::string::String`, `std::io::prelude::*`, `std::io::{Error, ErrorKind, Result}`, `std::net::{ToSocketAddrs, TcpStream}`, `std::str::FromStr`, `std::str`
4. External crate imports: `openssl::ssl::{SslConnector, SslStream}`, `regex::Regex`

**Path Aliases:**
- None used; all imports are full paths

## Error Handling

**Pattern:**
- Public API methods return `POP3Result` (a custom enum), NOT `Result<T, E>` - errors are represented as `POP3Result::POP3Err`
- The only `Result`-returning function is the internal `read_response` and the public `connect`
- `panic!` is used extensively for write errors in public methods: `panic!("Error writing")` — this is the dominant error handling strategy for I/O failures
- `panic!` is also used for authentication guard failures: `panic!("login")`, `panic!("Not Logged In")`
- `unwrap()` used freely on regex captures and `FromStr` parses inside private parse functions
- The `read_response` inner loop silently ignores read byte errors with `println!("Error Reading!")`
- Authentication is checked via a guard pattern at the top of each method:
  ```rust
  if !self.is_authenticated {
      panic!("login");
  }
  ```

**Return Value Pattern:**
- All public POP3 operations return `POP3Result` directly (not `Result<POP3Result, E>`)
- Callers must pattern-match on `POP3Result` variants to check success vs. failure
- `POP3Result::POP3Err` is the generic failure sentinel

## Logging

**Framework:** `println!` macro (no logging crate)

**Patterns:**
- Errors during response reading printed inline: `println!("Error Reading!")`
- No structured logging; no log levels; no log crate dependency

## Comments

**When to Comment:**
- Doc comments (`///`) on all public types and public methods
- Inline comments used sparingly for protocol-level constants: `//Carriage return`, `//Line Feed`
- Inline comments used for intent clarification: `//We are retreiving status line`, `//Send user command`
- No module-level doc comment (`//!`)

**Doc Comment Style:**
- Single-line `///` doc comments placed immediately before the item
- Examples: `/// Creates a new POP3Stream.`, `/// Login to the POP3 server.`, `/// Wrapper for a regular TcpStream or a SslStream.`
- No `# Examples` or `# Panics` sections documented despite heavy use of `panic!`

## Function Design

**Size:** Methods are medium-length (15–40 lines). `read_response` is the longest at ~30 lines. `add_line` is the most complex at ~55 lines.

**Parameters:**
- Methods take `&mut self` for stateful operations
- `connect` uses a generic bound `<A: ToSocketAddrs>` for flexible address input
- Optional parameters use `Option<i32>`: `uidl(message_number: Option<i32>)`, `list(message_number: Option<i32>)`

**Return Values:**
- Public methods return `POP3Result`
- Private helpers mutate `self` directly (e.g., `parse_stat`, `parse_message` set `self.result`)
- `connect` returns `Result<POP3Stream>` (the only method that propagates errors via `Result`)

## Module Design

**Exports:**
- Public items: `POP3Stream`, `POP3Result` (and its variants), `POP3EmailMetadata`, `POP3EmailUidldata`
- Internal items (private): `POP3StreamTypes`, `POP3Command`, `POP3Response`
- No explicit `pub use` re-exports; consumers import directly from `pop3::` namespace

**Barrel Files:**
- Not applicable — single-file library with no submodules

## Regex Pattern Usage

**Pattern:**
- All regex patterns compiled once at startup via `lazy_static!` block in `src/pop3.rs` (lines 21–29)
- Regex variables are module-level statics, accessed by reference throughout the file
- All regex patterns use `unwrap()` on compile — a panic at startup if patterns are malformed

---

*Convention analysis: 2026-03-01*
