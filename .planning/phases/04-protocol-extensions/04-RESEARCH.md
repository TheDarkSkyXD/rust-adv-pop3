# Phase 4: Protocol Extensions - Research

**Researched:** 2026-03-01
**Domain:** POP3 protocol extensions — APOP authentication, RFC 2449 RESP-CODES, Rust builder pattern
**Confidence:** HIGH

---

## Summary

Phase 4 closes the remaining four v2.0 requirements: APOP authentication (CMD-03), structured RESP-CODES parsing (CMD-04), and a fluent `Pop3ClientBuilder` API (API-01, API-02). All three are self-contained additions that layer on top of the existing `Pop3Client` without changing its internal session-state machine or transport. No new async machinery is needed — APOP is a pure command addition, RESP-CODES is a pure response-parsing addition, and the builder is a synchronous configuration struct with one `async fn connect()` terminal method.

The most critical correctness trap is APOP timestamp extraction: the greeting timestamp can appear anywhere in the greeting line (not just immediately after `+OK`). This is a real-world interoperability issue documented in MailKit's issue tracker. The timestamp must be found by searching for the first `<...>` bracket pair anywhere in the greeting string, not by expecting a fixed position. The MD5 digest is computed over the full timestamp string (including angle brackets) concatenated with the password — use the `md5` 0.8.0 crate (`md5::compute()` returning a `Digest` formatted with `{:x}`). Rustdoc for `apop()` MUST include a security caveat: MD5 is cryptographically broken and APOP provides no protection against offline dictionary attacks if the traffic is intercepted.

RESP-CODES parsing is a straightforward extension of the existing `parse_status_line()` function in `response.rs`. When a `-ERR` response contains text beginning with `[`, extract the bracketed code and map it to a named `Pop3Error` variant. Six codes are IANA-registered (LOGIN-DELAY, IN-USE, SYS/TEMP, SYS/PERM, AUTH, UTF8); add named variants for all five practical ones and preserve `ServerError(String)` as the fallback.

The `Pop3ClientBuilder` follows Rust's non-consuming builder idiom (methods take `&mut self`, return `&mut Self`). The terminal `async fn connect()` is the sole async operation. Feature-flag-gated TLS methods appear on the builder only when the relevant feature is active, which is how the builder satisfies API-02 (hiding TLS flag complexity).

**Primary recommendation:** Implement in this order — (1) APOP in `client.rs` + `response.rs` with the md5 crate, (2) RESP-CODES as new `Pop3Error` variants + updated `parse_status_line`, (3) `Pop3ClientBuilder` in a new `builder.rs` with `pub use` in `lib.rs`.

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CMD-03 | User can authenticate via APOP (with documented MD5 security caveat) | APOP command format from RFC 1939; md5 crate 0.8.0 API; greeting timestamp extraction pitfall; doctest pattern |
| CMD-04 | Server RESP-CODES are parsed into structured `Pop3Error` variants | RFC 2449 and RFC 3206 RESP-CODES definitions; IANA registry; parse_status_line extension pattern |
| API-01 | `Pop3ClientBuilder` provides a fluent interface for connection configuration | Rust non-consuming builder pattern; `&mut Self` return; terminal `async fn connect()` |
| API-02 | Builder hides TLS feature flag complexity from callers | `#[cfg(feature)]` on builder methods pattern; reqwest precedent; compile-time API gating |
</phase_requirements>

---

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| md5 | 0.8.0 | MD5 digest computation for APOP | Dedicated crate; pure Rust; `compute()` + `{:x}` format gives lowercase hex directly; zero transitive deps |
| thiserror | 2 (already in Cargo.toml) | New `Pop3Error` variants for RESP-CODES | Already used; no change needed |

### No New Supporting Libraries Needed
RESP-CODES parsing and the builder pattern require only std + existing dependencies. The builder struct holds `String` + `Duration` + `bool` fields — no additional crates.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| md5 0.8.0 | md-5 0.10.6 (RustCrypto) | md-5 uses the RustCrypto `Digest` trait ecosystem; heavier API for a one-function need. md5 0.8.0 is simpler and purpose-fit for legacy interop |
| md5 crate | std-only hex + manual computation | Hand-rolling MD5 is a prohibited anti-pattern — the algorithm has significant implementation complexity |

