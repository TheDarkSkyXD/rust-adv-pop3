# Pitfalls Research

**Domain:** Async Rust protocol client library — adding tokio async, dual TLS backends, STARTTLS, and full RFC 1939 coverage to an existing synchronous Rust POP3 library
**Researched:** 2026-03-01
**Confidence:** HIGH (async Rust pitfalls), HIGH (feature flag pitfalls), HIGH (existing code bugs), MEDIUM (STARTTLS-specific), MEDIUM (openssl build pitfalls)

---

## Critical Pitfalls

### Pitfall 1: Carrying Forward the Existing Bugs During Rewrite

**What goes wrong:**
The v1 codebase has four known bugs: `rset()` sends `RETR` instead of `RSET`, `noop()` sends lowercase `"noop\r\n"`, `is_authenticated` is set before the server confirms the `PASS` command, and `parse_list_one()` uses `STAT_REGEX` instead of a dedicated list regex. A rewrite that mechanically translates the existing logic async-first carries all four bugs into the new codebase. Because the v2 rewrite will touch every line, there is a strong temptation to "port it and fix it later," but post-rewrite debugging without tests is expensive.

**Why it happens:**
Developers focus on the structural migration (sync → async, error propagation, trait design) and treat the command-level bugs as minor and deferrable. Without tests, there is no regression gate to detect when the bugs reappear.

**How to avoid:**
Write tests for each of the four buggy behaviors *before* any migration work begins. Use `tokio_test::io::Builder` to construct a mock POP3 server that asserts the exact bytes received from the client. Fix all four bugs as the very first commits, confirmed by those tests. The test suite then guards against re-introducing the bugs.

**Warning signs:**
- The `rset()` method re-uses any string that contains `"RETR"`.
- The `noop()` method uses any lowercase version of the command string.
- `is_authenticated = true` appears before `read_response` is called in the login path.
- The list-one parser references `STAT_REGEX` by name.

**Phase to address:**
Phase 1 (Error Handling and Bug Fixes) — fix all four bugs before any async work begins.

---

### Pitfall 2: Async Rewrite Without a Test Safety Net

**What goes wrong:**
The existing codebase has zero tests. Rewriting synchronous I/O to async in a single pass with no tests means every behavioral regression is invisible until manual integration testing against a live POP3 server. The `POP3Response` state machine (`add_line`) and all five `parse_*` methods are the most complex logic in the library; they are also the most likely to break silently during a rewrite.

**Why it happens:**
The original codebase's design makes unit testing difficult: `POP3StreamTypes` is a concrete enum of `TcpStream` and `SslStream<TcpStream>`, not a trait, so there is no seam to inject a mock. Developers skip tests because they cannot easily write them, then rely on the code "looking right."

**How to avoid:**
Before touching any I/O or async code, introduce testability:
1. Make the response parser (`POP3Response`, all `parse_*` methods) `pub(crate)` or extract it into a separate module with `#[cfg(test)]` access.
2. Write unit tests for every `parse_*` method using hardcoded server response strings.
3. Introduce a transport trait (`trait Pop3Transport: AsyncRead + AsyncWrite + Unpin`) so the client can be tested with `tokio_test::io::Builder` mocks.
4. Only then migrate I/O to async.

**Warning signs:**
- CI passes on `cargo test` but there are no `#[test]` functions in the codebase.
- `POP3Response` and `parse_*` methods are `fn` (private) with no sibling test module.
- No `tokio_test` or equivalent in `[dev-dependencies]`.

**Phase to address:**
Phase 1 (Error Handling and Bug Fixes) — establish test infrastructure before refactoring begins.

---

### Pitfall 3: Blocking I/O Surviving Into the Async Runtime

**What goes wrong:**
`std::io::Read` and `std::net::TcpStream` block the OS thread. If any blocking I/O call survives the async migration — for example, if `read_response` is called from within an `async fn` but still uses `std::io::Read` under the hood — the tokio worker thread is occupied for the duration of the read. Under the default multi-threaded runtime, this starves other tasks. Under `current_thread`, it deadlocks the entire runtime.

