# Phase 9: MIME Integration - Research

**Researched:** 2026-03-01
**Domain:** Rust optional-dependency feature flags + mail-parser crate API
**Confidence:** HIGH

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MIME-01 | Client provides `retr_parsed()` method behind a `mime` feature flag | Feature-flag pattern (dep: syntax + #[cfg(feature)]) documented; impl block gating is standard Rust |
| MIME-02 | MIME integration uses `mail-parser` crate (zero external deps, RFC 5322 + MIME conformant) | mail-parser 0.11.2 confirmed; NOTE: it now has one mandatory dep (hashify, a proc-macro — no runtime overhead) |
</phase_requirements>

---

## Summary

Phase 9 adds optional MIME parsing to the POP3 client via the `mail-parser` crate. The core mechanic is simple: `retr()` already returns a dot-unstuffed `String`; `retr_parsed()` calls `retr()`, converts the string to bytes, feeds those bytes to `MessageParser::default().parse()`, and returns either a parsed `Message<'static>` (after calling `.into_owned()`) or a `Pop3Error::MimeParse` error.

The lifetime challenge — `Message<'x>` borrows from its input — is already solved by mail-parser: `Message::into_owned()` converts a `Message<'x>` to `Message<'static>` by cloning all `Cow`-wrapped fields. This means `retr_parsed()` can return an owned value with no self-referential struct complexity.

The `mime` feature flag follows the exact same `dep:` pattern the codebase already uses for TLS backends. The only structural addition is: a `mime = ["dep:mail-parser"]` entry in `[features]`, an optional dependency declaration, a new `#[cfg(feature = "mime")]` impl block on `Pop3Client`, and a corresponding `#[cfg(feature = "mime")]` test block.

**Primary recommendation:** Call `retr()`, call `.as_bytes()` on `Message.data`, pass to `MessageParser::default().parse()`, call `.into_owned()` on the result, propagate `None` as `Pop3Error::MimeParse`. Wrap in a `#[cfg(feature = "mime")]` impl block. No new types beyond `Pop3Error::MimeParse` are needed in non-feature code paths.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| mail-parser | 0.11.2 | RFC 5322 + MIME parsing | Required by MIME-02; battle-tested with millions of real emails; used in production by Stalwart Mail Server |

### Dependencies Pulled In by mail-parser

| Library | Version | Type | Notes |
|---------|---------|------|-------|
| hashify | 0.2 | Required (proc-macro) | Generates perfect hash maps at compile time — zero runtime overhead; pure Rust |
| encoding_rs | 0.8 | Optional (feature: `full_encoding`) | Extended charset support — do NOT enable unless needed |
| serde | 1.0 | Optional (feature: `serde`) | Serialization — do NOT enable |
| rkyv | 0.8 | Optional (feature: `rkyv`) | Serialization — do NOT enable |

### Dependency Note on MIME-02's "Zero External Deps" Claim

REQUIREMENTS.md states "zero external deps" for mail-parser. This was accurate for mail-parser < 0.11. **Version 0.11.x adds `hashify` as a required dependency.** hashify is a proc-macro crate (pure Rust, no system libs, no C FFI) that generates code at compile time — it has zero runtime overhead and zero binary size impact. The spirit of MIME-02 (no heavy system dependencies, pure Rust) is preserved. Acknowledge this in plan commentary.

### Installation

```toml
# In Cargo.toml [dependencies]:
mail-parser = { version = "0.11", optional = true }

# In [features]:
mime = ["dep:mail-parser"]
```

---

## Architecture Patterns

### Recommended Project Structure

No new source files are needed. All additions go into existing files:

```
src/
├── lib.rs          → Add `pub use` for ParsedMessage type alias + Pop3Error::MimeParse re-export (feature-gated)
├── client.rs       → Add #[cfg(feature = "mime")] impl Pop3Client block with retr_parsed()
├── error.rs        → Add MimeParse(String) variant to Pop3Error (always present — does NOT need feature gate)
└── types.rs        → Add `pub type ParsedMessage = mail_parser::Message<'static>;` (feature-gated)
```

Optional: create `src/mime.rs` if the impl block grows large, but for a single method it is unnecessary.

### Pattern 1: Feature-Gated Optional Dependency (Cargo.toml)

**What:** `dep:` prefix prevents implicit feature creation; keeps the internal library name hidden from callers
**When to use:** Any optional dependency that is an implementation detail, not a user-facing feature by name

```toml
# Cargo.toml

[dependencies]
mail-parser = { version = "0.11", optional = true }

[features]
default = ["rustls-tls"]
rustls-tls = ["dep:tokio-rustls", "dep:rustls-native-certs"]
openssl-tls = ["dep:tokio-openssl", "dep:openssl"]
mime = ["dep:mail-parser"]
```

### Pattern 2: Feature-Gated Impl Block on Existing Struct

**What:** A `#[cfg(feature = "mime")]` attribute gates an entire `impl Pop3Client` block
**When to use:** When adding methods that only exist when an optional dep is active

```rust
// Source: https://doc.rust-lang.org/reference/conditional-compilation.html
// In src/client.rs

#[cfg(feature = "mime")]
impl Pop3Client {
    /// Retrieve and parse a message as a structured MIME object.
    ///
    /// Requires the `mime` feature flag.
    ///
    /// # Errors
    ///
    /// Returns [`Pop3Error::MimeParse`] if the message bytes cannot be
    /// parsed as a valid RFC 5322 message (e.g., no headers found).
    pub async fn retr_parsed(
        &mut self,
        message_id: u32,
    ) -> crate::Result<mail_parser::Message<'static>> {
        let msg = self.retr(message_id).await?;
        mail_parser::MessageParser::default()
            .parse(msg.data.as_bytes())
            .map(|m| m.into_owned())
            .ok_or_else(|| Pop3Error::MimeParse(
                format!("message {message_id} could not be parsed as RFC 5322")
            ))
    }
}
```

### Pattern 3: Type Alias for Public API Ergonomics

**What:** Re-export the mail-parser `Message<'static>` under a project-local name
**When to use:** Prevents leaking the `mail_parser::` path into user-facing docs; callers use `pop3::ParsedMessage`

```rust
// Source: standard Rust re-export pattern
// In src/types.rs (behind feature gate)

#[cfg(feature = "mime")]
pub type ParsedMessage = mail_parser::Message<'static>;
```

```rust
// In src/lib.rs (behind feature gate)

#[cfg(feature = "mime")]
pub use types::ParsedMessage;
```

### Pattern 4: Parse Failure Error Variant

**What:** `Pop3Error::MimeParse(String)` — a distinct variant for MIME-specific parse failures
**When to use:** Always add to error.rs (unconditionally — not feature-gated); callers can match on it even if they are not using the `mime` feature at all

```rust
// In src/error.rs — no #[cfg] needed

/// Failed to parse the message as a valid RFC 5322 / MIME structure.
///
/// Returned by [`Pop3Client::retr_parsed()`](crate::Pop3Client::retr_parsed)
/// when `mail-parser` cannot find any headers in the retrieved bytes.
#[error("MIME parse error: {0}")]
MimeParse(String),
```

### Pattern 5: mail-parser Usage

**What:** Full parse flow from bytes to owned structured message
**Source:** https://docs.rs/mail-parser/0.11.2/mail_parser/

```rust
use mail_parser::MessageParser;

// Input: &[u8] — accepts anything that implements AsRef<[u8]>
let raw: &[u8] = b"From: alice@example.com\r\nSubject: Test\r\n\r\nHello\r\n";

// Parse: returns Option<Message<'_>> — borrows from raw
let parsed = MessageParser::default().parse(raw);

// Lift to owned: Message<'static> — no lifetime dependency on raw
let owned: mail_parser::Message<'static> = parsed.unwrap().into_owned();

// Access fields
let subject: Option<&str>                 = owned.subject();
let from:    Option<&mail_parser::Address> = owned.from();
let body:    Option<std::borrow::Cow<str>> = owned.body_text(0);
let html:    Option<std::borrow::Cow<str>> = owned.body_html(0);
let n_att:   usize                         = owned.attachment_count();
```

### Anti-Patterns to Avoid

- **Gating `Pop3Error::MimeParse` behind `#[cfg(feature = "mime")]`:** This is wrong. Error variants must always be present so that callers can match exhaustively regardless of feature combination. Only gate the _methods_ that produce the error.
- **Returning `Message<'x>` with a lifetime tied to a local buffer:** The local `msg.data` String will be dropped; you MUST call `.into_owned()` before returning.
- **Using `unwrap()` on the parse result:** mail-parser returns `None` for totally malformed input. Map to `Pop3Error::MimeParse`.
- **Enabling mail-parser's optional `serde` or `rkyv` features:** These bloat the dependency tree; use the crate with no extra features.
- **Feature-gating public traits or public fields:** Only gate _methods_ (impl blocks), type aliases, and re-exports. Never gate struct fields or trait definitions.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| RFC 5322 header parsing | Custom header splitting | `mail_parser::MessageParser` | Folded headers, encoded-word (RFC 2047), 41 charset encodings, MIRI-tested |
| MIME boundary splitting | Custom multipart parser | `mail_parser::MessageParser` | Nested multipart, non-standard boundaries, content-transfer-encoding |
| Base64 body decoding | Custom base64 decoder | `mail_parser::MessageParser` | Handles partial padding, line breaks, corrupt data gracefully |
| Quoted-printable decoding | Custom QP decoder | `mail_parser::MessageParser` | Handles soft line breaks, mixed content |
| Charset conversion | Custom charset mapping | `mail_parser::MessageParser` | 41 charsets including UTF-7, ISO-2022-JP |

**Key insight:** RFC 5322 email has accumulated 30+ years of quirks, obsolete syntax forms, and non-conformant server behavior. The entire domain is a minefield for hand-rolled parsers. mail-parser has been fuzzed, MIRI-tested, and validated against millions of real-world messages. Use it as a black box.

---

## Common Pitfalls

### Pitfall 1: Returning Borrowed Message Without into_owned()

**What goes wrong:** `retr_parsed()` returns `Message<'x>` where `'x` is tied to a local `String` buffer (`msg.data`). The compiler rejects this with a lifetime error.
**Why it happens:** `MessageParser::parse()` is a zero-copy parser — it stores `Cow::Borrowed` slices into the input. If the input drops at end-of-function, those slices dangle.
**How to avoid:** Always call `.into_owned()` on the parsed `Message<'x>` before returning. This promotes all `Cow::Borrowed` slices to `Cow::Owned` allocations, yielding `Message<'static>`.
**Warning signs:** Compiler error mentioning "borrowed value does not live long enough" inside `retr_parsed`.

### Pitfall 2: Treating parse() Failure as Impossible

**What goes wrong:** Code uses `.unwrap()` or `.expect()` on the `Option<Message<'x>>` returned by `parse()`. Malformed messages (e.g., a POP3 server that returns garbage, or a zero-byte message) cause a panic.
**Why it happens:** The happy path works for well-formed emails; `None` is only returned when no headers at all are found.
**How to avoid:** Always use `.ok_or_else(|| Pop3Error::MimeParse(...))` to convert the `Option` to a `Result`. This is the contract the caller expects from an async library method.
**Warning signs:** Tests only use well-formed fixture emails and never test a "no headers" byte sequence.

### Pitfall 3: Feature Gate on Pop3Error Variant

**What goes wrong:** Developer puts `#[cfg(feature = "mime")]` on the `MimeParse` variant in `error.rs`. Now callers who don't enable `mime` cannot exhaustively match on `Pop3Error` if they ever receive one (even though they can't, the compiler still complains about non-exhaustive matches in code compiled _with_ the feature).
**Why it happens:** Attempting to keep the error type "clean" when mime is disabled.
**How to avoid:** `MimeParse` variant lives unconditionally in `Pop3Error`. It simply cannot be constructed without the `mime` feature (since only `retr_parsed` creates it), but it must always be matchable.

