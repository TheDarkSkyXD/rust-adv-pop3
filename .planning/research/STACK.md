# Stack Research

**Domain:** Async Rust network protocol client library (POP3)
**Researched:** 2026-03-01
**Confidence:** HIGH

---

## Context: What Changes vs. What Stays

This is a milestone research file, not a greenfield project. The existing v1.0.6 codebase
provides synchronous POP3 via `openssl` 0.10. The v2.0 rewrite replaces or wraps all of
the following:

| v1.0.6 (keep/drop) | v2.0 replacement |
|--------------------|------------------|
| `openssl = "0.10"` | keep, but behind `openssl` feature flag; add `tokio-openssl` |
| `regex = "1"` | keep, same version |
| `lazy_static = "1"` | DROP — use `std::sync::LazyLock` (stable since Rust 1.80) |
| Rust 2015 edition | upgrade to Rust 2021 edition |
| Synchronous `std::net::TcpStream` | `tokio::net::TcpStream` |
| `std::io::BufReader` | `tokio::io::BufReader` |
| `std::io::Result` return types | typed `Pop3Error` enum via `thiserror` |

---

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| tokio | 1.49 | Async runtime: TCP sockets, buffered I/O, `#[tokio::test]` | Industry-standard Rust async runtime; largest ecosystem; `TcpStream`, `BufReader`, and `#[tokio::test]` are all in-tree. LTS 1.47.x supported until Sep 2026 (MSRV 1.70). |
| openssl | 0.10.75 | System OpenSSL bindings for TLS | Already in v1.0.6. Keeps existing user expectations. Uses system OpenSSL so no extra compile step on Linux/macOS. Pin as optional behind `openssl` feature flag. |
| tokio-openssl | 0.6.5 | Adapts `openssl::ssl::SslStream` to `AsyncRead + AsyncWrite` | Maintained by tokio-rs. Thin adapter — no new cryptographic code. Only required when `openssl` feature is active. |
| rustls | 0.23.36 | Pure-Rust TLS implementation | No C dependencies; easy cross-compilation; MSRV 1.71. Use `ring` crypto provider (see note below) to avoid `aws-lc-rs` build complexity. Pin as optional behind `rustls` feature flag. |
| tokio-rustls | 0.26.4 | Adapts `rustls` session to `AsyncRead + AsyncWrite` for tokio | Maintained by the rustls project itself. Matches `rustls 0.23.x`. |
| thiserror | 2.0.18 | Derive macro for typed `Pop3Error` enum | Library-quality errors: matchable variants, no boilerplate, does not appear in the public API. Correct choice for libraries (vs. `anyhow` which is for applications). |

### TLS Root Certificates (for `rustls` backend)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| webpki-roots | 1.0.6 | Compiled-in Mozilla root CA bundle for rustls | Default choice; no OS interaction required; deterministic across platforms. Activate when `rustls` feature is enabled. |
| rustls-native-certs | 0.8.1 | Use OS certificate store with rustls | Alternative for users who need corporate/private CAs. More complex; version 0.8.3 has a broken docs.rs build. Offer via a separate `rustls-native-certs` feature if desired in future; not needed for v2.0 MVP. |

### Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio-test | 0.4.5 | Mock `AsyncRead + AsyncWrite` streams for unit tests | In-tree mock I/O; use `tokio_test::io::Builder` to replay byte sequences into the POP3 parser without a live server. Dev dependency only. |
| regex | 1 (keep) | Parse POP3 response lines (STAT, LIST, UIDL) | Already present; no version bump needed. Replace `lazy_static!` wrappers with `std::sync::LazyLock`. |

### Development Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| `cargo clippy` | Lint for common Rust mistakes | Run with `-D warnings` in CI; catches the `pub is_authenticated` issue, `unwrap()` chains, etc. |
| `cargo fmt` (rustfmt) | Enforce consistent formatting | Stable toolchain includes it; run with `--check` in CI. |
| `cargo test` | Run unit and integration tests | Use `#[tokio::test]` for async tests; `tokio-test` for mock I/O. |
| GitHub Actions (`actions-rust-lang/setup-rust-toolchain@v1`) | CI pipeline | Current recommended action; handles toolchain, caching, and problem matchers for clippy/rustfmt output. Replaces broken Travis CI. |

---

## Cargo.toml Structure

The dual-backend feature flag design follows the pattern used by `reqwest`, `imap`, and similar network crates:

