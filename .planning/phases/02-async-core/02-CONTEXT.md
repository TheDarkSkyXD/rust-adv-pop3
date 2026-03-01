# Phase 2: Async Core - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Migrate all I/O to async/await using tokio. All public API methods become async and work over a plain TCP connection. Developers can connect, authenticate, and run every v1.0.6 command against a real server with no blocking calls. TLS backends, CAPA, TOP, builder pattern, and APOP are separate phases.

</domain>

<decisions>
## Implementation Decisions

### Client Ownership & quit()
- quit() takes `self` by value (move semantics) — consuming the client so the compiler rejects any use after disconnect
- No auto-QUIT on Drop — if caller drops without quit(), TCP connection closes silently and DELE marks are NOT committed (safer default)
- connect() and login() remain separate steps — preserves POP3 protocol phase separation and allows callers to call capa() before authenticating
- quit() return type: Claude's Discretion

### Timeout Configuration
- Pop3Error::Timeout dedicated variant — callers can match specifically on timeout vs other I/O errors for retry logic
- Timeouts set at connect time only, immutable after connection — prevents mid-session confusion
- Single vs separate read/write timeouts: Claude's Discretion
- How timeouts are passed to connect(): Claude's Discretion (must be forward-compatible with Phase 4 builder)

### Session State
- Public `SessionState` enum: `Connected`, `Authenticated`, `Disconnected` — callers can match on it, forward-compatible with Phase 7 reconnect state surfacing
- Public read-only accessor (e.g., `state() -> SessionState`) on the client
- login() only callable when in Connected state — calling when already authenticated returns an error
- Internal enum shape (whether to add granular RFC 1939 states like Greeting/Update): Claude's Discretion

### CI Setup
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

</decisions>

<specifics>
## Specific Ideas

- Move semantics on quit() is the key API ergonomics goal — compile-time use-after-disconnect prevention
- Keep the API simple for Phase 2 — no builder, no config struct, just connect() with sensible defaults
- Phase 2 is plain TCP only — TLS is Phase 3, so the transport layer should be designed for easy TLS addition later

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Transport` struct (src/transport.rs): Already abstracts over stream types via `Stream` enum. Needs async conversion but the pattern (Plain/Tls/Mock variants) carries forward.
- `response` module (src/response.rs): Pure parsing functions (parse_stat, parse_list_single, etc.) — these are I/O-independent and can be reused as-is.
- `Pop3Error` enum (src/error.rs): Already covers Io, ServerError, Parse, AuthFailed, NotAuthenticated, InvalidInput, InvalidDnsName. Needs Timeout variant added.
- `types` module (src/types.rs): Stat, ListEntry, UidlEntry, Message, Capability structs — reusable as-is.
- Mock transport pattern (transport.rs lines 166-180): Rc<RefCell<Vec<u8>>> writer pattern for test inspection.

### Established Patterns
- send_and_check() pattern: Send command, read status line, parse. Carries directly to async with .await added.
- require_auth() guard: Called at top of authenticated methods. Will use SessionState enum instead of bool.
- CRLF injection check: check_no_crlf() helper — pure function, no change needed.
- Dot-unstuffing in read_multiline(): Logic correct, needs async read conversion.

### Integration Points
- `Pop3Client::connect()` entry point in client.rs — becomes async, returns the client
- `Transport` methods (send_command, read_line, read_multiline) — all become async
- lib.rs re-exports — need to expose SessionState in addition to existing types
- Cargo.toml — needs tokio dependency added

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 02-async-core*
*Context gathered: 2026-03-01*
