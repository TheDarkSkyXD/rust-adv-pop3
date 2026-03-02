# Phase 9: MIME Integration - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Add optional MIME parsing via `mail-parser` behind a `mime` feature flag. Callers can retrieve and parse a message's MIME structure in one call (`retr_parsed()`) without manually handling raw RFC 5322 bytes. This phase does NOT add batch parsed methods, custom MIME types, or attachment extraction utilities.

</domain>

<decisions>
## Implementation Decisions

### Return type design
- Type alias: `pub type ParsedMessage = mail_parser::Message<'static>` — exposes mail-parser's full API directly
- Re-export from crate root: `pop3::ParsedMessage` behind `#[cfg(feature = "mime")]` — consistent with all other public types
- Do NOT re-export `mail_parser` itself — callers add it to their own Cargo.toml if they need deeper types (Address, HeaderValue, etc.)
- Document compatible mail-parser version in rustdoc (e.g., "This is `mail_parser::Message<'static>` from mail-parser 0.11.x")

### Method surface
- Include both `retr_parsed()` and `top_parsed()` — TOP also returns RFC 5322 content and parsing headers is a natural use case
- Naming: `retr_parsed()` / `top_parsed()` — describes what the method does, not tied to "MIME" which is inaccurate for plain-text messages
- `top_parsed()` rustdoc must warn that body may be truncated to N lines per the TOP command
- Batch parsed methods (`retr_many_parsed`) are deferred — see Deferred Ideas

### Partial parse handling
- Best-effort parsing: follow mail-parser's default (return Ok as long as some headers found; only Err for completely unparseable content)
- `Pop3Error::MimeParse(String)` error message includes message ID only: "message 5 could not be parsed as RFC 5322"
- `MimeParse` variant exists unconditionally in `Pop3Error` (not feature-gated) — correct for exhaustive matching across feature combinations
- Document the two-error model in rustdoc: network/protocol errors from underlying retr()/top(), MimeParse means retrieval succeeded but content isn't valid email

### Examples and docs
- Add standalone `examples/mime.rs` with `required-features = ["mime"]` — connect, retr_parsed, access fields, quit
- Rustdoc shows common parsed fields: `subject()`, `from()`, `body_text(0)`, `body_html(0)`, `attachment_count()` + links to mail-parser docs for full API
- Add `mime` feature to CI test matrix (rustls-tls + mime combination)
- Update lib.rs crate-level feature table to list: `| mime | No | Optional MIME parsing via mail-parser |`

### Claude's Discretion
- Exact CI matrix entry structure (which TLS backend to combine with mime)
- Doctest format (no_run vs ignore)
- Internal code organization (inline in client.rs vs separate mime.rs module)
- Error variant placement order in Pop3Error enum

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches. Research already provides a complete implementation pattern.

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `retr()` / `top()` in `client.rs`: Return `Message { data: String }` — the raw content that feeds into mail-parser
- Feature-flag pattern in `Cargo.toml`: `dep:` syntax for `rustls-tls`, `openssl-tls`, `pool` — exact pattern to follow
- `build_authenticated_test_client()` helper: Creates mock client for unit tests — same helper for mime tests
- `Pop3Error` enum with `thiserror`: 14 variants — adding `MimeParse(String)` is consistent

### Established Patterns
- Feature-gated `pub use` in `lib.rs`: Pool module uses `#[cfg(feature = "pool")]` — same pattern for mime re-exports
- Feature-gated impl blocks: Not yet used on `Pop3Client` but is standard Rust pattern documented in research
- `[[example]]` entries in `Cargo.toml` with `required-features`: Already used for `tls.rs` and `starttls.rs`
- Type aliases in `types.rs`: Not yet used but natural home for `ParsedMessage`

### Integration Points
- `src/error.rs`: Add `MimeParse(String)` variant (unconditional)
- `src/types.rs`: Add `pub type ParsedMessage` (feature-gated)
- `src/client.rs`: Add `#[cfg(feature = "mime")] impl Pop3Client` block with `retr_parsed()` and `top_parsed()`
- `src/lib.rs`: Add feature-gated `pub use types::ParsedMessage` + update crate docs
- `Cargo.toml`: Add `mail-parser` optional dep + `mime` feature
- `.github/workflows/rust.yml`: Add mime to CI matrix

</code_context>

<deferred>
## Deferred Ideas

- `retr_many_parsed()` / `dele_many_parsed()` batch methods — adds complexity with partial failure handling; defer to future iteration after Phase 9 ships

</deferred>

---

*Phase: 09-mime-integration*
*Context gathered: 2026-03-01*