```toml
[package]
name = "pop3"
version = "2.0.0"
edition = "2021"
rust-version = "1.80"   # LazyLock stable; tokio LTS MSRV is 1.70, but LazyLock needs 1.80

[features]
# Exactly one TLS backend must be selected; neither is the default (force explicit choice)
openssl = ["dep:openssl", "dep:tokio-openssl"]
rustls  = ["dep:rustls", "dep:tokio-rustls", "dep:webpki-roots"]

[dependencies]
tokio        = { version = "1.49", features = ["net", "io-util", "macros", "rt-multi-thread"] }
thiserror    = "2.0"
regex        = "1"

# TLS backends — both optional, selected via feature flags
openssl      = { version = "0.10", optional = true }
tokio-openssl = { version = "0.6", optional = true }

rustls       = { version = "0.23", default-features = false, features = ["logging", "std", "tls12", "ring"], optional = true }
tokio-rustls = { version = "0.26", optional = true }
webpki-roots = { version = "1.0", optional = true }

[dev-dependencies]
tokio-test = "0.4"
tokio      = { version = "1.49", features = ["rt", "macros"] }
```

**Critical notes on this structure:**

- `rustls` is declared with `default-features = false, features = ["ring"]` to use the `ring` crypto provider instead of the default `aws-lc-rs`. This avoids the `aws-lc-sys` native build (CMake, NASM) that breaks cross-compilation and CI on many platforms. The `ring` provider covers TLS 1.2 and TLS 1.3 fully for client use.
- `tokio` features: `net` for `TcpStream`; `io-util` for `BufReader`/`BufWriter`; `macros` for `#[tokio::main]` and `#[tokio::test]`; `rt-multi-thread` for the full runtime in examples and tests.
- `lazy_static` is removed entirely; use `std::sync::LazyLock` (no external dependency).

---

## Installation

```bash
# For users who want the openssl backend
cargo add pop3 --features openssl

# For users who want the rustls backend (no C dependencies)
cargo add pop3 --features rustls

# Development setup
cargo add --dev tokio-test
```

---

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| `rustls` with `ring` crypto | `rustls` with `aws-lc-rs` (default) | When FIPS-140-3 compliance is required or post-quantum algorithms are needed. Not applicable to a POP3 client library. |
| `tokio-openssl 0.6` | `native-tls` + `tokio-native-tls` | When you want OS TLS (Secure Transport on macOS, SChannel on Windows) with no system OpenSSL requirement. Excluded because v1.0.6 already committed to `openssl` and `native-tls` API surface differs significantly. |
| `tokio-test` mock I/O | Live POP3 test server in CI | Live server tests require credentials, external network, and are flaky. `tokio-test::io::Builder` lets you replay byte-exact POP3 sessions deterministically. |
| `thiserror` | Manual `impl std::error::Error` | Manual impls are fine but generate significant boilerplate for 6+ error variants; `thiserror` produces identical output at zero runtime cost. |
| `std::sync::LazyLock` | `once_cell::sync::Lazy` | `once_cell` is the right choice only if MSRV < 1.80. Since MSRV is 1.80 (for `LazyLock`), there is no reason to add the `once_cell` dependency. |
| `std::sync::LazyLock` | `lazy_static!` | `lazy_static` is explicitly deprecated by the regex crate docs and slower than `LazyLock` in benchmarks. Remove it. |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `lazy_static` | Deprecated upstream; slower than `std::sync::LazyLock`; requires `#[macro_use]` extern crate idiom from Rust 2015 | `std::sync::LazyLock<Regex>` (stable since Rust 1.80) |
| `rustls` default features (`aws-lc-rs`) | Requires C/CMake/NASM toolchain; breaks CI on stripped-down containers; build failures reported across ecosystem | `rustls` with `default-features = false, features = ["ring"]` |
| `async-trait` crate | No longer needed for this use case. Rust 1.75+ has native `async fn` in traits; this library does not use `dyn Trait` for its public API | Native `async fn` in struct `impl` blocks |
| `anyhow` | Application-level error aggregator; hides error variant from library consumers who need to `match` on failures | `thiserror` for typed, matchable `Pop3Error` variants |
| `async-pop` (external crate) | Uses `async-native-tls` only; lacks tokio-native dual-backend design; minimal maintenance | Build in-house as this project IS the `pop3` crate |
| `bytes` crate | Overkill for line-oriented POP3 parsing; adds a dependency for no benefit over `String`/`Vec<u8>` | `String` and `tokio::io::BufReader::read_line()` |
| Travis CI | Free tier for open source discontinued; badge in README is stale/broken | GitHub Actions with `actions-rust-lang/setup-rust-toolchain@v1` |
| Rust 2015 edition | Disables modern import paths, `async`/`await`, and forces `extern crate` declarations | `edition = "2021"` in `Cargo.toml` |

---

## Feature Flag Design (TLS Backend Selection)

```
              pop3 crate
               /      \
     [feature: openssl] [feature: rustls]
          |                    |
    openssl 0.10          rustls 0.23
    tokio-openssl 0.6     tokio-rustls 0.26
                          webpki-roots 1.0
```