**Installation:**
```bash
# Add to [dependencies] in Cargo.toml:
# md5 = "0.8"
```

---

## Architecture Patterns

### Recommended File Changes
```
src/
├── lib.rs          → Add: pub use builder::Pop3ClientBuilder
├── client.rs       → Add: pub async fn apop()
├── response.rs     → Add: parse_resp_code(), update parse_status_line()
├── error.rs        → Add: MailboxInUse, LoginDelay, SysTemp, SysPerm, AuthError RESP-CODE variants
└── builder.rs      → NEW: Pop3ClientBuilder struct + impl
```

No new modules needed beyond `builder.rs`. All other changes are additive within existing files.

### Pattern 1: APOP Greeting Timestamp Extraction
**What:** Extract the `<timestamp>` from the server greeting by finding the first `<...>` bracket pair — do NOT assume it is at a fixed position.
**When to use:** In `apop()` method before computing the digest; also in the `connect()` variants (store greeting, search it lazily when apop() is called).
**Example:**
```rust
// Source: RFC 1939 + MailKit issue #529 (timestamp can be anywhere in greeting)
fn extract_apop_timestamp(greeting: &str) -> Option<&str> {
    let start = greeting.find('<')?;
    let end = greeting[start..].find('>')? + start;
    Some(&greeting[start..=end])
}
```

### Pattern 2: APOP MD5 Digest Computation
**What:** Concatenate timestamp (with angle brackets) + password, compute MD5, format as lowercase hex.
**When to use:** In `apop()` after extracting the timestamp.
**Example:**
```rust
// Source: RFC 1939 §7, md5 crate 0.8.0 docs
fn compute_apop_digest(timestamp: &str, password: &str) -> String {
    let input = format!("{timestamp}{password}");
    let digest = md5::compute(input.as_bytes());
    format!("{:x}", digest)
}
```

### Pattern 3: RESP-CODES Parsing in parse_status_line
**What:** After detecting a `-ERR` response, check if the text starts with `[` and extract the bracketed code to return a specific `Pop3Error` variant.
**When to use:** Replace the `ServerError(String)` return in the `-ERR` branch of `parse_status_line()`.
**Example:**
```rust
// Source: RFC 2449 §8, RFC 3206 §2
pub(crate) fn parse_status_line(line: &str) -> Result<&str> {
    let line = line.trim_end_matches("\r\n").trim_end_matches('\n');
    if line.starts_with("+OK") && (line.len() == 3 || line.as_bytes()[3].is_ascii_whitespace()) {
        Ok(line[3..].trim_start())
    } else if line.starts_with("-ERR") && (line.len() == 4 || line.as_bytes()[4].is_ascii_whitespace()) {
        let text = line[4..].trim_start();
        Err(parse_resp_code(text))
    } else {
        Err(Pop3Error::Parse(format!("unexpected response: {line}")))
    }
}

/// Map a response code bracket prefix to a typed Pop3Error variant.
fn parse_resp_code(text: &str) -> Pop3Error {
    if let Some(inner) = text.strip_prefix('[') {
        if let Some(end) = inner.find(']') {
            let code = &inner[..end];
            let rest = inner[end + 1..].trim_start().to_string();
            return match code {
                "IN-USE"      => Pop3Error::MailboxInUse(rest),
                "LOGIN-DELAY" => Pop3Error::LoginDelay(rest),
                "SYS/TEMP"    => Pop3Error::SysTemp(rest),
                "SYS/PERM"    => Pop3Error::SysPerm(rest),
                "AUTH"        => Pop3Error::AuthError(rest),
                _             => Pop3Error::ServerError(text.to_string()),
            };
        }
    }
    Pop3Error::ServerError(text.to_string())
}
```