### Pitfall 4: Enabling mail-parser's Full Encoding Feature

**What goes wrong:** Developer accidentally enables `full_encoding` feature (e.g., via `mail-parser = { version = "0.11", features = ["full_encoding"] }`). This pulls in `encoding_rs` which adds ~2MB of charset data tables.
**Why it happens:** Copy-paste from another project's Cargo.toml.
**How to avoid:** Declare `mail-parser` with no explicit features: `mail-parser = { version = "0.11", optional = true }`. The default feature set is empty — only the core parser with the 41 most common charsets.

### Pitfall 5: Forgetting required-features on the Example

**What goes wrong:** Adding an example that uses `retr_parsed()` but not gating the example with `required-features = ["mime"]`. The example fails to compile when `mime` is not enabled (e.g., in the plain `cargo build` step of CI).
**Why it happens:** The pattern already exists for TLS examples, but it's easy to forget when adding a new example.
**How to avoid:** Add `required-features = ["mime"]` to the `[[example]]` entry for any example that uses the `mime` feature.

### Pitfall 6: Lifetime Confusion with body_text() and body_html()

**What goes wrong:** After calling `into_owned()`, code tries to store `body_text(0)` results as `&str` outside the `Message` value. Since `body_text()` returns `Option<Cow<'_, str>>` borrowing from `self`, the borrow checker rejects storing the reference after `self` moves.
**Why it happens:** `Message<'static>` means the data _could_ outlive the struct, but the borrow is still tied to the struct instance lifetime when accessed via methods.
**How to avoid:** Either keep the `Message<'static>` alive for the entire scope where body text is needed, or clone the `Cow` to an owned `String` with `.map(|c| c.into_owned())`.