**Rules:**
- No default TLS backend — forces users to make an explicit choice in their `Cargo.toml`.
- Both features can theoretically coexist (they compile independently), but the public API should expose a single `TlsConnector` enum that routes to whichever backend is compiled in.
- Use `#[cfg(feature = "openssl")]` and `#[cfg(feature = "rustls")]` at the module level to gate implementation code.

---

## Version Compatibility Matrix

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| `tokio 1.49` | `tokio-openssl 0.6.5` | tokio-openssl targets tokio 1.x |
| `tokio 1.49` | `tokio-rustls 0.26.4` | tokio-rustls 0.26 targets rustls 0.23 and tokio 1.x |
| `rustls 0.23.36` | `tokio-rustls 0.26.4` | tokio-rustls 0.26 is compatible with rustls 0.23; tokio-rustls 0.27 targets rustls 0.24+ |
| `rustls 0.23.36` | `webpki-roots 1.0.6` | webpki-roots 1.x is the current series for rustls 0.23+ |
| `openssl 0.10.75` | `tokio-openssl 0.6.5` | Both are on their respective stable series |
| `thiserror 2.0.18` | All above | thiserror 2.x is a major version bump from 1.x; both work with stable Rust |
| `tokio-test 0.4.5` | `tokio 1.49` | tokio-test 0.4.x targets tokio 1.x |

**MSRV summary:**
- `tokio 1.49` LTS: MSRV 1.70
- `rustls 0.23`: MSRV 1.71
- `std::sync::LazyLock`: stable since Rust 1.80
- **Recommended project MSRV: 1.80** (driven by `LazyLock`; satisfies all dependencies)

---

## GitHub Actions CI Structure

Two jobs are sufficient for v2.0:

```yaml
# .github/workflows/ci.yml
jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        features: ["openssl", "rustls"]
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          components: rustfmt, clippy
      - run: cargo test --features ${{ matrix.features }}
      - run: cargo clippy --features ${{ matrix.features }} -- -D warnings
      - run: cargo fmt --check
```

The matrix tests both TLS backends independently. The `openssl` job on `ubuntu-latest` works because GitHub's Ubuntu runner has OpenSSL installed. No separate Windows/macOS jobs are needed for v2.0.

---

## Sources

- [docs.rs/tokio/latest](https://docs.rs/tokio/latest/tokio/) — version 1.49.0 confirmed; features `net`, `io-util`, `macros`, `rt-multi-thread` verified. Confidence: HIGH.
- [docs.rs/tokio-openssl/latest](https://docs.rs/tokio-openssl/latest/tokio_openssl/) — version 0.6.5 confirmed; AsyncRead/AsyncWrite adapter role verified. Confidence: HIGH.
- [docs.rs/tokio-rustls/latest](https://docs.rs/tokio-rustls/latest/tokio_rustls/) — version 0.26.4 confirmed; rustls 0.23 compatibility confirmed. Confidence: HIGH.
- [docs.rs/rustls/latest](https://docs.rs/rustls/latest/rustls/) — version 0.23.36; MSRV 1.71; `ring` feature flag for pure-Rust alternative to `aws-lc-rs` confirmed. Confidence: HIGH.
- [docs.rs/thiserror/latest](https://docs.rs/thiserror/latest/thiserror/) — version 2.0.18 confirmed. Confidence: HIGH.
- [docs.rs/webpki-roots/latest](https://docs.rs/webpki-roots/latest/webpki_roots/) — version 1.0.6 confirmed. Confidence: HIGH.
- [docs.rs/tokio-test/latest](https://docs.rs/tokio-test/latest/tokio_test/) — version 0.4.5; `io::Builder` mock confirmed. Confidence: HIGH.
- [docs.rs/rustls/latest — ring feature](https://docs.rs/rustls/latest/rustls/) — `default-features = false, features = ["ring"]` pattern to avoid `aws-lc-rs` confirmed via official docs. Confidence: HIGH.
- [doc.rust-lang.org LazyLock](https://doc.rust-lang.org/std/sync/struct.LazyLock.html) — stable since Rust 1.80; replaces `lazy_static`. Confirmed via clippy issue #12895 and community discussion. Confidence: HIGH.
- [github.com/rustls/rustls issue #1877](https://github.com/rustls/rustls/issues/1877) — `aws-lc-rs` vs `ring` conflict and resolution confirmed. Confidence: MEDIUM.
- [actions-rust-lang/setup-rust-toolchain](https://github.com/actions-rust-lang/setup-rust-toolchain) — current recommended GitHub Actions for Rust CI; handles caching and problem matchers. Confidence: HIGH.

---

*Stack research for: rust-adv-pop3 v2.0 async rewrite*
*Researched: 2026-03-01*