### Pattern 4: Non-Consuming Builder with Async Terminal Method
**What:** Builder stores all connection parameters. Methods take `&mut self`, return `&mut Self`. The terminal `connect()` method is `async fn` that consumes `self` (by value) to produce a `Pop3Client`.
**When to use:** `Pop3ClientBuilder` implementation in `builder.rs`.
**Example:**
```rust
// Source: Rust API guidelines §C-BUILDER; tokio::runtime::Builder precedent
pub struct Pop3ClientBuilder {
    hostname: String,
    port: u16,
    timeout: Duration,
    tls: bool,           // set by .tls() — feature-gated
}

impl Pop3ClientBuilder {
    pub fn new(hostname: impl Into<String>, port: u16) -> Self {
        Pop3ClientBuilder {
            hostname: hostname.into(),
            port,
            timeout: crate::transport::DEFAULT_TIMEOUT,
            tls: false,
        }
    }

    pub fn timeout(&mut self, duration: Duration) -> &mut Self {
        self.timeout = duration;
        self
    }

    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    pub fn tls(&mut self) -> &mut Self {
        self.tls = true;
        self
    }

    pub async fn connect(self) -> crate::Result<Pop3Client> {
        // Note: terminal method takes self by value to construct Pop3Client
        if self.tls {
            Pop3Client::connect_tls(
                (self.hostname.as_str(), self.port),
                &self.hostname,
                self.timeout,
            ).await
        } else {
            Pop3Client::connect(
                (self.hostname.as_str(), self.port),
                self.timeout,
            ).await
        }
    }
}
```

### Pattern 5: APOP Method on Pop3Client
**What:** `apop()` checks state, extracts timestamp from stored greeting, computes digest, sends `APOP user digest`, updates session state.
**When to use:** New method in `client.rs` `impl Pop3Client` block.
**Example:**
```rust
// Source: RFC 1939 §7; CRLF-injection protection follows existing check_no_crlf pattern
pub async fn apop(&mut self, username: &str, password: &str) -> Result<()> {
    check_no_crlf(username)?;
    check_no_crlf(password)?;
    if self.state != SessionState::Connected {
        return Err(Pop3Error::NotAuthenticated);
    }
    let timestamp = extract_apop_timestamp(&self.greeting)
        .ok_or_else(|| Pop3Error::ServerError(
            "server does not support APOP (no timestamp in greeting)".to_string()
        ))?
        .to_string();
    let digest = compute_apop_digest(&timestamp, password);
    let cmd = format!("APOP {username} {digest}");
    self.transport.send_command(&cmd).await?;
    let line = self.transport.read_line().await?;
    match response::parse_status_line(&line) {
        Ok(_) => {
            self.state = SessionState::Authenticated;
            Ok(())
        }
        Err(Pop3Error::ServerError(msg)) => Err(Pop3Error::AuthFailed(msg)),
        Err(e) => Err(e),
    }
}
```

### Anti-Patterns to Avoid
- **Fixed timestamp position:** Do NOT parse `greeting.split_whitespace().last()` or `greeting[4..]` to find the timestamp. The `<timestamp>` can be anywhere in the greeting text after `+OK`. Use `find('<')`.
- **Consuming builder methods by default:** Do NOT return `Self` (owned) from builder configuration methods. Return `&mut Self` to avoid requiring reassignment in multi-step configuration (e.g., `builder = builder.timeout(...)`).
- **Putting TLS-unconditional connect() on builder:** The `connect()` terminal method must use `#[cfg]` internally, not expose feature-dependent branches in the public signature.
- **Panic on missing timestamp:** `apop()` MUST return `Err(Pop3Error::ServerError(...))` when the greeting contains no timestamp, never panic.
- **Converting ALL ServerErrors to AuthFailed in apop():** Only convert the top-level `-ERR` to `AuthFailed`; RESP-CODE variants like `MailboxInUse` should propagate as-is (mailbox locked is not an auth failure).

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MD5 digest computation | Custom MD5 implementation | `md5` 0.8.0 crate | MD5 has subtle implementation details (padding, endianness, bit-length encoding); wrong implementations produce silently-wrong digests |
| Hex encoding of digest bytes | Manual `format!` with `{:02x}` per byte loop | `format!("{:x}", md5::Digest)` | The `Digest` type's `LowerHex` impl handles this correctly in one call |
| Response code parsing | Regex-based extraction | Plain string operations (`strip_prefix`, `find`) | Regex is overkill for `[CODE]` prefix; the format is trivially parseable with slice operations |

**Key insight:** The md5 crate is the only new dependency. Everything else is standard string parsing in the existing response module pattern.

---

## Common Pitfalls