---

## Code Examples

Verified patterns from official documentation (https://docs.rs/mail-parser/0.11.2/):

### Complete retr_parsed() Implementation

```rust
// Source: https://docs.rs/mail-parser/0.11.2/mail_parser/struct.MessageParser.html
// In src/client.rs

#[cfg(feature = "mime")]
impl Pop3Client {
    pub async fn retr_parsed(
        &mut self,
        message_id: u32,
    ) -> crate::Result<mail_parser::Message<'static>> {
        let msg = self.retr(message_id).await?;
        mail_parser::MessageParser::default()
            .parse(msg.data.as_bytes())
            .map(|m| m.into_owned())
            .ok_or_else(|| {
                crate::error::Pop3Error::MimeParse(format!(
                    "message {message_id} could not be parsed as RFC 5322"
                ))
            })
    }
}
```

### Test Fixture: Minimal RFC 5322 Message

```rust
// Use inline byte literals — no file I/O needed in unit tests
// Source: RFC 5322 Section 3.3 + RFC 1939 test pattern

const MINIMAL_EMAIL: &[u8] = b"\
From: alice@example.com\r\n\
To: bob@example.com\r\n\
Subject: Test Subject\r\n\
Date: Mon, 1 Jan 2024 12:00:00 +0000\r\n\
\r\n\
This is the plain text body.\r\n";

const MULTIPART_EMAIL: &[u8] = b"\
From: alice@example.com\r\n\
To: bob@example.com\r\n\
Subject: MIME test\r\n\
Content-Type: multipart/alternative; boundary=\"boundary42\"\r\n\
\r\n\
--boundary42\r\n\
Content-Type: text/plain\r\n\
\r\n\
Plain text body\r\n\
--boundary42\r\n\
Content-Type: text/html\r\n\
\r\n\
<html><body>HTML body</body></html>\r\n\
--boundary42--\r\n";
```

### Test: retr_parsed() Happy Path

```rust
// In src/client.rs #[cfg(test)] block

#[cfg(feature = "mime")]
#[tokio::test]
async fn retr_parsed_returns_structured_message() {
    // Build mock: server returns an RFC 5322 message after RETR
    // Note: dot-stuffed boundary in transport layer, un-stuffed by retr()
    let mock = Builder::new()
        .write(b"RETR 1\r\n")
        .read(
            b"+OK\r\n\
From: alice@example.com\r\n\
Subject: Hello\r\n\
\r\n\
Body text\r\n\
.\r\n",
        )
        .build();
    let mut client = build_authenticated_test_client(mock);
    let parsed = client.retr_parsed(1).await.unwrap();
    assert_eq!(parsed.subject(), Some("Hello"));
    assert!(parsed.body_text(0).is_some());
}

#[cfg(feature = "mime")]
#[tokio::test]
async fn retr_parsed_returns_error_for_malformed_message() {
    // Message with no headers at all -> parse() returns None -> MimeParse error
    let mock = Builder::new()
        .write(b"RETR 1\r\n")
        // Empty body (just CRLF dot terminator, no headers)
        .read(b"+OK\r\n\r\n.\r\n")
        .build();
    let mut client = build_authenticated_test_client(mock);
    let result = client.retr_parsed(1).await;
    assert!(matches!(result, Err(Pop3Error::MimeParse(_))));
}
```

### Cargo.toml Changes

```toml
[dependencies]
# ... existing deps ...
mail-parser = { version = "0.11", optional = true }

[features]
default = ["rustls-tls"]
rustls-tls = ["dep:tokio-rustls", "dep:rustls-native-certs"]
openssl-tls = ["dep:tokio-openssl", "dep:openssl"]
mime = ["dep:mail-parser"]
```

### CI Matrix Addition

```yaml
# In .github/workflows/rust.yml — add mime to matrix

- features: "rustls-tls,mime"
  name: "rustls-tls + mime"
```

### Doctest (no_run — no real server needed)

```rust
/// # Example
///
/// ```no_run
/// # #[cfg(feature = "mime")]
/// # async fn example() -> pop3::Result<()> {
/// use pop3::Pop3Client;
///
/// let mut client = Pop3Client::connect(("pop.example.com", 110),
///     std::time::Duration::from_secs(30)).await?;
/// client.login("user", "pass").await?;
/// let msg = client.retr_parsed(1).await?;
/// println!("Subject: {:?}", msg.subject());
/// println!("Body: {:?}", msg.body_text(0));
/// client.quit().await?;
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "mime")]
pub async fn retr_parsed(&mut self, message_id: u32) -> crate::Result<mail_parser::Message<'static>> {
    // ...
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `mailparse` (staktrace) | `mail-parser` (stalwartlabs) | ~2022 onward | mail-parser is faster, MIRI-tested, used in production mail server |
| `mail-parser` 0.9.x (zero deps) | `mail-parser` 0.11.x (hashify dep) | Jan 2025 | hashify is a proc-macro, zero runtime overhead; not a concern |
| `Message<'x>` without into_owned() | `Message::into_owned() -> Message<'static>` | Added in 0.9.x | Eliminates the only lifetime obstacle for returning a parsed message |
| Feature-flag via implicit dep feature | `dep:` prefix (RFC 3143) | Rust 1.60 (2022) | Prevents accidental exposure of internal dep as a user-facing feature |

