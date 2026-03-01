# Phase 1: Foundation - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Fix known bugs, establish error handling, and build test infrastructure. The v2.0-async-rewrite branch has already implemented 8 of 9 requirements (bugs fixed, panics eliminated, error types defined, edition 2021). The remaining work is QUAL-01: end-to-end mock I/O tests that prove the fixes hold by scripting server interactions, plus adding an `AuthFailed` error variant.

</domain>

<decisions>
## Implementation Decisions

### Test coverage depth
- Comprehensive mock I/O tests for ALL POP3 commands (stat, list, retr, dele, uidl, rset, noop, quit, login), not just the 4 bug-fix proofs
- Both happy path and error path (-ERR response) tests for each command
- Wire-level verification: inspect the write buffer to confirm exact command bytes sent (e.g., assert buffer contains `b"RSET\r\n"` not `b"RETR\r\n"`)
- This gives a full safety net before Phase 2's async rewrite touches every method

### Error variant addition
- Add `AuthFailed(String)` variant to `Pop3Error` enum
- Carries the server's -ERR text as payload (e.g., "invalid password")
- Only returned from `login()` when the server rejects USER or PASS commands
- `NotAuthenticated` remains for client-side guard (calling commands while not authenticated)
- No other error variants added in Phase 1 — Timeout and ConnectionClosed wait for later phases

### API naming
- Keep current v2 naming conventions: `Pop3Client`, `Pop3Error`, `ListEntry`, `UidlEntry`, `Stat`, `Message`
- Keep crate name `pop3`
- Keep 5-module structure: client.rs, error.rs, response.rs, transport.rs, types.rs
- Flat re-exports in lib.rs: `use pop3::Pop3Client`, `use pop3::Pop3Error`, etc.

### Claude's Discretion
- Test file location (transport.rs #[cfg(test)] vs tests/ directory vs client.rs tests)
- Mock transport implementation approach (Stream::Mock variant vs trait object vs generic constructor)
- Exact test helper function signatures and organization
- Whether to use a shared test fixture builder or per-test setup

</decisions>

<specifics>
## Specific Ideas

- Wire-level verification is essential for FIX-01 and FIX-02 — the test must show the exact bytes on the write side, not just that the server responded OK
- AuthFailed should be a clean separation from ServerError: login rejection = AuthFailed, other -ERR responses = ServerError
- The 20 existing parser unit tests in response.rs are sufficient for parse coverage — mock I/O tests focus on the send-command/read-response pipeline

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/response.rs`: 20 pure-function parser unit tests covering all response formats (happy + error paths)
- `src/transport.rs`: 1 existing unit test for dot-unstuffing logic
- `src/error.rs`: `Pop3Error` enum with `thiserror 2` derive — add `AuthFailed` variant here
- `src/client.rs`: `Pop3Client` with `login()`, `stat()`, `list()`, `retr()`, `dele()`, `uidl()`, `rset()`, `noop()`, `quit()`

### Established Patterns
- Pure-function parsers: all parsing in response.rs takes `&str` and returns `Result<T, Pop3Error>` — no I/O
- `send_and_check()` pattern in client.rs: sends command, reads response, checks for +OK
- `Transport` wraps a private `Stream` enum with `Plain` and `Tls` variants
- `#[derive(Debug, thiserror::Error)]` pattern for error types

### Integration Points
- `Transport::send_command(&mut self, cmd: &str)` — writes `{cmd}\r\n` to the stream
- `Transport::read_line(&mut self)` — reads a single CRLF-terminated line
- `Transport::read_multiline(&mut self)` — reads dot-terminated multi-line response
- `Stream` enum (private) — needs a `Mock` variant for test injection

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 01-foundation*
*Context gathered: 2026-03-01*