### Pitfall 1: Timestamp Position Assumption
**What goes wrong:** APOP fails against servers (Dovecot, Cyrus, Exchange) that put text before the timestamp, e.g., `+OK Dovecot ready. <12345.678@mail.example.com>`.
**Why it happens:** RFC 1939 example shows timestamp immediately after `+OK`, but the RFC only requires it to be present _in_ the greeting, not at a fixed offset.
**How to avoid:** Use `greeting.find('<')` + `greeting[start..].find('>')` to locate the bracket pair anywhere in the string. Store the full greeting text in `Pop3Client` (already done via `self.greeting`).
**Warning signs:** APOP returning `ServerError("server does not support APOP")` against servers that visually show a timestamp when you connect manually via telnet.

### Pitfall 2: APOP Error Mapping — Conflating RESP-CODES with AuthFailed
**What goes wrong:** `apop()` blindly converts all `-ERR` responses to `AuthFailed`, hiding that the real error was `[IN-USE]` (mailbox locked) or `[LOGIN-DELAY]`.
**Why it happens:** Same pattern as `login()` where we promote `ServerError` -> `AuthFailed`, but RESP-CODE variants are semantically distinct.
**How to avoid:** Only convert `Pop3Error::ServerError(msg)` to `AuthFailed`. Let `MailboxInUse`, `LoginDelay` etc. propagate unchanged.
**Warning signs:** Callers see `AuthFailed` but retrying with a different password succeeds — the real error was a transient lock.

### Pitfall 3: RESP-CODE Parsing Breaks Existing AuthFailed Promotion
**What goes wrong:** After updating `parse_status_line()` to return RESP-CODE variants, `login()`'s `-ERR` -> `AuthFailed` promotion logic stops working because it matches `Pop3Error::ServerError` but now gets `Pop3Error::AuthError`.
**Why it happens:** `login()` currently does `Err(Pop3Error::ServerError(msg)) => Err(Pop3Error::AuthFailed(msg))`. After Phase 4, a server returning `-ERR [AUTH] bad credentials` will produce `Pop3Error::AuthError(...)`, which login's match arm won't catch.
**How to avoid:** Update `login()`'s error remapping to also handle `Pop3Error::AuthError(msg) => Err(Pop3Error::AuthFailed(msg))`. The `[AUTH]` code means "credential problem" which is exactly `AuthFailed` semantics.
**Warning signs:** Tests for `login()` with `-ERR [AUTH] ...` responses returning `AuthError` instead of `AuthFailed`.

### Pitfall 4: Builder's `tls` Field Compile Error Without Feature
**What goes wrong:** If `tls: bool` field is unconditional on the struct but the `.tls()` method is `#[cfg(feature)]`-gated, Clippy warns about a dead field in no-TLS builds.
**Why it happens:** The field exists but is never set to `true` when there is no TLS feature method to call it.
**How to avoid:** Either gate the field itself with `#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]` and default to false only when the feature is present, OR always keep the field but add `#[allow(dead_code)]` on no-TLS builds. The simpler approach: gate the field and provide a const default.
**Warning signs:** `cargo clippy` warning `dead_code` on `tls` field in plain (no-TLS) build.

### Pitfall 5: Builder `connect()` Ownership — Can't Chain `builder.tls().connect().await`
**What goes wrong:** If `.tls()` returns `&mut Self` but `connect()` takes `self` by value, the call chain `Pop3ClientBuilder::new(...).tls().connect()` fails: `connect()` needs to move out of a mutable reference.
**Why it happens:** `&mut Self` chaining works for configuration, but ownership transfer for the terminal method requires `self`. You cannot move out of a `&mut T`.
**How to avoid:** Store builder config in `let mut builder = Pop3ClientBuilder::new(...)`, configure it, then call `.connect()` which takes the builder by value. Alternatively, use `clone()` in connect (requires `Clone` on builder). **The idiomatic solution:** Document that `connect()` must be called on a named variable, not in a chain with `&mut Self` methods.
**Warning signs:** Compiler error `cannot move out of `*builder` which is behind a mutable reference`.

### Pitfall 6: APOP Digest Lowercase Requirement
**What goes wrong:** Sending uppercase hex in the `APOP` command (e.g., `APOP user C4C9...`) causes authentication failure.
**Why it happens:** RFC 1939 requires the digest in "lower-case ASCII characters".
**How to avoid:** Always use `format!("{:x}", digest)` (lowercase `x`), never `"{:X}"`.
**Warning signs:** Authentication fails against servers that do strict digest comparison.

