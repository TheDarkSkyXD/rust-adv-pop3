# Technology Stack

**Analysis Date:** 2026-03-01

## Languages

**Primary:**
- Rust (stable channel) - All library and example code

## Runtime

**Environment:**
- Native binary (no managed runtime)
- Compiled with `rustc` targeting the host platform

**Package Manager:**
- Cargo 1.93.1
- Lockfile: `Cargo.lock` present (listed in `.gitignore`, not committed)

## Frameworks

**Core:**
- None — this is a standalone library crate (`[lib]`), not an application framework

**Testing:**
- Rust built-in test harness (`cargo test`) — no external test framework
- Coverage: `cargo-tarpaulin` used in CI only (not a declared dependency)

**Build/Dev:**
- Cargo (build, test, lint)

## Key Dependencies

**Declared in `Cargo.toml` (v1.0.6 manifest):**
- `openssl` `0.10` — TLS/SSL support for encrypted POP3 connections (port 995); wraps the system OpenSSL library via FFI
- `regex` `1` — POP3 response parsing (pattern matching on `+OK`, `-ERR`, STAT, LIST, UIDL lines)
- `lazy_static` `1` — Static initialization of compiled `Regex` instances to avoid recompiling on every call

**Note — Cargo.lock divergence:**
The `Cargo.lock` resolves the `pop3` package at version `2.0.0` with a completely different dependency set:
- `rustls` `0.23.37` — Pure-Rust TLS implementation (replaces openssl)
- `rustls-native-certs` `0.8.3` — Loads OS trust store for certificate validation
- `thiserror` `2.0.18` — Ergonomic error type derivation

This indicates the lock file reflects an in-progress rewrite branch or unreleased v2.0.0 that has not yet been merged back into `Cargo.toml`. The source file `src/pop3.rs` still uses `openssl`, matching the `Cargo.toml` manifest.

**Transitive dependencies of note (from Cargo.lock):**
- `ring` `0.17.14` — Cryptographic primitives (used by rustls)
- `schannel` `0.1.28` — Windows SChannel TLS (used by rustls-native-certs on Windows)
- `security-framework` `3.7.0` — macOS Security framework (used by rustls-native-certs on macOS)

## Configuration

**Environment:**
- No runtime environment variables required by the library itself
- Consuming applications supply connection parameters (host, port, credentials) as function arguments at call time
- OpenSSL on Linux requires `libssl-dev` system package (enforced in `.travis.yml`)

**Build:**
- `Cargo.toml` — package manifest and dependency declarations
- `.travis.yml` — CI build matrix (stable, beta, nightly channels)

## Platform Requirements

**Development:**
- Rust toolchain (stable channel minimum)
- Linux: `libssl-dev` system package required for openssl crate FFI bindings
- macOS/Windows: system TLS libraries resolved automatically by openssl crate

**Production:**
- Statically links or dynamically links OpenSSL depending on platform and build config
- No runtime service dependencies; library connects directly to any POP3/POP3S server provided by the caller

---

*Stack analysis: 2026-03-01*
