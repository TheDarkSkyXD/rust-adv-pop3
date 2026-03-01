# Phase 3: TLS and Publish - Research

**Researched:** 2026-03-01
**Domain:** Rust async TLS (tokio-rustls / tokio-openssl), Cargo feature flags, rustdoc doctests, crates.io publishing
**Confidence:** HIGH (stack confirmed via official docs and crates.io; patterns cross-verified)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### TLS Connection API
- Separate `connect_tls(addr, hostname, timeout)` method alongside existing `connect()`
- `hostname` parameter is `&str` — DNS name validation happens internally, returns `Pop3Error::InvalidDnsName` on bad input
- Uses system trust store via `rustls-native-certs` — no custom TLS config parameter (simplest API, covers 95% of use cases)
- Add `connect_tls_default(addr, hostname)` convenience method with 30s timeout — matches existing `connect_default()` symmetry

#### Feature Flag Design
- `rustls-tls` is the default feature — `pop3 = "2.0"` gets TLS out of the box with no system deps
- `openssl-tls` is an opt-in feature requiring system OpenSSL
- `connect_tls()` and `stls()` are conditionally compiled — they only exist when a TLS feature is active (no dead code / misleading API)
- `compile_error!` when both `rustls-tls` and `openssl-tls` are active simultaneously
- Error type: single `Pop3Error::Tls(String)` variant — converts both rustls and openssl errors to string messages, no backend types leak into public API

#### STARTTLS Upgrade Flow
- Explicit `stls()` method on `Pop3Client` — maps 1:1 to POP3 STLS command
- Pre-auth only per RFC 2595 — returns error if already authenticated (STLS valid only in AUTHORIZATION state)
- No SessionState change for TLS — TLS is a transport concern, not session state. Separate `is_encrypted()` method reports TLS status

#### Documentation
- Full crate-level documentation with examples — user landing on docs.rs understands the library in 60 seconds
- One example file per connection mode: `examples/basic.rs` (plain), `examples/tls.rs` (TLS-on-connect), `examples/starttls.rs` (upgrade)
- Integration tests cover full connect-auth-command flows against a mock POP3 server (not just TLS-specific scenarios)
- Every public type, function, and method has a rustdoc comment with a working doctest

### Claude's Discretion
- OpenSSL async integration approach (tokio-openssl vs manual wrapping)
- STARTTLS hostname parameter design (pass explicitly to stls() vs store from connect)
- README update timing (now vs at publish time)
- Integration test mock server implementation details

### Deferred Ideas (OUT OF SCOPE)
- Custom TLS configuration (client certificates, custom root CAs) — future enhancement
- Pop3ClientBuilder fluent interface — Phase 4
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| TLS-01 | User can connect via TLS-on-connect (port 995) using rustls backend | tokio-rustls 0.26.4 + rustls 0.23 + rustls-native-certs 0.8.3; Transport::connect_tls() stub is ready to implement |
| TLS-02 | User can connect via TLS-on-connect using openssl backend | tokio-openssl 0.6.5 + openssl 0.10; feature-gated, conditionally compiled |
| TLS-03 | TLS backend selected via Cargo feature flags (`rustls-tls`, `openssl-tls`) | Cargo [features] + optional deps + #[cfg(feature = ...)] pattern documented |
| TLS-04 | Simultaneous activation of both TLS features produces a compile error | compile_error! macro under #[cfg(all(feature="rustls-tls", feature="openssl-tls"))] pattern |
| TLS-05 | User can upgrade a plain TCP connection to TLS via STARTTLS (STLS command) | stls() method sends STLS command, drains BufReader, replaces stream via Transport::upgrade_tls() |
| TLS-06 | STARTTLS correctly drains BufReader before stream upgrade | BufReader::buffer() extracts pending bytes; into_inner() recovers TCP stream; pattern researched |
| CMD-01 | User can retrieve message headers + N lines via TOP command | top() already implemented in client.rs; needs doctests and integration test coverage |
| CMD-02 | User can query server capabilities via CAPA command | capa() already implemented in client.rs; needs doctests and integration test coverage |
| QUAL-02 | Integration tests cover connect, auth, and command flows via mock POP3 server | tokio_test::io::Builder is the established mock pattern; existing tests are the model |
| QUAL-04 | CI matrix tests both `rustls-tls` and `openssl-tls` feature flags | GitHub Actions matrix strategy pattern with feature-specific cargo test commands |
| QUAL-05 | All public items have rustdoc with working doctests | no_run attribute for network examples; async doctests need tokio_test::block_on or no_run |
</phase_requirements>