---

## Code Examples

Verified patterns from official sources:

### MD5 Digest for APOP (RFC 1939 §7 example verified)
```rust
// Source: md5 0.8.0 docs; RFC 1939 §7 example
// RFC says: timestamp=<1896.697170952@dbc.mtview.ca.us>, password=tanstaaf
// Expected: c4c9334bac560ecc979e58001b3e22fb
let timestamp = "<1896.697170952@dbc.mtview.ca.us>";
let password = "tanstaaf";
let input = format!("{timestamp}{password}");
let digest = md5::compute(input.as_bytes());
let hex = format!("{:x}", digest);
assert_eq!(hex, "c4c9334bac560ecc979e58001b3e22fb");
```

### New Pop3Error Variants for RESP-CODES
```rust
// Source: RFC 2449 §8.1 (IN-USE, LOGIN-DELAY), RFC 3206 §2 (SYS/TEMP, SYS/PERM, AUTH)
#[derive(Debug, thiserror::Error)]
pub enum Pop3Error {
    // ... existing variants ...

    /// Mailbox is locked by another POP3 session (RESP-CODE: `[IN-USE]`).
    /// Try again after the other session ends.
    #[error("mailbox in use: {0}")]
    MailboxInUse(String),

    /// Authentication attempted too soon after last login (RESP-CODE: `[LOGIN-DELAY]`).
    #[error("login delay: {0}")]
    LoginDelay(String),

    /// Temporary system error — likely transient (RESP-CODE: `[SYS/TEMP]`).
    #[error("temporary system error: {0}")]
    SysTemp(String),

    /// Permanent system error — requires manual intervention (RESP-CODE: `[SYS/PERM]`).
    #[error("permanent system error: {0}")]
    SysPerm(String),

    /// Authentication credential problem (RESP-CODE: `[AUTH]`).
    /// Distinct from `AuthFailed` — indicates a RESP-CODE-aware credential error.
    #[error("auth error: {0}")]
    AuthError(String),
}
```

### Builder API Usage (caller perspective)
```rust
// Source: Rust API guidelines §C-BUILDER pattern
// Non-chained (works with &mut Self returns):
let mut builder = Pop3ClientBuilder::new("pop.gmail.com", 995);
builder.timeout(Duration::from_secs(60));
#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
builder.tls();
let mut client = builder.connect().await?;
client.login("user@gmail.com", "app-password").await?;
```

### APOP Doctest Pattern (no_run — requires real server)
```rust
/// # Security Warning
///
/// APOP uses MD5, which is cryptographically broken. MD5 collision attacks are
/// practical and trivial. APOP provides no protection against offline dictionary
/// attacks if the exchanged messages are intercepted. Use only with legacy servers
/// where USER/PASS over TLS is unavailable. For modern servers, prefer
/// [`login()`](Self::login) over a TLS connection.
///
/// # Example
///
/// ```no_run
/// # use pop3::Pop3Client;
/// # #[tokio::main]
/// # async fn main() -> pop3::Result<()> {
/// let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
/// // Server greeting must contain an APOP timestamp: <timestamp@host>
/// client.apop("user", "password").await?;
/// client.quit().await?;
/// # Ok(())
/// # }
/// ```
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| TlsMode enum in public API | Builder with feature-gated `.tls()` method | v2.0 design decision | Callers never see TLS feature flags; compile-time safety |
| Generic `ServerError(String)` for all -ERR | Named RESP-CODE variants per RFC 2449/3206 | Phase 4 | Callers can match on `MailboxInUse` vs `LoginDelay` vs `AuthFailed` |
| No APOP support | `apop()` method with MD5 security caveat in docs | Phase 4 | Legacy server compatibility; prominently discouraged |
| Direct constructor calls | Fluent builder `Pop3ClientBuilder` | Phase 4 | API-02: TLS complexity hidden; API-01: fluent configuration |

**Deprecated/outdated:**
- Using `ServerError(String)` to signal mailbox-in-use: after Phase 4, callers should match `MailboxInUse`.
- Calling `connect_tls()` directly when a builder is available: the builder is the recommended entry point per API-01/API-02.

---

## Open Questions