**Why it happens:**
Rust's type system does not prevent mixing sync and async I/O. A method can be declared `async fn` while internally calling blocking `std::io::Read::read()`. The compiler accepts it; only runtime behavior reveals the problem — and only under load.

**How to avoid:**
Replace every `std::io::Read`, `std::io::Write`, and `TcpStream` reference with their tokio equivalents (`tokio::net::TcpStream`, `tokio::io::AsyncReadExt`, `tokio::io::AsyncWriteExt`). Use `tokio::io::BufReader` (not `std::io::BufReader`) for buffered line reading. Audit every `fn` in the migration for any `std::io` import that is not `std::io::Error`. Never call `std::thread::sleep`, `std::io::stdin().read_line()`, or any other blocking primitive inside an `async fn`.

**Warning signs:**
- `use std::io::prelude::*` import surviving in async code.
- `std::net::TcpStream` type appearing anywhere in the new async module.
- `std::io::BufReader` wrapping any stream inside an `async fn`.
- Tests pass in single-threaded mode but hang under `#[tokio::test(flavor = "multi_thread")]`.

**Phase to address:**
Phase 2 (Async I/O Migration) — the entire phase must be audited for blocking I/O survivors.

---

### Pitfall 4: Both TLS Feature Flags Active Simultaneously Causes Compile Errors

**What goes wrong:**
Cargo's feature system is additive: if a downstream user or a transitive dependency enables both `tls-openssl` and `tls-rustls` features simultaneously, both feature branches compile. This produces duplicate type definitions, conflicting trait impls, or link-time conflicts between `openssl-sys` and `ring`/`rustls`. The build fails with confusing errors that appear unrelated to TLS.

**Why it happens:**
Cargo has no native concept of mutually exclusive features. A dependency appearing twice in the dependency graph (one crate enabling `tls-openssl`, another enabling `tls-rustls`) causes both to be active. This is a documented, open issue in Cargo that has no built-in solution.

**How to avoid:**
1. Use a `compile_error!` guard at the top of `lib.rs`:
   ```rust
   #[cfg(all(feature = "tls-openssl", feature = "tls-rustls"))]
   compile_error!("Features 'tls-openssl' and 'tls-rustls' are mutually exclusive. Enable only one.");
   ```
2. Do not make either TLS feature a default — require the user to opt in with exactly one:
   ```toml
   [features]
   tls-openssl = ["dep:openssl"]
   tls-rustls  = ["dep:rustls", "dep:rustls-native-certs"]
   ```
3. Document prominently in `README.md` and `Cargo.toml` that exactly one feature must be selected.
4. Add a `no-tls` or plaintext-only build path gated behind `#[cfg(not(any(feature = "tls-openssl", feature = "tls-rustls")))]` so users who select neither get a clear error rather than a missing-symbol link failure.

**Warning signs:**
- CI matrix does not include a test run with both features enabled simultaneously.
- `#[cfg(feature = "tls-openssl")]` and `#[cfg(feature = "tls-rustls")]` blocks declare the same type names without guards.
- No `compile_error!` in `lib.rs` for the dual-enable case.

**Phase to address:**
Phase 3 (Dual TLS Backends) — define the feature gate structure and compile guard on day one of that phase.

---

### Pitfall 5: STARTTLS Upgrade Leaks Buffered Data Before TLS Handshake

**What goes wrong:**
STARTTLS works by sending a `STLS\r\n` command on a plaintext connection, reading the `+OK` response, then upgrading the same TCP stream to TLS in place. If a `BufReader` wraps the TCP stream before the upgrade, bytes that arrived after `+OK` but before `TcpStream::into_tls()` may be buffered in the reader's internal buffer. After the TLS upgrade replaces the underlying stream, those buffered plaintext bytes are presented to the TLS decoder as TLS records, causing an immediate `bad_record_mac` or `illegal parameter` TLS alert and dropping the connection.