---

## Summary

Phase 3 adds dual TLS backends (rustls-tls default, openssl-tls opt-in) via Cargo feature flags, implements STARTTLS stream upgrade, completes integration test coverage for CAPA and TOP, adds full rustdoc with doctests, and publishes v2.0.0 to crates.io.

The existing codebase provides strong scaffolding. `Transport::connect_tls()` is a stub ready to implement. `Pop3Error::Tls` exists but currently takes `rustls::Error` directly — this must change to `Tls(String)` for backend-agnostic error reporting. The `BufReader`-wrapped transport pattern requires a specific drain-then-replace sequence for STARTTLS: extract buffered bytes via `buffer()`, recover the TCP stream via `into_inner()`, perform the TLS handshake, then reconstruct the `Transport` with the TLS stream.

For CI, the openssl-tls matrix job on Linux is straightforward (apt-get libssl-dev). Windows CI for openssl-tls is documented as problematic in STATE.md — the research confirms this: the openssl-sys crate requires either system OpenSSL (via vcpkg on Windows, which is brittle) or the `vendored` feature flag (which requires Perl and Make). The recommendation is to limit the openssl-tls CI matrix to ubuntu-latest only for this phase.

**Primary recommendation:** Implement rustls-tls first (default feature, pure Rust, no system deps), gate openssl-tls behind a feature that is test-only on Linux CI, and use `no_run` doctests for all TLS examples since they require a real server.

---

## Standard Stack

### Core (TLS)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio-rustls | 0.26.4 | Async TLS with rustls backend for Tokio | Official Tokio TLS integration; pure Rust, no OpenSSL |
| rustls | 0.23.x | TLS protocol implementation | Already in Cargo.toml; zero C dependencies |
| rustls-native-certs | 0.8.3 | Load OS trust store for rustls | Already in Cargo.toml; system cert integration |
| tokio-openssl | 0.6.5 | Async TLS with OpenSSL backend for Tokio | Standard tokio-openssl wrapper; supports async connect |
| openssl | 0.10.x | OpenSSL bindings (via tokio-openssl) | Transitive dependency via tokio-openssl |