1. **Should `Pop3ClientBuilder` include a `starttls()` / `.upgrade_to_tls()` method?**
   - What we know: STARTTLS is already implemented as `client.stls()`. The builder's `.tls()` sets the connection mode to TLS-on-connect (port 995). STARTTLS is a different flow (connect plain, then upgrade).
   - What's unclear: Does API-02 ("hides TLS feature flag complexity") require the builder to also encapsulate STARTTLS, or just TLS-on-connect?
   - Recommendation: Defer STARTTLS from the builder for Phase 4. Scope API-01/API-02 to TLS-on-connect only. Add STARTTLS builder support in a later phase or as a follow-on in Phase 4 if time allows.

2. **Should the builder validate the hostname before `connect()`?**
   - What we know: `connect_tls()` validates via DNS name parsing; this will surface as `InvalidDnsName` at connect time.
   - What's unclear: Whether eager validation (at `.tls()` call time) provides meaningful ergonomic benefit.
   - Recommendation: Lazy validation (at `connect()`) is simpler and consistent with how `Pop3Client::connect_tls()` works today. No change needed.

3. **Which `Pop3Error` variant does `apop()` promote server errors to?**
   - What we know: `login()` promotes all `ServerError` to `AuthFailed`. RESP-CODES like `[AUTH]` will now produce `AuthError`, not `ServerError`.
   - What's unclear: Whether `apop()` should also promote `AuthError` to `AuthFailed` for consistency with `login()`.
   - Recommendation: Yes — promote both `ServerError(msg)` and `AuthError(msg)` from `apop()` to `AuthFailed(msg)`. The `[AUTH]` RESP-CODE explicitly means credential problem, which is semantically identical to `AuthFailed`.

---

## Validation Architecture

> Skipped — `workflow.nyquist_validation` is not set in `.planning/config.json`.

---

## Sources

### Primary (HIGH confidence)
- RFC 1939 (https://www.rfc-editor.org/rfc/rfc1939.html) — APOP command syntax, timestamp format, MD5 digest computation (§7)
- RFC 2449 (https://www.rfc-editor.org/rfc/rfc2449.html) — RESP-CODES capability, IN-USE and LOGIN-DELAY response codes (§6.4, §8)
- RFC 3206 (https://www.rfc-editor.org/rfc/rfc3206.html) — SYS/TEMP, SYS/PERM, AUTH response codes
- IANA POP3 Extension Mechanism Registry (https://www.iana.org/assignments/pop3-extension-mechanism) — Complete list of 6 registered response codes
- md5 crate docs (https://docs.rs/md5) — v0.8.0 `compute()` API and `{:x}` formatting
- Rust API Guidelines §C-BUILDER (https://doc.rust-lang.org/1.0.0/style/ownership/builders.html) — Non-consuming builder pattern, `&mut Self` return

### Secondary (MEDIUM confidence)
- MailKit issue #529 (https://github.com/jstedfast/MailKit/issues/529) — Real-world confirmation that APOP timestamp can appear anywhere in greeting; fix is `find('<')` not fixed-offset parsing
- tokio::runtime::Builder docs (https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html) — Precedent for `&mut Self` builder with synchronous `build()` terminal; adapted for async `connect()`
- reqwest ClientBuilder (https://docs.rs/reqwest/latest/reqwest/) — Precedent for feature-flag-gated builder methods hiding TLS complexity

### Tertiary (LOW confidence)
- async-pop crate docs (https://docs.rs/async-pop/latest/async_pop/) — Confirmed no builder pattern exists in this space; validated the builder is novel for POP3 crates

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — md5 0.8.0 verified via docs.rs; existing deps confirmed in Cargo.toml
- Architecture: HIGH — RFC 1939, 2449, 3206 are stable specifications; Rust builder pattern is stable idiom
- Pitfalls: HIGH — Timestamp position pitfall verified by MailKit issue; RESP-CODES/AuthFailed interaction derived from existing code analysis; builder ownership pitfall is fundamental Rust borrow checker behavior
- APOP MD5 specifics: HIGH — RFC 1939 §7 provides exact test vector verifiable with md5 crate

**Research date:** 2026-03-01
**Valid until:** 2026-09-01 (RFCs are stable; Rust builder idiom is stable; md5 crate is unlikely to change)
