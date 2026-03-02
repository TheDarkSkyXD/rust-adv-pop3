# Phase 4: Protocol Extensions - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Add APOP authentication, structured RESP-CODES error parsing, and a fluent `Pop3ClientBuilder` API — rounding out the v2.x feature set. Requirements: CMD-03, CMD-04, API-01, API-02.

</domain>

<decisions>
## Implementation Decisions

### Builder API Surface
- **Consuming chain style** — builder methods return `Self` (owned), enabling one-liner fluent chains like `Pop3ClientBuilder::new("host").tls().credentials("u","p").connect().await?`
- **Auto-auth support** — builder accepts optional credentials via `.credentials(user, pass)` for USER/PASS and `.apop(user, pass)` for APOP. If set, `connect()` authenticates before returning
- **Same return type** — `connect()` always returns `Pop3Client`. When credentials are set, the client is already in `SessionState::Authenticated`
- **Smart port defaults** — `new(host)` without explicit port uses 110 for plain/STARTTLS, 995 for TLS-on-connect. `.port()` method available to override
- **STARTTLS in builder** — `.starttls()` method triggers connect-plain → STLS-upgrade → (optional auth) flow during `connect()`
- **TLS mode precedence** — if both `.tls()` and `.starttls()` are called, last one wins (internal enum tracks mode)
- **Auto SNI** — hostname from `new(host)` is reused for TLS SNI verification; no separate `.tls_hostname()` method

### APOP Exposure
- **Dual availability** — APOP is selectable via builder (`.apop(user, pass)`) AND as standalone `client.apop(user, pass)` method
- **`#[deprecated]` attribute** — `apop()` method carries `#[deprecated(note = "...")]` producing a compiler warning on every call site, plus a prominent `# Security Warning` rustdoc section documenting MD5 is broken
- **No silent fallback** — if APOP is explicitly requested (via builder or method) but server greeting has no timestamp, return `Pop3Error` immediately. Do NOT fall back to USER/PASS
- **Separate methods** — `.credentials()` for USER/PASS and `.apop()` for APOP, not an enum-based `.auth()` method

### Error Variant Design
- **4 new RESP-CODE variants** — `MailboxInUse`, `LoginDelay`, `SysTemp`, `SysPerm` added to `Pop3Error`
- **`[AUTH]` maps to `AuthFailed`** — no separate `AuthError` variant. The `[AUTH]` RESP-CODE is semantically identical to `AuthFailed`, so it merges directly
- **Unknown codes fall through** — unrecognized RESP-CODEs (not in IANA list) fall through to `ServerError(String)` with full text
- **`login()` updated** — must handle the case where `parse_status_line` now returns `AuthFailed` (from `[AUTH]`) instead of `ServerError`

### Claude's Discretion
- Constructor naming (whether to add convenience constructors like `::plain()` / `::tls()`)
- UTF8 RESP-CODE inclusion (6th IANA code — rarely seen in practice)
- Whether RESP-CODE variants strip the bracketed code from the message string or preserve it
- Exact `#[deprecated]` message wording for APOP

</decisions>

<specifics>
## Specific Ideas

- Builder should feel like `reqwest::ClientBuilder` — consuming chain, fluent one-liners
- APOP compiler warning is intentionally aggressive — this is a legacy mechanism and users should know it

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Pop3Client::greeting` field (String) — already stores server greeting, ready for APOP timestamp extraction via `find('<')`
- `SessionState` enum — tracks Connected/Authenticated/Disconnected, builder uses this to signal auth completion
- `check_no_crlf()` helper — existing CRLF injection protection, reuse for APOP username/password validation
- `parse_status_line()` in `response.rs` — pure function that returns `Result<&str>`, RESP-CODES parsing extends this directly
- `connect_tls()` / `connect_openssl_tls()` / `stls()` methods — builder wraps these existing methods

### Established Patterns
- `thiserror` derive on `Pop3Error` — new variants follow the same pattern with `#[error("...")]` and `String` payloads
- `login()` promotes `ServerError` → `AuthFailed` — `apop()` should use the same pattern, and both must handle RESP-CODE variants after Phase 4
- Feature-gated TLS: `#[cfg(feature = "rustls-tls")]` and `#[cfg(feature = "openssl-tls")]` — builder TLS methods follow this pattern
- `pub use` in `lib.rs` — `Pop3ClientBuilder` gets re-exported here

### Integration Points
- `lib.rs` — add `mod builder; pub use builder::Pop3ClientBuilder;`
- `error.rs` — add 4 new `Pop3Error` variants
- `response.rs` — extend `parse_status_line()` with `parse_resp_code()` helper
- `client.rs` — add `apop()` method, update `login()` error handling for RESP-CODE interaction

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 04-protocol-extensions*
*Context gathered: 2026-03-01*