**Deprecated/outdated:**
- `mailparse` crate: Older, slower, uses `ParsedMail<'_>` with no `into_owned()`, would require the self-referential struct workaround. Do not use.
- `lettre` / `email` crates for parsing: These are mail-sending libraries, not parsers. Do not use.

---

## Open Questions

1. **Should `ParsedMessage` be a type alias or a newtype wrapper?**
   - What we know: A type alias (`pub type ParsedMessage = mail_parser::Message<'static>`) exposes mail-parser's type directly; a newtype wrapper hides it but requires forwarding methods.
   - What's unclear: Whether callers will want to call mail-parser methods directly or via our API.
   - Recommendation: Start with a type alias. It is simpler, avoids a forwarding burden, and lets callers access the full mail-parser API. Document clearly that `ParsedMessage` is `mail_parser::Message<'static>`. A newtype can always be added in a future semver-compatible release.

2. **Should malformed-but-partially-parsed messages succeed or fail?**
   - What we know: `MessageParser::parse()` returns `None` only when _no_ headers are found. A message with some unparseable headers but a recognizable From/Subject will return `Some(...)`.
   - What's unclear: Whether callers prefer leniency (best-effort) or strictness.
   - Recommendation: Accept mail-parser's default behavior (return `Some` with best-effort results; only `None` for completely unparseable). Document this in the method's rustdoc.

