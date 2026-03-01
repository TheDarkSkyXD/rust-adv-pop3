# rust-pop3

## What This Is

A Rust POP3 client library providing async/await access to POP3 email servers with full protocol support. Published on crates.io as the `pop3` crate, it supports both OpenSSL and rustls TLS backends via feature flags.

## Core Value

Provide a correct, async, production-quality POP3 client that handles errors gracefully instead of panicking — so Rust developers can integrate POP3 email retrieval into real applications.

## Requirements

### Validated

<!-- Shipped and confirmed valuable. Inferred from existing v1.0.6 codebase. -->

- ✓ Connect to POP3 servers (plain TCP and SSL) — v1.0.6
- ✓ USER/PASS authentication — v1.0.6
- ✓ STAT (mailbox status) — v1.0.6
- ✓ LIST (message listing, single and all) — v1.0.6
- ✓ UIDL (unique ID listing, single and all) — v1.0.6
- ✓ RETR (retrieve message) — v1.0.6
- ✓ DELE (delete message) — v1.0.6
- ✓ NOOP (keep-alive) — v1.0.6
- ✓ RSET (session reset) — v1.0.6
- ✓ QUIT (disconnect) — v1.0.6

### Active

<!-- Current scope: v2.0 milestone -->

- [ ] Async/await API using tokio
- [ ] Idiomatic Rust API: builder pattern, typed error enum, proper Result types
- [ ] Dual TLS backends: openssl + rustls via feature flags
- [ ] Buffered I/O replacing byte-at-a-time reads
- [ ] Fix known bugs: rset sends wrong command, noop lowercase, auth flag timing, parse_list_one regex
- [ ] Full RFC 1939 support: TOP, CAPA, APOP
- [ ] POP3 extensions: STARTTLS, RESP-CODES
- [ ] Unit tests with mock POP3 server
- [ ] GitHub Actions CI pipeline
- [ ] Rustdoc documentation with examples

### Out of Scope

<!-- Explicit boundaries -->

- IMAP support — different protocol, different crate
- Email parsing/MIME decoding — use `mailparse` or similar crate
- Connection pooling — application-level concern
- OAUTH2/XOAUTH2 authentication — can be added in future milestone
- Sync API wrapper — async-only for v2.0; users can use `block_on` if needed

## Context

- Existing crate published at https://crates.io/crates/pop3 (v1.0.6)
- Original author: Matt McCoy (mattnenterprise). Fork: rust-adv-pop3
- Current codebase is single-file (`src/pop3.rs`, ~537 lines), synchronous, Rust 2015 edition
- Known bugs documented in `.planning/codebase/CONCERNS.md`
- Codebase map available in `.planning/codebase/`
- Zero existing tests — no safety net for refactoring
- Travis CI badge in README is stale/broken

## Constraints

- **Crate name**: Must remain `pop3` for crates.io continuity
- **MSRV**: Target Rust stable (no nightly features)
- **Dependencies**: Minimize — tokio, openssl, rustls behind feature flags
- **Semver**: This is a major breaking change (v1.x → v2.0)

## Current Milestone: v2.0 Full Async Rewrite

**Goal:** Transform the POP3 client from a synchronous, panic-prone v1 library into a modern async Rust crate with full protocol coverage, proper error handling, dual TLS backends, and comprehensive tests.

**Target features:**
- Async/await API with tokio
- Idiomatic Rust API (builder pattern, typed errors)
- Dual TLS backends (openssl + rustls via feature flags)
- Complete POP3 protocol (RFC 1939 + STARTTLS, RESP-CODES)
- Buffered I/O, proper error handling
- Unit tests, mock server, GitHub Actions CI
- Rustdoc with examples

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Async with tokio | Industry standard async runtime for Rust; largest ecosystem | — Pending |
| Dual TLS via feature flags | openssl for compatibility, rustls for pure-Rust builds | — Pending |
| Major version bump to v2.0 | API breaking changes justify semver major | — Pending |
| Drop sync API | Simplifies codebase; sync callers use `block_on` | — Pending |

---
*Last updated: 2026-03-01 after milestone v2.0 initialization*