**Why it happens:**
`tokio::io::BufReader` buffers aggressively. After the server sends `+OK Begin TLS negotiation`, the server immediately begins the TLS handshake. The OS may deliver the first TLS `ClientHello` bytes in the same TCP segment as the `+OK` line. A `BufReader::read_line()` call consumes `+OK` but also reads and buffers the initial TLS bytes, which are then lost when the reader is discarded to perform the upgrade.

**How to avoid:**
Before performing STARTTLS, consume the `BufReader` back to the raw stream:
```rust
// In the STARTTLS upgrade path:
let raw_stream = buf_reader.into_inner(); // get the unwrapped TcpStream
let tls_stream = connector.connect(domain, raw_stream).await?;
// Now wrap tls_stream in a new BufReader
```
Never leave a `BufReader` active across a stream type upgrade. Assert that the `BufReader`'s internal buffer is empty before calling `into_inner()` — if it is not, abort the STARTTLS negotiation with an error.

**Warning signs:**
- STARTTLS succeeds against a local test server but fails intermittently against Gmail or Outlook (whose implementations send TLS data immediately after `+OK`).
- TLS handshake errors are `bad_record_mac` or `unexpected_message` rather than certificate errors.
- `buf_reader.buffer().len()` is non-zero immediately after reading the `+OK Begin TLS` response line.

**Phase to address:**
Phase 4 (STARTTLS and Protocol Extensions) — the stream-upgrade logic must explicitly drain the buffer before upgrading.

---

### Pitfall 6: `is_authenticated` State Machine Not Re-Set After STARTTLS or QUIT

**What goes wrong:**
The v1 codebase already sets `is_authenticated` before the server confirms the password (a known bug). In the v2 rewrite, a related problem arises after STARTTLS: RFC 2595 requires that after a successful `STLS` upgrade, the client must discard cached capability information and re-authenticate. If the state machine does not reset `is_authenticated = false` upon performing the TLS upgrade, a client that issues `STLS` on an already-authenticated session may be allowed to skip re-authentication, or may present stale capability data. Similarly, calling methods after `QUIT` on a closed connection should produce an error, not silently succeed.

**Why it happens:**
State machines are hard to enumerate completely. Developers test the happy path (connect → auth → commands → quit) and miss transition edge cases (connect → STARTTLS → re-auth, or QUIT → subsequent command).

**How to avoid:**
Model the POP3 session as a typed state machine:
```rust
struct POP3Client<S: SessionState> { ... }
struct Unauthenticated;
struct Authenticated;
```
Commands that require authentication are only available on `POP3Client<Authenticated>`. STARTTLS transitions from `Unauthenticated` back to a fresh `Unauthenticated` (with TLS active). QUIT consumes the struct (`fn quit(self) -> ...`) so it cannot be used after closing. This makes illegal state transitions compile errors, not runtime bugs.

**Warning signs:**
- `is_authenticated` is a mutable `bool` field rather than a type-state or enum.
- QUIT does not consume `self`.
- No test covers issuing commands after QUIT.
- No test covers issuing commands on a connection where STARTTLS was performed but login was not re-issued.

**Phase to address:**
Phase 2 (Async I/O Migration) — introduce type-state as part of the core client redesign, before STARTTLS is added.

---

## Technical Debt Patterns

Shortcuts that seem reasonable but create long-term problems.

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Keep `panic!` in parse methods during migration, convert later | Unblocks async I/O work faster | Parse errors crash the process in production; tests cannot exercise error paths | Never — fix before shipping any phase |
| Use `String::from_utf8_lossy` instead of `from_utf8` for server responses | Avoids handling UTF-8 error | Silently corrupts email content with replacement characters; masks encoding bugs | Never for a library |
| Skip `compile_error!` guard for mutual TLS exclusion | Saves 3 lines of code | Users who accidentally enable both features get cryptic linker errors | Never |
| Use `unwrap()` on `BufReader::read_line()` in tests | Faster test writing | Panics in tests hide the actual error message | Never in tests |
| Hardcode `set_read_timeout(30s)` without exposing it to callers | Eliminates a config knob | POP3 servers with high latency (slow mailboxes) hit spurious timeouts; callers cannot tune | Only for MVP; expose as builder option in same phase |
| Implement STARTTLS without checking CAPA first | Reduces round trips | Some servers reject `STLS` if they have not advertised `STLS` in `CAPA`; client gets an unhelpful error | Never — always check CAPA first |
| Use `regex` crate for response parsing in v2 | Familiar from v1 | For a line-oriented protocol with fixed prefixes, a regex crate is a heavyweight dependency; simple `starts_with`/`split` is faster and has no compile overhead | Acceptable if already transitively depended upon, but prefer removing it |