3. **Does the `mime` feature interact with `rustls-tls` / `openssl-tls` mutex?**
   - What we know: The existing `compile_error!` only checks `rustls-tls && openssl-tls`. The `mime` feature is independent.
   - What's unclear: Nothing — `mime` is orthogonal to TLS selection.
   - Recommendation: No `compile_error!` guard needed for `mime`. It can be combined with either TLS backend (or neither).

---

## Validation Architecture

> `workflow.nyquist_validation` is not set to `true` in `.planning/config.json` — this section is included for reference only.

The existing test infrastructure (inline `#[cfg(test)]` + `tokio_test::io::Builder`) supports all needed test patterns without additional framework changes. Tests for `retr_parsed()` use the same `build_authenticated_test_client()` helper as all other client tests. Gate test functions with `#[cfg(feature = "mime")]` to match the production code gate.

---

## Sources

### Primary (HIGH confidence)

- `https://docs.rs/mail-parser/0.11.2/mail_parser/struct.MessageParser.html` — `parse()` method signature, `MessageParser::default()`, builder methods
- `https://docs.rs/mail-parser/0.11.2/mail_parser/struct.Message.html` — `Message<'x>` struct definition, `into_owned()` signature, all accessor methods
- `https://doc.rust-lang.org/cargo/reference/features.html` — `dep:` syntax, optional dependency patterns, best practices for library crates
- `https://doc.rust-lang.org/reference/conditional-compilation.html` — `#[cfg(feature = "...")]` on impl blocks
- `https://github.com/stalwartlabs/mail-parser/blob/main/Cargo.toml` — version 0.11.2, required `hashify` dep, optional `encoding_rs`/`serde`/`rkyv` deps, empty default features

### Secondary (MEDIUM confidence)

- `https://lib.rs/crates/mail-parser` — version 0.11.2 confirmed released Feb 14, 2026; ~134K downloads/month
- `https://lib.rs/crates/hashify` — hashify is a proc-macro crate (pure Rust, no runtime deps)
- Multiple web search results confirming mail-parser's zero-copy design with `Cow<'x, [u8]>` and into_owned() availability

### Tertiary (LOW confidence)

- `https://stalwartlabs.medium.com/parsing-mime-e-mail-messages-in-rust-8095d4b1ee5c` — Parsing MIME messages article (blocked by 403); findings corroborated by docs.rs

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — mail-parser version, API signatures, and dependency list verified directly from docs.rs and lib.rs
- Architecture: HIGH — feature flag patterns verified from official Cargo docs; `into_owned()` signature confirmed from docs.rs
- Pitfalls: HIGH — lifetime pitfall (Pitfall 1) directly verified from API signature; others are logical consequences of confirmed facts
- Cargo.toml changes: HIGH — `dep:` syntax verified from official Cargo reference; identical pattern already in use in this codebase

**Research date:** 2026-03-01
**Valid until:** 2026-09-01 (stable ecosystem; mail-parser is production-grade and not likely to change the parse() signature)
