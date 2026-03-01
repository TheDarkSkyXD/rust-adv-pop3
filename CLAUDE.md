# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`pop3` crate — a Rust POP3 client library being rewritten from synchronous v1 to async v2 with Tokio. Published on crates.io. Currently on the `v2.0-async-rewrite` branch.

## Workflow Rules

- Only create PRs in this repo.

## Build & Test Commands

```bash
cargo build              # Build the library
cargo test               # Run all tests (unit tests are inline in source files)
cargo test <test_name>   # Run a single test by name
cargo clippy -- -D warnings  # Lint (must pass with no warnings)
cargo fmt --check        # Check formatting
cargo doc --open         # Build and open rustdoc
```

No CI pipeline yet (planned for Phase 2). No integration tests — all tests use mock I/O.

## Architecture

Five-module library with layered separation:

```
lib.rs          → Public re-exports only (Pop3Client, TlsMode, Pop3Error, Result, types)
client.rs       → Pop3Client struct — all POP3 command methods + auth state tracking
transport.rs    → TCP/TLS connection handling, BufReader I/O, Stream enum, Mock transport (#[cfg(test)])
response.rs     → Pure parsing functions (status line, stat, list, uidl, capa) — no I/O
error.rs        → Pop3Error enum (thiserror) with 8 variants including AuthFailed, InvalidInput
types.rs        → Data structs: Stat, ListEntry, UidlEntry, Message, Capability
```

**Key patterns:**
- `Pop3Client` tracks authentication state via a `bool` — commands that require auth check this first and return `Pop3Error::NotAuthenticated`
- `Stream` enum wraps `TcpStream` or `TlsStream` (and `Mock` under `#[cfg(test)]`)
- Response parsing is separated from I/O: `response.rs` contains pure functions that take `&str` and return `Result<T>`
- Mock transport in `client.rs` tests uses `Rc<RefCell<Vec<u8>>>` to capture sent bytes (single-threaded, no locking)
- CRLF injection protection on all user-supplied strings
- Dot-unstuffing per RFC 1939 for multi-line responses

## Test Infrastructure

Tests are inline `#[cfg(test)]` modules in each source file, primarily in `client.rs`. Two helper constructors:
- `build_test_client(response_data)` — creates unauthenticated mock client
- `build_authenticated_test_client(response_data)` — creates authenticated mock client

Both inject canned server responses and capture client-sent bytes for assertion.

## Project Planning

The `.planning/` directory contains structured planning docs managed by the GSD workflow system:
- `PROJECT.md` — Vision, requirements (v2.0 + v3.0), constraints
- `ROADMAP.md` — 9 phases across two milestones
- `STATE.md` — Current progress, velocity, decisions
- `phases/` — Per-phase plans (PLAN.md), research, and verification artifacts

**Current status:** Phase 1 (Foundation) complete. Phase 2 (Async Core) planned — will migrate to Tokio async/await, add `SessionState` enum, make `quit()` consume `self`, and add GitHub Actions CI.

## Key Design Decisions

- Pure Rust TLS via `rustls` (no OpenSSL dependency in current code; dual backend planned for Phase 3)
- `thiserror` for error derive — `Pop3Error` is the single error type
- `AuthFailed(String)` is a distinct variant from `ServerError(String)` for semantic auth failures
- Mock transport is `#[cfg(test)]`-only — never leaks into public API
- Async rewrite targets Tokio specifically (not async-std)
- `quit()` will consume `self` by value in v2 (compile-time use-after-disconnect prevention)