---

## Integration Gotchas

Common mistakes when integrating with real POP3 servers.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Gmail POP3 (pop.gmail.com:995) | Sending commands in the wrong state (e.g., `STAT` before `PASS` response) causes `-ERR` with no description | Always await the full response to `USER` before sending `PASS`; always await the `+OK` to `PASS` before any transaction-state command |
| Gmail POP3 STARTTLS (port 110) | Skipping `CAPA` before `STLS` — Gmail requires this negotiation sequence | Issue `CAPA`, verify `STLS` is listed, then issue `STLS`, then TLS handshake |
| Outlook/Microsoft 365 | Assumes the server will not send TLS data in the same TCP segment as the STLS `+OK` | It does. The BufReader drain issue (Pitfall 5) is most visible on Outlook |
| Any POP3 server with APOP | Attempting APOP before parsing the timestamp from the server's greeting banner | The APOP MD5 challenge is in the greeting `+OK <timestamp> message`; parse it on connect or do not offer APOP |
| Servers that send `.\r\n` as the message terminator | Treating a line that begins with a dot-stuffed `.` as the terminator | RFC 1939 requires dot-unstuffing: a line beginning with `..` in multi-line responses has the leading `.` stripped; `.\r\n` alone is the terminator |
| Servers with long response delays | Using the tokio default behavior (no connection timeout) causes tasks to await forever | Set `tokio::time::timeout` on the connect and on each command response |

---

## Performance Traps

Patterns that work at small scale but degrade under real usage.

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Byte-at-a-time reads carried forward from v1 | `RETR` of a 10 MB message takes seconds instead of milliseconds; thousands of syscalls per message | Wrap the stream in `tokio::io::BufReader` with a 8 KB or 64 KB buffer; use `read_line()` | Any message larger than a few KB |
| Unbuffered writes (one write syscall per character) | `write_str("STAT\r\n")` makes a separate syscall for each byte if called character by character | Use `write_all` for each complete command string; do not chunk commands | Visible on any network with non-zero latency |
| Creating a new `Regex` on every parse call | Response parsing takes O(compile) per message instead of O(match) | Keep `LazyLock<Regex>` statics; or replace `Regex` with `str::starts_with` for simple prefix checks | Every response, regardless of message count |
| Holding a `tokio::sync::Mutex` guard across a network `await` | Other tasks that need the connection starve; latency spikes | Use `std::sync::Mutex` for guards not held across await points; for the connection itself, use `&mut` access rather than `Mutex` | Any concurrent usage of the client |
| Spawning a tokio task per message retrieval without backpressure | Memory grows linearly with mailbox size when all messages are retrieved concurrently | Use `tokio::task::JoinSet` with a bounded concurrency limit (e.g., 4 concurrent retrievals) | Mailboxes with more than ~100 messages |

---

## Security Mistakes

Domain-specific security issues for a POP3 client library.

| Mistake | Risk | Prevention |
|---------|------|------------|
| Non-TLS connection sends `USER`/`PASS` in cleartext | Credentials exposed to network observers on any non-HTTPS path | Emit a `#[deprecated]` warning on the plaintext connect path; document in rustdoc that plaintext is insecure; consider removing it entirely in v2 |
| Certificate verification disabled for development convenience | Man-in-the-middle intercepts credentials undetected | Never expose a `verify_none` or `danger_accept_invalid_certs` API path without a loud `#[must_use]` warning and explicit crate-feature opt-in |
| Leaking credentials in `Debug` output for the client struct | Credentials appear in logs | Implement `Debug` manually for any struct that holds a password, omitting the field or replacing it with `"[redacted]"` |
| Silently accepting CAPA response before STARTTLS as authoritative | A MITM strips `STLS` from the pre-TLS CAPA list, preventing upgrade | Per RFC 2595, discard the pre-STARTTLS CAPA response; re-issue `CAPA` after TLS is established and use only that result |
| Hostname in TLS handshake taken from user-provided string without validation | Empty string or mismatched hostname may weaken or bypass certificate validation on some TLS backends | Assert that the hostname is a non-empty, valid DNS name before passing to `connect()`; for openssl, SNI is set automatically by `SslConnector::connect`; verify the same for rustls |