### Supporting (Docs and Publish)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio-test | 0.4 (existing) | Mock I/O for doctests and integration tests | All tests that mock server responses |
| rustls-pki-types | (via rustls) | ServerName type for hostname validation | Required for TlsConnector::connect() |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| tokio-openssl | native-tls / tokio-native-tls | native-tls uses platform APIs (Schannel/SecureTransport/OpenSSL); tokio-openssl gives explicit OpenSSL control per user request |
| rustls-native-certs | webpki-roots | webpki-roots is a bundled cert set; rustls-native-certs uses the OS trust store (user's locked decision) |

**Installation (Cargo.toml changes):**
```toml
[features]
default = ["rustls-tls"]
rustls-tls = ["dep:tokio-rustls"]
openssl-tls = ["dep:tokio-openssl", "dep:openssl"]

[dependencies]
tokio = { version = "1", features = ["net", "io-util", "time", "rt-multi-thread", "macros"] }
thiserror = "2"
# TLS backends — feature-gated, only one can be active
tokio-rustls = { version = "0.26", optional = true }
rustls-native-certs = { version = "0.8", optional = true }
tokio-openssl = { version = "0.6", optional = true }
openssl = { version = "0.10", optional = true }
```

Note: `rustls` itself becomes a transitive dependency through `tokio-rustls` — remove the direct `rustls` dependency from Cargo.toml when adding `tokio-rustls`. `rustls-native-certs` also becomes optional.

---

## Architecture Patterns

### Recommended Module Structure (unchanged)

```
src/
├── lib.rs          # Re-exports; add connect_tls, is_encrypted to public API
├── client.rs       # connect_tls(), connect_tls_default(), stls(), is_encrypted()
├── transport.rs    # connect_tls() impl; upgrade_tls_rustls() / upgrade_tls_openssl()
├── error.rs        # Pop3Error::Tls(String) — change from Tls(#[from] rustls::Error)
├── response.rs     # Unchanged
└── types.rs        # Unchanged
```

### Pattern 1: Feature-Gated TLS Dependency with compile_error

**What:** Cargo feature flags that gate TLS code with a compile-time mutual exclusion guard.

**When to use:** Whenever two features cannot be active simultaneously.

**Example:**
```rust
// src/lib.rs — place at the top, before any modules
#[cfg(all(feature = "rustls-tls", feature = "openssl-tls"))]
compile_error!(
    "Feature flags `rustls-tls` and `openssl-tls` are mutually exclusive. \
     Enable only one TLS backend at a time."
);
```

The `compile_error!` macro under `#[cfg(all(...))]` fires at compile time with a human-readable message when both features are activated. This is the canonical Rust pattern (verified across multiple crates including rusoto, ring, etc.).

### Pattern 2: Backend-Agnostic Error Wrapping

**What:** Convert backend-specific error types to `Pop3Error::Tls(String)` so the public API never exposes `rustls::Error` or `openssl::ssl::Error`.

**When to use:** All TLS error conversion sites.

**Example:**
```rust
// In error.rs — change FROM:
//   Tls(#[from] rustls::Error),
// TO:
#[error("TLS error: {0}")]
Tls(String),

// In transport.rs rustls path:
connector.connect(server_name, stream).await
    .map_err(|e| Pop3Error::Tls(e.to_string()))?;

// In transport.rs openssl path:
Pin::new(&mut stream).connect().await
    .map_err(|e| Pop3Error::Tls(e.to_string()))?;
```

### Pattern 3: rustls TLS-on-Connect

**What:** Build a `TlsConnector` from rustls `ClientConfig` using system trust store.

**When to use:** `Transport::connect_tls()` implementation for rustls-tls feature.

**Example:**
```rust
// Source: tokio-rustls 0.26.4 README + rustls-native-certs 0.8.3 docs
#[cfg(feature = "rustls-tls")]
pub(crate) async fn connect_tls(
    addr: impl tokio::net::ToSocketAddrs,
    hostname: &str,
    timeout: Duration,
) -> Result<Self> {
    use rustls_pki_types::ServerName;
    use std::sync::Arc;
    use tokio_rustls::TlsConnector;
    use tokio_rustls::rustls::{ClientConfig, RootCertStore};

    // Parse and validate the hostname
    let server_name = ServerName::try_from(hostname.to_owned())
        .map_err(|e| Pop3Error::InvalidDnsName(e.to_string()))?;

    // Load system trust store
    let certs = rustls_native_certs::load_native_certs();
    let mut root_store = RootCertStore::empty();
    for cert in certs.certs {
        root_store.add(cert).map_err(|e| Pop3Error::Tls(e.to_string()))?;
    }

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));

    let tcp_stream = TcpStream::connect(addr).await?;
    let tls_stream = connector.connect(server_name, tcp_stream).await
        .map_err(|e| Pop3Error::Tls(e.to_string()))?;

    let (read_half, write_half) = io::split(tls_stream);
    Ok(Transport {
        reader: BufReader::new(Box::new(read_half)),
        writer: Box::new(write_half),
        timeout,
        encrypted: true,
    })
}
```

**Key note:** `rustls_native_certs::load_native_certs()` in 0.8.x returns a struct with `.certs` (Vec) and `.errors` fields — not a `Result<Vec<...>>`. Handle errors gracefully (log or ignore non-fatal cert load errors as recommended by the crate docs).

### Pattern 4: OpenSSL TLS-on-Connect

**What:** Build an `SslConnector`, wrap the TCP stream in `SslStream`, perform async handshake.

**When to use:** `Transport::connect_tls()` implementation for openssl-tls feature.

**Example:**
```rust
// Source: tokio-openssl 0.6.5 test.rs pattern
#[cfg(feature = "openssl-tls")]
pub(crate) async fn connect_tls(
    addr: impl tokio::net::ToSocketAddrs,
    hostname: &str,
    timeout: Duration,
) -> Result<Self> {
    use openssl::ssl::{SslConnector, SslMethod};
    use tokio_openssl::SslStream;
    use std::pin::Pin;

    let mut connector_builder = SslConnector::builder(SslMethod::tls())
        .map_err(|e| Pop3Error::Tls(e.to_string()))?;
    // Use system trust store (OpenSSL reads it by default on Linux/macOS)
    let connector = connector_builder.build();

    let ssl = connector.configure()
        .map_err(|e| Pop3Error::Tls(e.to_string()))?
        .into_ssl(hostname)
        .map_err(|e| Pop3Error::Tls(e.to_string()))?;

    let tcp_stream = TcpStream::connect(addr).await?;
    let mut tls_stream = SslStream::new(ssl, tcp_stream)
        .map_err(|e| Pop3Error::Tls(e.to_string()))?;

    Pin::new(&mut tls_stream).connect().await
        .map_err(|e| Pop3Error::Tls(e.to_string()))?;

    let (read_half, write_half) = io::split(tls_stream);
    Ok(Transport {
        reader: BufReader::new(Box::new(read_half)),
        writer: Box::new(write_half),
        timeout,
        encrypted: true,
    })
}
```

**Key note:** `into_ssl(hostname)` enables SNI and hostname verification. OpenSSL on Linux/macOS reads the system CA bundle automatically. Windows requires `openssl` with the `vendored` feature or pre-installed OpenSSL via vcpkg.

### Pattern 5: STARTTLS BufReader Drain and Stream Upgrade

**What:** Drain the BufReader's internal buffer, recover the underlying TCP stream, upgrade it to TLS, rebuild Transport.

**When to use:** `Transport::upgrade_to_tls()` called from `Pop3Client::stls()`.

**Critical detail:** `tokio::io::BufReader::into_inner()` discards buffered data. You must copy the buffer contents FIRST, then call `into_inner()`.

**Example:**
```rust
pub(crate) async fn upgrade_to_tls_rustls(
    mut self,
    hostname: &str,
) -> Result<Self> {
    use rustls_pki_types::ServerName;
    use std::sync::Arc;
    use tokio_rustls::TlsConnector;
    use tokio_rustls::rustls::{ClientConfig, RootCertStore};

    let server_name = ServerName::try_from(hostname.to_owned())
        .map_err(|e| Pop3Error::InvalidDnsName(e.to_string()))?;

    // Step 1: Drain the BufReader's internal buffer before discarding it.
    // buffer() returns a &[u8] reference to bytes already read from TCP
    // but not yet consumed by the application.
    // For POP3 STARTTLS, the server must have sent exactly "+OK ready\r\n"
    // before this point — if the buffer is non-empty at this point, something
    // went wrong. We assert empty and proceed.
    let pending = self.reader.buffer().len();
    if pending > 0 {
        // Data was buffered before the STLS +OK — treat as protocol error
        return Err(Pop3Error::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected {pending} bytes in buffer before TLS upgrade"),
        )));
    }

    // Step 2: Recover the underlying reader (TCP stream half).
    // into_inner() discards buffer — safe since we verified it's empty above.
    let tcp_read_half = self.reader.into_inner();

    // Step 3: Reunite the split halves back into a TcpStream.
    // This requires tokio's unsplit() — only works if both halves came from
    // the same split() call. Store the OwnedReadHalf/OwnedWriteHalf types
    // in Transport to enable reuniting.
    // NOTE: The current Transport uses Box<dyn AsyncRead> which loses the
    // concrete type needed for unsplit(). Transport must store typed halves
    // for TLS upgrade paths. See Architecture Notes below.

    // ... reunite, handshake, split new TLS stream ...
    todo!("See Architecture Notes — typed halves required")
}
```

**Architecture Notes for STARTTLS — Critical Design Decision:**

The current `Transport` erases the concrete types of its read/write halves into `Box<dyn AsyncRead/AsyncWrite>`. This type erasure prevents `unsplit()` from being called (you can't recover `TcpStream` from `Box<dyn AsyncRead>`).

There are two viable approaches for STARTTLS:

**Option A (Simpler — Recommended):** Accept that STARTTLS does NOT literally reunite the TCP stream. Instead, send the STLS command, verify +OK, then call `Transport::connect_tls()` with the same `addr` and open a FRESH TLS connection to the same address. This avoids the type erasure problem entirely. The drawback is that it opens a second TCP connection (briefly two sockets), but for POP3 this is acceptable since STARTTLS is a negotiation step before any mailbox state is established.

**Option B (Correct per RFC — More Complex):** Change `Transport` to use an enum over concrete types (TcpStream, rustls TlsStream, openssl SslStream) rather than trait objects. This allows type-safe reuniting and real stream upgrade. The `Box<dyn ...>` erasure must be removed. This is the correct approach if genuine in-place upgrade is required.

Given the complexity of Option B, the planner should evaluate whether Option A is acceptable per the CONTEXT.md decisions. The CONTEXT.md states "BufReader buffer is drained before stream upgrade" which implies genuine in-place upgrade (Option B).

**Recommendation for planner:** Use Option B with a concrete stream enum in Transport. Store `OwnedReadHalf` / `OwnedWriteHalf` typed for each stream variant. This is the correct approach.

### Pattern 6: Conditional Compilation with cfg_if or cfg blocks

**What:** Methods that only exist under the correct TLS feature flag.

**When to use:** `connect_tls()`, `stls()`, `is_encrypted()` on `Pop3Client`.

**Example:**
```rust
// In client.rs
#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
pub async fn connect_tls(
    addr: impl tokio::net::ToSocketAddrs,
    hostname: &str,
    timeout: Duration,
) -> Result<Self> {
    let mut transport = Transport::connect_tls(addr, hostname, timeout).await?;
    let greeting_line = transport.read_line().await?;
    let greeting_text = response::parse_status_line(&greeting_line)?;
    Ok(Pop3Client {
        transport,
        greeting: greeting_text.to_string(),
        state: SessionState::Connected,
    })
}

#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
pub async fn connect_tls_default(
    addr: impl tokio::net::ToSocketAddrs,
    hostname: &str,
) -> Result<Self> {
    Self::connect_tls(addr, hostname, crate::transport::DEFAULT_TIMEOUT).await
}

#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
pub async fn stls(&mut self, hostname: &str) -> Result<()> {
    if self.state == SessionState::Authenticated {
        return Err(Pop3Error::ServerError(
            "STLS not allowed after authentication (RFC 2595)".into()
        ));
    }
    self.send_and_check("STLS").await?;
    self.transport = self.transport.upgrade_to_tls(hostname).await?;
    Ok(())
}

// is_encrypted() is always available (returns false when no TLS feature active)
pub fn is_encrypted(&self) -> bool {
    self.transport.is_encrypted()
}
```

### Pattern 7: Rustdoc Doctests for Async Network Code

**What:** Rustdoc runs code blocks in `///` comments as tests. Network code cannot actually connect, so use `no_run`.

**When to use:** All public methods that perform I/O.

**Example:**
```rust
/// Connect to a POP3 server over TLS (port 995).
///
/// # Example
///
/// ```no_run
/// use pop3::Pop3Client;
///
/// #[tokio::main]
/// async fn main() -> pop3::Result<()> {
///     let mut client = Pop3Client::connect_tls(
///         ("pop.gmail.com", 995),
///         "pop.gmail.com",
///         std::time::Duration::from_secs(30),
///     ).await?;
///     Ok(())
/// }
/// ```
pub async fn connect_tls(...) { ... }
```

`no_run` compiles the code (verifies syntax and types) but does not execute it. This is the correct approach for network examples. `#[tokio::main]` is valid inside `no_run` doctests because the code is compiled but not run, so the tokio runtime setup is syntactically valid.

For methods that test only parsing (no I/O), use normal doctests without `no_run`. These can use `tokio_test::block_on()` if async is needed in a standalone doctest context.

### Anti-Patterns to Avoid

- **Exposing backend types in public API:** Never return `rustls::Error`, `openssl::ssl::Error`, or `ServerName` from public methods. Always convert to `Pop3Error::Tls(String)`.
- **Both TLS backends in same Transport impl without cfg:** Code paths for both backends must be in separate `#[cfg(feature = ...)]` blocks — no runtime dispatch over TLS backends.
- **Keeping direct `rustls` dep after adding `tokio-rustls`:** `tokio-rustls` re-exports `rustls` — a direct dep causes version conflicts. Remove the direct dep.
- **Using `into_inner()` without checking `buffer()` first:** For STARTTLS, always verify the buffer is empty before calling `into_inner()`. Non-empty buffer means data was lost.
- **Testing TLS on Windows CI with openssl-tls:** Avoid until vendored or vcpkg setup is confirmed working. Linux-only is correct for Phase 3.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Async TLS handshake (rustls) | Custom async state machine | tokio-rustls TlsConnector | Handles split handshake, buffering, shutdown |
| Async TLS handshake (openssl) | Custom pinning/polling | tokio-openssl SslStream | Handles Pin, async connect, session reuse |
| OS trust store loading | Custom cert file parsing | rustls-native-certs 0.8 | Handles Windows/macOS/Linux stores; ~300KB parsed correctly |
| Hostname validation | Regex or string matching | rustls-pki-types ServerName::try_from | RFC-compliant DNS name parsing including IDN |
| TLS error formatting | Custom error types | `.map_err(|e| Pop3Error::Tls(e.to_string()))` | All backends implement Display correctly |

**Key insight:** TLS handshakes have subtleties (renegotiation, session tickets, shutdown) that require battle-tested implementations. Custom TLS state machines are how security vulnerabilities are born.

---

## Common Pitfalls

### Pitfall 1: Direct rustls Dependency Conflict with tokio-rustls

**What goes wrong:** Cargo resolves `rustls = "0.23"` (direct dep) and `tokio-rustls`'s transitive `rustls = "0.23"` as the same crate — but if versions diverge even slightly, you get duplicate type errors like "`rustls::ClientConfig` (crate A) != `rustls::ClientConfig` (crate B)".

**Why it happens:** Two different semver-compatible versions of the same crate can be included as distinct crates. Types from different crate instances are not compatible.

**How to avoid:** Remove the direct `rustls` dep when adding `tokio-rustls`. Use `tokio_rustls::rustls::ClientConfig` (re-exported). Check `cargo tree` for duplicate `rustls` entries.

**Warning signs:** Compiler error "expected `rustls::ClientConfig`, found a different `rustls::ClientConfig`".

### Pitfall 2: Pop3Error::Tls Currently Takes #[from] rustls::Error

**What goes wrong:** `error.rs` has `Tls(#[from] rustls::Error)` — this must change to `Tls(String)`. If it is not changed, the openssl path cannot convert errors, and the public API leaks the `rustls::Error` type.

**Why it happens:** The current error.rs was written during Phase 1 before the dual-backend decision was finalized.

**How to avoid:** Change the variant to `Tls(String)` early in Phase 3. Update all call sites to use `.map_err(|e| Pop3Error::Tls(e.to_string()))`.

**Warning signs:** Compilation fails on openssl branch with "expected rustls::Error" conversion errors.

### Pitfall 3: rustls-native-certs 0.8 API Break from 0.7

**What goes wrong:** In rustls-native-certs 0.8, `load_native_certs()` returns `CertificateResult` (struct with `.certs` Vec and `.errors` Vec), NOT a `Result<Vec<...>>`. Code written for 0.7 using `?` operator will fail to compile.

**Why it happens:** The API was changed in 0.8 to support partial success (some certs load even if others fail).

**How to avoid:** Always use the 0.8 pattern:
```rust
let result = rustls_native_certs::load_native_certs();
// result.errors contains non-fatal load errors (log them if desired)
// result.certs contains successfully loaded certs
for cert in result.certs {
    root_store.add(cert).map_err(|e| Pop3Error::Tls(e.to_string()))?;
}
```

**Warning signs:** Compile error "cannot apply `?` to `CertificateResult`".

### Pitfall 4: OpenSSL on Windows CI Fails Without Vendored Feature

**What goes wrong:** `cargo test --features openssl-tls` on a Windows GitHub Actions runner fails with "Could not find OpenSSL installation" because openssl-sys cannot find system OpenSSL.

**Why it happens:** Windows does not ship OpenSSL. The `vendored` feature compiles OpenSSL from source but requires Perl and Make to be available.

**How to avoid:** Limit the `openssl-tls` CI job to `ubuntu-latest` only. Add a note in CONTRIBUTING.md that openssl-tls requires system OpenSSL (`libssl-dev` on Ubuntu). STATE.md documents this concern explicitly.

**Warning signs:** CI error: "Could not find directory of OpenSSL installation" or "Could not find OpenSSL on Windows".

### Pitfall 5: BufReader Buffer Loss During STARTTLS Type Erasure

**What goes wrong:** Calling `self.reader.into_inner()` to get the TCP stream for TLS upgrade silently discards any bytes already buffered in `BufReader`. For POP3, this should not happen in practice (the STLS +OK line is the last thing read before upgrade), but if the server sends extra data, those bytes are lost.

**Why it happens:** `tokio::io::BufReader::into_inner()` doc: "Note that any leftover data in the internal buffer is lost."

**How to avoid:** Call `self.reader.buffer()` first, verify it's empty (or handle the bytes), then call `into_inner()`. The CONTEXT.md requirement TLS-06 specifically calls out this drain.

**Warning signs:** Intermittent connection failures after STARTTLS when server sends data coalesced with +OK response.

### Pitfall 6: Doctest Compilation Failure Without TLS Feature

**What goes wrong:** A doctest for `connect_tls()` compiles without the `rustls-tls` feature active — but since the method is `#[cfg(feature = "rustls-tls")]`, the method doesn't exist, and the doctest fails with "no method named `connect_tls`".

**Why it happens:** `cargo test --doc` compiles with default features by default. If the doctest references a feature-gated method, you must either tag the doctest appropriately or ensure it compiles with the feature active.

**How to avoid:** Add `cfg_attr` annotations or use `ignore` on doctests that only compile under specific features:
```rust
/// ```no_run,cfg_attr(not(feature = "rustls-tls"), ignore)
/// // This example only compiles with the rustls-tls feature
/// ...
/// ```
```
Or simply ensure CI runs `cargo test --doc` with `--features rustls-tls`.

**Warning signs:** `cargo test --doc` fails with "no method" when TLS feature not in default.

---

## Code Examples

### Cargo.toml Feature Flag Configuration

```toml
# Source: Cargo features docs + compile_error pattern from Rust reference
[features]
default = ["rustls-tls"]
rustls-tls = ["dep:tokio-rustls", "dep:rustls-native-certs"]
openssl-tls = ["dep:tokio-openssl", "dep:openssl"]

[dependencies]
tokio = { version = "1", features = ["net", "io-util", "time", "rt-multi-thread", "macros"] }
thiserror = "2"
tokio-rustls = { version = "0.26", optional = true }
rustls-native-certs = { version = "0.8", optional = true }
tokio-openssl = { version = "0.6", optional = true }
openssl = { version = "0.10", optional = true }

[dev-dependencies]
tokio-test = "0.4"
```

### Mutual Exclusion Guard (lib.rs)

```rust
// Source: Rust Reference - conditional compilation + compile_error! macro
// Place at top of lib.rs before any module declarations
#[cfg(all(feature = "rustls-tls", feature = "openssl-tls"))]
compile_error!(
    "Feature flags `rustls-tls` and `openssl-tls` are mutually exclusive. \
     Enable only one: `cargo test --features rustls-tls` or \
     `cargo test --no-default-features --features openssl-tls`."
);
```

### GitHub Actions CI Matrix for Dual TLS Features

```yaml
# Source: GitHub Actions docs - strategy.matrix pattern
jobs:
  test:
    name: Test (${{ matrix.tls }})
    runs-on: ubuntu-latest
    strategy:
      matrix:
        tls: [rustls-tls, openssl-tls]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Install OpenSSL dev (openssl-tls only)
        if: matrix.tls == 'openssl-tls'
        run: sudo apt-get update && sudo apt-get install -y libssl-dev pkg-config
      - name: Test
        run: cargo test --no-default-features --features ${{ matrix.tls }}
```

Note: Both features are tested with `--no-default-features --features <one>` to avoid inadvertently enabling both. The clippy job should also run against both feature sets.

### rustls-native-certs 0.8 Correct Usage

```rust
// Source: rustls-native-certs 0.8.3 docs
let native_certs = rustls_native_certs::load_native_certs();
// Log errors but don't abort — some systems have some invalid certs
for error in &native_certs.errors {
    eprintln!("Warning: failed to load cert: {error}");
}
let mut root_store = rustls::RootCertStore::empty();
for cert in native_certs.certs {
    root_store.add(cert).map_err(|e| Pop3Error::Tls(e.to_string()))?;
}
```

### Doctest Pattern for Async Network Methods

```rust
/// Connect to a POP3 server over TLS.
///
/// # Example
///
/// ```no_run
/// use pop3::Pop3Client;
///
/// #[tokio::main]
/// async fn main() -> pop3::Result<()> {
///     let mut client = Pop3Client::connect_tls(
///         ("pop.example.com", 995),
///         "pop.example.com",
///         std::time::Duration::from_secs(30),
///     ).await?;
///     client.login("user", "pass").await?;
///     client.quit().await?;
///     Ok(())
/// }
/// ```
#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
pub async fn connect_tls(...) -> Result<Self> { ... }
```

### Crates.io Publish Checklist

Required Cargo.toml fields (already present, verify current values):
```toml
[package]
name = "pop3"
version = "2.0.0"          # Already set — this IS the publish version
description = "..."         # Present — ensure it's accurate for v2
license = "MIT"             # Present
repository = "..."          # Present — UPDATE to TheDarkSkyXD/rust-adv-pop3
readme = "README.md"        # Add this field; create README.md if missing
keywords = ["pop3", "email", "mail", "client"]  # Present
categories = ["email", "network-programming"]   # Present
```

Publish steps:
```bash
cargo publish --dry-run --features rustls-tls   # Verify packaging
cargo publish --features rustls-tls             # Actual publish
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `rustls::Error` in Pop3Error variant | `Tls(String)` backend-agnostic | Phase 3 | Public API no longer exposes rustls types |
| `rustls` as direct dependency | `tokio-rustls` (re-exports rustls) | Phase 3 | No duplicate crate |
| Hard-coded TLS-only transport | Feature-gated dual backend | Phase 3 | Users choose backend |
| No integration tests (just unit) | tokio_test mock-based integration tests | Phase 3 | Full command flows verified |
| Direct `rustls::Error` `#[from]` | String conversion via `.to_string()` | Phase 3 | Backend-agnostic public API |

**Deprecated/outdated:**
- `Pop3Error::Tls(#[from] rustls::Error)`: Must be replaced with `Tls(String)` before implementing openssl backend
- `Transport::connect_tls()` stub returning "not yet supported": Replace with real implementations under feature flags
- `TlsMode` enum: Was removed in Phase 2 (02-03 decision); do not reintroduce

---

## Open Questions

1. **Transport concrete-type requirement for STARTTLS (TLS-06)**
   - What we know: `Box<dyn AsyncRead/AsyncWrite>` trait objects prevent type-safe reuniting for stream upgrade; `into_inner()` loses buffered data
   - What's unclear: Planner must decide between Option A (fresh TCP reconnect) and Option B (enum-based Transport rewrite). Option B is more correct per RFC 2595 and CONTEXT.md, but adds complexity to Phase 3.
   - Recommendation: Planner should use Option B (enum-based Transport) but scope it narrowly — only add a `TlsStream` enum variant alongside `PlainStream`. Mock variant stays as-is.

2. **STARTTLS hostname parameter on stls()**
   - What we know: CONTEXT.md marks this as Claude's Discretion
   - What's unclear: Should `stls(hostname: &str)` accept a hostname parameter, or should `connect()` store the addr/hostname for later use?
   - Recommendation: Pass `hostname: &str` explicitly to `stls()`. This matches the pattern of `connect_tls()` and makes the dependency explicit. No stored state needed.

3. **Doctest compilation under non-default features**
   - What we know: `cargo test --doc` runs with default features; `connect_tls()` is only present under `rustls-tls` (which IS the default), so doctests compile by default
   - What's unclear: Does `cargo test --doc --no-default-features --features openssl-tls` work correctly for openssl-tls path? The doctests would show rustls-tls examples but compile under openssl-tls feature.
   - Recommendation: Write doctests that show the public API (`connect_tls()`) without referencing which backend is active. Both backends expose the same method name. CI should run `cargo test --doc` with default features only.

4. **README.md existence**
   - What we know: Cargo.toml does not have a `readme` field; no README.md exists in the project root
   - What's unclear: CONTEXT.md says "README update timing (now vs at publish time)" is Claude's Discretion
   - Recommendation: Create README.md as part of Phase 3 (before `cargo publish --dry-run` — crates.io shows it on the crate page). A missing README produces a poor docs.rs experience.

---

## Sources

### Primary (HIGH confidence)
- tokio-rustls GitHub README (https://github.com/rustls/tokio-rustls) — version 0.26.4, TlsConnector API, ServerName, connect pattern
- rustls-native-certs docs.rs 0.8.3 (https://docs.rs/rustls-native-certs) — load_native_certs() return type CertificateResult
- tokio-openssl docs.rs 0.6.5 (https://docs.rs/tokio-openssl) — SslStream, async connect pattern
- tokio-openssl test.rs (https://github.com/tokio-rs/tokio-openssl/blob/master/src/test.rs) — SslConnector.configure().into_ssl(hostname) pattern
- tokio BufReader docs (https://docs.rs/tokio/latest/tokio/io/struct.BufReader.html) — buffer(), into_inner() behavior
- cargo publishing docs (https://doc.rust-lang.org/cargo/reference/publishing.html) — required Cargo.toml fields, dry-run checklist
- rustdoc documentation tests (https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html) — no_run attribute

### Secondary (MEDIUM confidence)
- rustls-native-certs GitHub (https://github.com/rustls/rustls-native-certs) — 0.8 API confirmed; examples/google.rs shows pattern
- Rust Reference compile_error! (verified via multiple sources: Rust forums, reqwest/ring pattern) — confirmed as canonical mutual exclusion approach
- GitHub Actions matrix strategy for Rust TLS feature testing — openssl-tls requires libssl-dev on ubuntu-latest

### Tertiary (LOW confidence)
- STARTTLS Option A (fresh reconnect) vs Option B (in-place upgrade): Architecturally reasoned from tokio I/O docs; not verified against a real POP3 STARTTLS implementation in the Rust ecosystem. Confidence LOW — planner should validate the Transport restructuring design before committing.

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all library versions confirmed via docs.rs and GitHub
- Architecture patterns: HIGH (TLS connect patterns), MEDIUM (STARTTLS upgrade — requires Transport design decision)
- Pitfalls: HIGH — rustls-native-certs API break is confirmed; OpenSSL/Windows CI documented in STATE.md; BufReader into_inner behavior confirmed in tokio docs
- CI matrix pattern: HIGH — standard GitHub Actions feature matrix

**Research date:** 2026-03-01
**Valid until:** 2026-04-01 (30 days — rustls/tokio-rustls are stable but active; openssl-sys Windows situation may improve)