---

## "Looks Done But Isn't" Checklist

Things that appear complete but are missing critical pieces.

- [ ] **Async migration complete:** Verify with `cargo clippy -- -W clippy::blocking_fn_in_async` and search for `std::net::TcpStream` and `std::io::Read` imports in async code paths.
- [ ] **Error handling:** Every `unwrap()` and `panic!` replaced — grep for `unwrap()`, `expect()`, `panic!` in non-test code; zero hits is the target.
- [ ] **Feature flag coverage:** CI matrix must include `--features tls-openssl`, `--features tls-rustls`, and `--features tls-openssl,tls-rustls` (last must produce `compile_error!`).
- [ ] **STARTTLS buffer drain:** After reading `+OK Begin TLS negotiation`, assert `buf_reader.buffer().is_empty()` before calling `into_inner()`.
- [ ] **Dot-unstuffing:** `RETR` and `TOP` responses must strip the leading `.` from dot-stuffed lines and stop at `.\r\n`; test with a message containing a line that begins with `.`.
- [ ] **RFC 1939 state machine:** `STLS` is only valid in AUTHORIZATION state; `RETR`, `DELE`, `TOP`, `UIDL` are only valid in TRANSACTION state; `QUIT` is valid in both; verify these are enforced or documented.
- [ ] **Tests cover error responses:** Every command has a test that feeds `-ERR some message\r\n` and verifies the returned `Err` variant contains the server's message.
- [ ] **`QUIT` consumes the client:** Calling any method after `QUIT` must fail to compile (type-state) or return `Err` (runtime guard). Neither `unwrap()` nor silent success is acceptable.
- [ ] **Rustdoc examples compile:** Run `cargo test --doc`; every `# Examples` block in rustdoc must be a working doctest.
- [ ] **GitHub Actions CI:** Matrix must test on Linux (Ubuntu latest), macOS, and Windows; openssl feature must install `libssl-dev` on Linux; rustls feature must not require system packages.

---

## Recovery Strategies

When pitfalls occur despite prevention, how to recover.

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Known bugs carried into v2 | LOW if caught in phase 1 test phase; HIGH if discovered after publish | Add a mock server test reproducing the exact wrong byte sequence; fix the byte string; re-run full test suite |
| Blocking I/O in async context discovered post-merge | MEDIUM | Use `tokio-console` to identify which task is blocking; replace the blocking call with its async equivalent or wrap with `spawn_blocking` |
| Both TLS features enabled by a downstream user | LOW if `compile_error!` exists; HIGH if it does not (confusing linker errors) | Add the `compile_error!` guard; publish a patch release |
| STARTTLS buffer drain bug in production | HIGH — affects all servers that coalesce TCP segments | Add a test case that reproduces the coalesced-segment scenario using `tokio_test::io::Builder` with adjacent reads; fix `into_inner()` call; patch release |
| openssl cross-compilation fails in CI | MEDIUM | Add `openssl = { features = ["vendored"] }` under the `tls-openssl` feature conditional; document that vendored requires cmake and perl on the build host |
| State machine allows post-QUIT commands | LOW if type-state was used; MEDIUM if runtime guard only | If type-state: fix the type to consume `self` in `quit()`; if runtime guard: add an `is_quit: bool` field and return `Err` on any subsequent call |

---

## Pitfall-to-Phase Mapping

How roadmap phases should address these pitfalls.

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Known bugs carried forward (rset, noop, auth flag, parse_list_one regex) | Phase 1: Error Handling and Bug Fixes | Mock server test asserts exact bytes for rset and noop; test verifies is_authenticated only set after +OK |
| No test safety net for refactoring | Phase 1: Error Handling and Bug Fixes | `cargo test` produces at least one test per parse method and one per POP3 command |
| Blocking I/O surviving async migration | Phase 2: Async I/O Migration | `grep -r "std::net::TcpStream\|std::io::Read" src/` returns zero hits; all tests pass under multi-thread flavor |
| Both TLS features simultaneously active | Phase 3: Dual TLS Backends | CI matrix step with `--features tls-openssl,tls-rustls` must exit non-zero with the `compile_error!` message |
| STARTTLS buffer drain | Phase 4: STARTTLS and Protocol Extensions | Mock server test sends TLS bytes coalesced with the +OK response; connection succeeds |
| State machine post-QUIT and post-STARTTLS | Phase 2: Async I/O Migration | Type-state ensures post-quit calls do not compile; if runtime guard, test asserts Err on post-quit command |
| Dot-unstuffing in multi-line responses | Phase 4: STARTTLS and Protocol Extensions | Test feeds RETR response with a dot-stuffed line (starting with `..`) and verifies single dot in output |
| Blocking mutex held across await | Phase 2: Async I/O Migration | `clippy::await_holding_lock` lint enabled in CI |
| openssl build failure on Linux CI | Phase 3: Dual TLS Backends | GitHub Actions job for tls-openssl must succeed on `ubuntu-latest` with `sudo apt-get install -y libssl-dev` step |
| Missing timeout on network operations | Phase 2: Async I/O Migration | Test that connects to a slow server mock (no response) times out within the configured duration |
| Credentials visible in Debug output | Phase 2: Async I/O Migration | `assert!(!format!("{:?}", client_struct).contains("password"))` test |

---

## Sources

- CONCERNS.md: Known bugs in current codebase (`rset`, `noop`, `is_authenticated`, `parse_list_one`)
- [Common Mistakes with Rust Async — Qovery](https://www.qovery.com/blog/common-mistakes-with-rust-async) — task cancellation, blocking in async, mutex across await, future starvation
- [Unit Testing — Tokio official docs](https://tokio.rs/tokio/topics/testing) — `tokio_test::io::Builder`, mock I/O pattern, paused time
- [Mutually Exclusive Features — Rust Internals Discussion](https://internals.rust-lang.org/t/mutually-exclusive-feature-flags/8601) — Cargo additive feature problem, `compile_error!` workaround
- [RFC 2595 — Using TLS with IMAP, POP3 and ACAP](https://datatracker.ietf.org/doc/html/rfc2595) — STARTTLS sequence requirements, CAPA re-issue after TLS
- [RFC 2449 — POP3 Extension Mechanism](https://datatracker.ietf.org/doc/html/rfc2449) — CAPA command specification
- [RFC 1939 — Post Office Protocol Version 3](https://www.ietf.org/rfc/rfc1939.txt) — dot-stuffing, state machine (AUTHORIZATION vs TRANSACTION), command list
- [Async Rust Pitfalls — Comprehensive Rust (Google)](https://google.github.io/comprehensive-rust/concurrency/async-pitfalls/async-traits.html) — async trait objects, dyn Trait limitations
- [openssl-sys cross-compilation issues](https://github.com/rust-openssl/rust-openssl/issues/2178) — vendored feature requirements, C compiler, perl, make dependencies
- [cargo-semver-checks](https://crates.io/crates/cargo-semver-checks) — SemVer breakage detection for v1→v2 publish
- [Async Rust in Practice — ScyllaDB](https://www.scylladb.com/2022/01/12/async-rust-in-practice-performance-pitfalls-profiling/) — performance pitfalls profiling
- [tokio-tls-upgrade — GitHub](https://github.com/saefstroem/tokio-tls-upgrade) — STARTTLS implementation reference noting lack of documentation and trial-and-error

---

*Pitfalls research for: Async Rust POP3 client library — v1 sync to v2 async rewrite with dual TLS backends*
*Researched: 2026-03-01*
