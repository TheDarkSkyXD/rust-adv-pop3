# Pitfalls Research

**Domain:** Async Rust POP3 client library — adding pipelining, UIDL caching, reconnection, connection pooling, and MIME integration to an existing async tokio-based v2.0 codebase
**Researched:** 2026-03-01
**Confidence:** HIGH (pipelining RFC requirements, cancel-safety, connection pool protocol mismatch), HIGH (UIDL reuse documented in RFC 1939 and Microsoft Learn), MEDIUM (MIME integration gotchas from mailparse/mail-parser docs), MEDIUM (reconnection state machine pitfalls from tokio forum discussion)

---

## Critical Pitfalls

### Pitfall 1: Pipelining Without Verifying PIPELINING Capability First

**What goes wrong:**
The client sends multiple POP3 commands in a batch before receiving responses, assuming the server supports RFC 2449 PIPELINING. Servers that do not advertise PIPELINING in their CAPA response are not required to handle out-of-sequence or batched commands — they may respond to the first command only, drop subsequent commands, or send `-ERR` for every command after the first. The result is a silent partial failure: some commands succeed, others are silently dropped, and the client misinterprets the server's single response as the response to command N.

**Why it happens:**
RFC 2449 clearly states clients MUST only pipeline if the server advertises PIPELINING in its CAPA response. Developers test against Gmail or Outlook — both of which advertise PIPELINING — and never test against a server that doesn't. The code works in development, breaks silently in production against a self-hosted Dovecot or Postfix with a different configuration.

**How to avoid:**
Issue `CAPA` on every connection (after authentication, in TRANSACTION state). Parse the CAPA response. Store it as a `Capabilities` struct on the `Client`. Gate all pipelining code paths behind a `capabilities.supports_pipelining()` check. Fall back to sequential send-wait-receive-repeat when PIPELINING is absent. Make the fallback the default code path; pipelining is the fast path, not the assumed path.

```rust
// In the pipelining code path:
if !self.capabilities.pipelining {
    return self.send_sequential(commands).await;
}
self.send_pipelined(commands).await
```

**Warning signs:**
- Tests only run against a mock server that always advertises PIPELINING.
- No test covers issuing batched commands against a mock that does NOT advertise PIPELINING.
- `CAPA` is parsed once at connect-time, not after re-authentication following a reconnect (capabilities may differ per session state).

**Phase to address:**
Phase 1 (Pipelining Implementation) — capability check must be the first thing implemented, not an afterthought.

---

### Pitfall 2: Pipelining Deadlock Due to TCP Send Buffer Saturation

**What goes wrong:**
When the client sends many large POP3 commands (e.g., a batch of 50 `RETR` commands) without reading responses, the server's TCP receive buffer fills. The server cannot drain its receive buffer because its own send buffer (holding the `RETR` responses) is also full — the client isn't reading. The TCP window closes on both sides. Both client and server block on writes while waiting for the other to read. This is a classic TCP half-duplex deadlock, and it is specifically called out in RFC 2449 Section 6.6: "If either the client or server uses blocking writes, it MUST not exceed the window size of the underlying transport layer."

**Why it happens:**
Naive pipelining implementations send all commands first, then read all responses. This works for small command batches but deadlocks as batch size grows. Tokio's `write_all` is async, but if the kernel send buffer is full, `write_all` will not complete until data is drained — and data is not drained until the server reads it — which the server cannot do while its own send buffer is full from trying to send responses the client isn't reading.

**How to avoid:**
Interleave writes and reads. Use `tokio::select!` or spawn the write loop and read loop as separate tasks. A practical pattern is a "sliding window" pipeline: track the number of outstanding (sent but unacknowledged) commands. Limit the window to a small count (4–8 commands is typically sufficient). Send a new command only when a response has been received for the oldest outstanding command.

```rust
// Pseudocode for windowed pipelining:
const PIPELINE_WINDOW: usize = 8;
let mut pending: VecDeque<CommandType> = VecDeque::new();

for cmd in commands {
    // Drain responses if window is full
    while pending.len() >= PIPELINE_WINDOW {
        let response = read_response().await?;
        process_response(pending.pop_front().unwrap(), response)?;
    }
    send_command(&cmd).await?;
    pending.push_back(cmd);
}
// Drain remaining
while let Some(cmd) = pending.pop_front() {
    let response = read_response().await?;
    process_response(cmd, response)?;
}
```

**Warning signs:**
- Pipelining implementation sends ALL commands before reading ANY responses.
- No bound on the number of outstanding pipelined commands.
- Load test with a mailbox of 100+ messages hangs indefinitely.
- Tests only use small synthetic mailboxes (1–5 messages).

**Phase to address:**
Phase 1 (Pipelining Implementation) — the windowed send-receive interleaving must be the design basis, not a retrofit.

---

### Pitfall 3: Response Ordering Assumptions Break When Pipelining RETR

**What goes wrong:**
RFC 2449 guarantees that a server advertising PIPELINING will process commands in the order received and return responses in the same order. However, the client must maintain a queue of sent commands that maps each response to the command that generated it. If the client loses track of ordering — for example by processing responses in a concurrent task without a shared ordered queue — it may attribute a `RETR 3` response body to message 2, or attribute a `-ERR` (for a deleted message) to the wrong command. This causes silent data corruption: the wrong message content is returned under the wrong message number.

**Why it happens:**
Developers use `tokio::spawn` to send commands concurrently and collect results into a `JoinSet` or similar. `JoinSet::join_next()` returns futures in completion order, not submission order. POP3 responses arrive in submission order. The mismatch silently corrupts the association between command and response.

**How to avoid:**
Maintain a `VecDeque<oneshot::Sender<Result<Response>>>` as the pending command queue. When sending a command, push a sender. When reading a response from the server, pop the front sender and send the response through it. The reader task is a single sequential loop — it NEVER races. The writer task may batch sends but always pushes to the queue in the same order commands are sent.

```rust
// Command ordering via ordered queue:
// sender side:
let (tx, rx) = oneshot::channel();
pending_queue.push_back((CommandType::Retr(msg_num), tx));
writer.write_all(format!("RETR {}\r\n", msg_num).as_bytes()).await?;

// receiver side (sequential, never concurrent):
while let Some((cmd, tx)) = pending_queue.pop_front() {
    let response = read_response_for(&cmd).await?;
    let _ = tx.send(response);
}
```

**Warning signs:**
- Pipelining uses `tokio::spawn` for both sending and receiving responses.
- Response collection uses `FuturesUnordered` or `JoinSet::join_next()` without position tracking.
- Test corpus does not include a mailbox where message N has a different size than message N+1 (making ordering bugs detectable).

**Phase to address:**
Phase 1 (Pipelining Implementation) — ordered queue must be designed before any concurrent send/receive code is written.

---

### Pitfall 4: UIDL Cache Not Invalidated After Server-Side Deletion

**What goes wrong:**
POP3 UIDL values can, in principle, be reused by a server after a message is deleted. RFC 1939 says servers SHOULD NOT reuse UIDs, but this is advisory. More common in practice: a message is deleted in one session but the local UIDL cache is not updated before the connection closes (e.g., due to a crash or timeout). On the next connection, the cache considers that UIDL "seen" and skips re-downloading a new message that the server has assigned the same (or identical hash-based) UIDL. The user never sees the new message.

**Why it happens:**
UIDL caches are typically write-on-delete (remove the UIDL when the user deletes). But cache invalidation should also happen on reconnect: compare the current server UIDL list against the cache and remove any cached UIDs no longer present on the server. Skipping this reconciliation step causes the cache to accumulate "ghost" entries for messages that no longer exist, and to miss new messages with coincidentally identical UIDs.

**How to avoid:**
On each new session, issue `UIDL` (all) immediately after authentication. Reconcile the result with the cache:
1. Any UID in the cache that is NOT in the server's current UIDL list: remove from cache (the message was deleted, possibly from another client or via webmail).
2. Any UID in the server's list that is NOT in the cache: mark as "new", queue for download.
3. Any UID in both: mark as "seen", skip.

Never treat the local cache as authoritative. The server's `UIDL` response is always the source of truth.

```rust
pub struct UidlCache {
    seen: HashSet<String>,  // UIDs confirmed seen on server in a previous session
}

impl UidlCache {
    pub fn reconcile(&mut self, server_uidls: &[(u32, String)]) -> Vec<(u32, String)> {
        let server_set: HashSet<&str> = server_uidls.iter().map(|(_, uid)| uid.as_str()).collect();
        // Remove ghosts: UIDs we cached but server no longer has
        self.seen.retain(|uid| server_set.contains(uid.as_str()));
        // Return new messages: server has them but we haven't seen them
        server_uidls.iter()
            .filter(|(_, uid)| !self.seen.contains(uid.as_str()))
            .cloned()
            .collect()
    }
}
```

**Warning signs:**
- UIDL cache is only written, never pruned.
- No test deletes a message via one simulated session and then verifies the cache is updated on the next simulated session.
- `incremental_sync()` uses a `HashMap::contains_key()` check without first reconciling against the live UIDL list.

**Phase to address:**
Phase 2 (UIDL Caching) — the reconciliation loop must be part of the initial cache design, not a later correctness fix.

---

### Pitfall 5: UIDL Values Are Not Globally Unique — Only Per-Maildrop

**What goes wrong:**
RFC 1939 defines UIDL uniqueness within a single maildrop (mailbox), not across mailboxes or accounts. A client that manages multiple accounts and uses a single shared UIDL cache will experience cross-account collisions: a UID from account A may match a UID from account B, causing messages from account B to be skipped because the cache "thinks" they were already seen in account A's context.

**Why it happens:**
Developers test with a single-account scenario, where per-maildrop uniqueness is sufficient. Multi-account support is added later, and the cache is shared by convenience.

**How to avoid:**
Namespace the UIDL cache by account. The key should be `(username, hostname, port)` or a hash thereof — not just the UIDL string. Each account's cache is entirely independent.

```rust
pub struct UidlCache {
    // Key: account identity (username + host + port), Value: seen UIDs
    per_account: HashMap<AccountKey, HashSet<String>>,
}
```

**Warning signs:**
- `UidlCache` stores UIDs as a flat `HashSet<String>` without any account namespace.
- No test exercises two accounts where both servers happen to assign a message the same UIDL.

**Phase to address:**
Phase 2 (UIDL Caching) — namespace the cache key from day one.

---

### Pitfall 6: Reconnection Reuses Exhausted Futures — Tokio Future Completion Is One-Shot

**What goes wrong:**
An automatic reconnection loop that reuses the same `tokio::signal` future, `tokio::time::sleep` future, or any other future that has already completed will panic at runtime. Tokio futures are one-shot: once a future resolves, it cannot be awaited again. A reconnection loop that does `loop { select! { _ = &mut shutdown_signal => break; _ = connect().await => ... } }` without creating a fresh `shutdown_signal` future on each iteration will panic on the second reconnection attempt with a `poll after completion` error.

**Why it happens:**
Developers come from callback-based async models where event handlers can be called multiple times. Rust futures are state machines that reach a terminal state on completion. The compiler does not prevent re-awaiting a completed future through a mutable reference — the panic only occurs at runtime.

**How to avoid:**
Create fresh futures on every iteration of the reconnection loop. Do not store futures in variables outside the loop body if they must be live for the entire loop.

```rust
loop {
    // WRONG: shutdown_rx created outside loop, reused on 2nd iteration
    // let mut shutdown = tokio::signal::ctrl_c();

    // CORRECT: fresh future each iteration
    let shutdown = tokio::signal::ctrl_c();
    tokio::select! {
        _ = shutdown => { break; }
        result = attempt_connect(&config) => {
            match result {
                Ok(client) => run_session(client).await,
                Err(e) => {
                    tracing::warn!("Connection failed: {e}, retrying...");
                    tokio::time::sleep(backoff.next()).await;
                }
            }
        }
    }
}
```

**Warning signs:**
- Reconnection loop stores a `tokio::signal` or `sleep` future in a variable declared outside the `loop {}` block.
- No test simulates two consecutive reconnections (the first works, the second panics).
- Test coverage for the reconnection path only covers the "first attempt succeeds" case.

**Phase to address:**
Phase 3 (Reconnection with Exponential Backoff) — the loop structure must be reviewed for future reuse before any reconnection code ships.

---

### Pitfall 7: Reconnection Does Not Reset Session State — Commands Issued in Wrong State

**What goes wrong:**
After a reconnection, the v2.0 `Client` is in `SessionState::Authorization`. If the reconnection wrapper reuses the same `Client` struct with its state preserved from before the drop — or if the reconnection logic does not re-authenticate before issuing commands — the next command fails with a `-ERR` or `NotAuthenticated` error. In the worst case, if the session state is incorrectly left as `Transaction` after reconnect (because the reconnection code only replaces the stream, not the state), commands are sent before the server is ready.

**Why it happens:**
Reconnection logic is often layered on top of an existing `Client` without reviewing which parts of the client state must be reset. Developers replace the TCP stream and TLS layer but overlook resetting session state, cached capabilities, and the pending command queue (if pipelining is active).

**How to avoid:**
Reconnection must construct a brand-new `Client` from scratch, not patch the existing one. After establishing the new TCP/TLS connection and reading the greeting, re-authenticate using stored credentials. Re-issue `CAPA` and update the stored capabilities. Only then resume the command queue. Make this explicit in the reconnection wrapper's interface: it consumes the old (broken) `Client` and produces a new authenticated one.

```rust
async fn reconnect(config: &ClientConfig) -> Result<Client, Pop3Error> {
    // Always build from scratch — do NOT reuse the broken client's state
    let client = Client::connect(config).await?;
    client.login(&config.username, &config.password).await?;
    Ok(client)
}
```

**Warning signs:**
- Reconnection patches `self.stream` in place without resetting `self.state`.
- No test verifies that `client.stat()` succeeds immediately after a simulated reconnect.
- The reconnection code does not call `login()` before returning.

**Phase to address:**
Phase 3 (Reconnection with Exponential Backoff) — design the reconnection wrapper to produce a fresh `Client`, not modify an existing one.

---

### Pitfall 8: Exponential Backoff Without Jitter Causes Thundering Herd

**What goes wrong:**
When multiple instances of an application use the same POP3 library against the same server, and the server becomes temporarily unavailable, all clients fail at approximately the same time. If backoff uses a pure exponential formula without jitter (delay = base * 2^attempt), all clients will retry at exactly the same intervals. They hit the recovering server simultaneously, overload it again, and the cycle repeats. This is the "thundering herd" problem.

**Why it happens:**
Pure exponential backoff is the simplest formula to implement. Jitter requires additional thought (and a random number generator). Developers copy the basic formula from an example.

**How to avoid:**
Use the "full jitter" strategy: actual delay = random(0, base * 2^attempt). Cap the maximum delay at a sensible ceiling (30–60 seconds). Use the `backon` crate (actively maintained as of 2025; the `backoff` crate is unmaintained) which implements jitter natively:

```rust
use backon::{ExponentialBuilder, Retryable};

let result = connect_to_server
    .retry(ExponentialBuilder::default()
        .with_jitter()
        .with_max_delay(Duration::from_secs(60))
        .with_max_times(10))
    .await?;
```

**Warning signs:**
- Reconnection delay is `Duration::from_secs(2_u64.pow(attempt))` without any `rand::thread_rng()` or jitter.
- The `backoff` crate (unmaintained) is used instead of `backon`.
- No test simulates concurrent reconnections from multiple client instances.

**Phase to address:**
Phase 3 (Reconnection with Exponential Backoff) — use `backon` with jitter from the start; do not implement the retry loop manually.

---

### Pitfall 9: Connection Pool Fundamentally Mismatches POP3's Exclusive-Lock Model

**What goes wrong:**
POP3 is defined by RFC 1939 as an exclusive-access protocol: when a client authenticates, the server acquires an exclusive lock on the maildrop. Any second client that attempts to authenticate against the same mailbox receives a negative response (commonly `-ERR [IN-USE] Maildrop already locked`). A conventional connection pool — which maintains N pre-authenticated connections and hands them out to concurrent callers — will have N-1 connections that always fail to authenticate. The pool degrades to a single-connection pool at best; at worst it fills with failed connections, starves all callers, and deadlocks on the pool's own checkout timeout.

**Why it happens:**
Connection pools are a familiar pattern from HTTP and database clients (bb8, deadpool, r2d2). Developers assume the same pattern applies. The POP3 exclusive-lock constraint is not obvious from the API surface.

**How to avoid:**
A POP3 "connection pool" can only pool connections if each pooled connection targets a different account/mailbox. Design the pool as a per-account connection pool with a max-connections-per-account of 1. The practical use case for pooling in POP3 is managing connections for many different accounts (e.g., a mail aggregator), not concurrent access to one account. Make this constraint explicit in the type signature:

```rust
pub struct Pop3Pool {
    // One connection per (username, host, port) tuple at most
    connections: HashMap<AccountKey, Option<Client>>,
    max_idle: Duration,
}
```

Document prominently: attempting to acquire two connections for the same account will block until the first is returned, or time out if the configured wait limit is exceeded.

**Warning signs:**
- Pool implementation is `Vec<Client>` without any per-account keying.
- Pool checkout does not check if a connection for the same account is already checked out.
- No test demonstrates that a second checkout for the same account blocks or times out rather than returning a second connection.
- README uses the term "connection pool" without documenting the per-mailbox exclusive access constraint.

**Phase to address:**
Phase 4 (Connection Pooling) — the per-account exclusive-lock constraint must be the foundation of the pool design, stated in the type system, not a runtime check.

---

### Pitfall 10: MIME Parsing Receives Raw RETR Output Including Dot-Stuffed Lines

**What goes wrong:**
The output of a POP3 `RETR` command is dot-stuffed per RFC 1939: any line that begins with a `.` in the actual message has been prefixed with an additional `.` by the server. The client's `read_until_dot_crlf()` routine strips the dot-stuffing and removes the terminal `.\r\n`. If the caller passes the raw server output (before dot-unstuffing) to `mailparse::parse_mail()` or `mail_parser::MessageParser::parse()`, the MIME parser receives malformed input with doubled-dot lines. It may parse them as literal content, fail to decode base64 parts whose line boundaries were altered, or silently corrupt encoded message bodies.

**Why it happens:**
The responsibility boundary between "POP3 transport" and "MIME parsing" is unclear. A developer wires up: `let raw = client.retr(1).await?; mailparse::parse_mail(raw.as_bytes())`. If `retr()` returns the server wire bytes (with dot-stuffing intact), the MIME parser receives garbage. The bug is invisible unless the test message includes a line beginning with `.`.

**How to avoid:**
Guarantee that `client.retr()` always returns the dot-unstuffed message content — the final `String` returned by the public API must be the actual message bytes, not the POP3-wire bytes. Verify this in the v2.0 test suite with a message that contains a dot-stuffed line before any MIME integration code is written. The MIME integration layer must receive clean RFC 5322 content and must never deal with POP3 framing.

```rust
// The boundary: RETR returns clean RFC 5322 content, not POP3 wire format
let raw_message: String = client.retr(1).await?;
// raw_message has: dot-stuffing removed, terminal ".\r\n" stripped
// Safe to pass directly to MIME parser:
let parsed = mailparse::parse_mail(raw_message.as_bytes())?;
```

**Warning signs:**
- `retr()` returns a `Vec<String>` or `String` without documentation stating whether dot-unstuffing has been applied.
- The MIME integration test corpus does not include a message with a body line beginning with `.`.
- No assertion in `retr()` tests that a server response containing `..fake dot stuffed\r\n` produces `fake dot stuffed` (single dot removed) in the output.

**Phase to address:**
Phase 5 (MIME Integration) — verify dot-unstuffing in v2.0 RETR tests before v3.0 MIME work begins. The MIME layer must receive clean content as a precondition.

---

### Pitfall 11: `mailparse` vs `mail-parser` — Wrong Crate Behind the Feature Flag

**What goes wrong:**
`mailparse` has two documented limitations that are critical for POP3 use cases:
1. The `addrparse()` function produces incorrect results when parsing headers containing encoded words (RFC 2047); callers must use `addrparse_header()` instead, which is an easy mistake to make.
2. `dateparse()` "may fail to parse some of the more creative formattings" of Date headers.
3. Only 87% of the API is documented, meaning some behaviors are undiscovered until production.

By contrast, `mail-parser` (by Stalwart Labs) is zero-copy, handles 41 character encodings including UTF-7, has no external dependencies, conforms to RFC 5322 and MIME RFCs 2045-2049 fully, and uses `Cow<str>` returns to avoid heap allocation. It is more robust, better documented, and higher-performance for the POP3 use case.

**Why it happens:**
`mailparse` is the older, better-known crate and appears first in web searches. Developers pick it because it's familiar.

**How to avoid:**
Use `mail-parser` as the default MIME backend. Keep it behind a feature flag (`mime` or `mailparse`). The feature flag name should match the capability, not the crate:

```toml
[features]
mime = ["dep:mail-parser"]

[dependencies]
mail-parser = { version = "0.9", optional = true }
```

If users need `mailparse` specifically, offer a second feature flag. Default to the better library. Document the choice in the crate-level rustdoc.

**Warning signs:**
- `mailparse` is chosen without comparison to `mail-parser`.
- No test exercises a message with a non-ASCII header value (e.g., a UTF-8 encoded From: header).
- `addrparse()` is used directly on a `MailHeader` value (instead of `addrparse_header()`).
- No test exercises a message with an unusual Date: format.

**Phase to address:**
Phase 5 (MIME Integration) — evaluate both crates against real-world test messages (including non-ASCII, multipart, and unusual date formats) before committing to one.

---

### Pitfall 12: `write_all` Cancel-Safety in Reconnection Path Causes Partial Command Writes

**What goes wrong:**
When a timeout or cancellation interrupts a `write_all` call mid-write (e.g., during command transmission in the reconnection path), only a partial command may have been sent to the server. The server receives a truncated command — for example `RETR` without the message number and `\r\n`. The server's response to a truncated command is undefined: it may hang waiting for more bytes, send `-ERR syntax`, or close the connection. The client, having lost the context of what was partially written, cannot safely resume the session.

This is an inherent cancel-safety issue with `tokio::io::AsyncWriteExt::write_all`: if the future is dropped before completion, the amount of data written is unknown.

**Why it happens:**
Reconnection wrappers often wrap commands in `tokio::time::timeout`. If the timeout fires during a write (not just a read), the write is partially completed. The caller receives `Err(Elapsed)` and retries the command on a new connection without knowing the server saw partial data.

**How to avoid:**
Never cancel a `write_all` via a timeout. Wrap ONLY the response-read path in a timeout. Use a separate `tokio::select!` that uses `tokio::io::AsyncWriteExt::write_all_buf` (which uses a cursor to track partial progress and can be safely resumed) for writes, combined with a timeout only on the read:

```rust
// Safe pattern: timeout only the read, not the write
writer.write_all(cmd.as_bytes()).await?;  // writes fully or returns Err
tokio::time::timeout(
    Duration::from_secs(30),
    reader.read_line(&mut response_buf),
).await??;  // timeout only on awaiting the response
```

If the write itself fails (not a timeout, but an actual I/O error), close the connection immediately. Do not attempt to recover a session where a partial command write occurred.

**Warning signs:**
- A `tokio::time::timeout(duration, client.some_command())` call wraps the entire command including the write.
- No test injects a write failure mid-command and verifies the connection is terminated rather than reused.
- The reconnection wrapper retries a command on a new connection without verifying the old connection was cleanly closed first.

**Phase to address:**
Phase 3 (Reconnection with Exponential Backoff) — the timeout placement decision must be made before the reconnection wrapper is designed.

---

## Technical Debt Patterns

Shortcuts that seem reasonable but create long-term problems.

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Pipeline all RETR commands without a window limit | Simpler code (send all, read all) | TCP deadlock for large mailboxes; hangs indefinitely with no timeout | Never — windowed pipelining is not much more complex |
| UIDL cache as a `HashSet<String>` without account namespacing | Simple code | Cross-account UID collisions in multi-account use cases; messages silently skipped | Only if the library explicitly only supports single-account use (should be documented) |
| Skip the `CAPA` check before pipelining | One fewer round-trip on every connect | Broken pipelining against non-PIPELINING servers; hard-to-debug failures | Never |
| Implement reconnection as a loop with `tokio::time::sleep(fixed_duration)` | One line of code | Thundering herd on multi-instance deployments; no jitter; fixed delay regardless of error type | Never in production code |
| Use `mailparse::addrparse()` directly on raw header value strings | Familiar API | Incorrect address parsing for encoded-word headers; subtle data corruption | Never — use `addrparse_header()` exclusively |
| Share a `UidlCache` across multiple accounts | Simpler data structure | UID namespace collisions; messages from account B skipped because a matching UID was seen in account A | Never |
| Pool POP3 connections like HTTP connections (N connections per account) | Familiar pooling pattern | N-1 connections always fail with IN-USE; pool starvation and timeout; user-visible hangs | Never — POP3 pools must be per-account, max 1 concurrent |
| Wrap entire commands (write + read) in `tokio::time::timeout` | Simple timeout code | Partial writes cause undefined server state; subsequent commands on same connection produce wrong responses | Never for write path; acceptable for read-only path |

---

## Integration Gotchas

Common mistakes at the boundaries between v3.0 features and external systems.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| CAPA response in AUTHORIZATION state | POP3 servers may advertise different capabilities in AUTHORIZATION vs TRANSACTION state. Issuing `CAPA` before login and caching the result may miss capabilities only available post-auth (or vice versa). | Issue `CAPA` after successful authentication. Cache capabilities per session, not per connection. |
| Server that does not support UIDL | Issuing `UIDL` on a server that does not advertise `UIDL` in CAPA returns `-ERR`. If the UIDL caching code assumes UIDL is always available, it panics or returns a misleading error. | Check `capabilities.uidl` before issuing `UIDL`. Fall back to message-number-based sync (with a clear warning that incremental sync is unavailable). |
| MIME parsing of multipart messages | `mailparse::parse_mail()` returns a `ParsedMail` with `.subparts` for multipart messages. Accessing only `.get_body()` on the top-level message returns empty string for multipart messages — the body is in the subparts. | Recurse over `.subparts` when `content_type.mimetype` is `"multipart/..."`. Provide a utility function that flattens text/html body parts. |
| `mail-parser` with POP3 RETR output (CRLF) | `mail-parser` handles both LF and CRLF. However, `mailparse` was documented to accept `\n` as line delimiters — meaning it silently normalizes. If code written for `mailparse` is ported to `mail-parser`, CRLF handling behavior may differ in edge cases. | Use the same test corpus against both parsers during evaluation. Normalize line endings to `\n` before passing to either parser if cross-crate compatibility is needed. |
| Reconnection during an active pipelined batch | If the connection drops mid-pipeline (after some commands sent, before all responses received), the reconnect must discard the entire partial batch and re-issue all commands from scratch. There is no safe way to know which pipelined commands the server processed before the drop. | On connection error during pipelining, fail the entire batch with a retriable error. The caller retries the full batch on the new connection. |
| Connection pool checkout timeout vs. POP3 IN-USE | If a pool checkout times out because the per-account connection is in use, the error surfaced to the caller should be `AccountBusy` (the account is already being accessed), not a generic timeout. A timeout on a database pool means "no free connections"; a POP3 pool timeout means "the mailbox is locked". | Map pool checkout timeout to a specific `Pop3Error::AccountInUse` variant with documentation explaining the POP3 exclusive lock model. |

---

## Performance Traps

Patterns that work at small scale but degrade under real usage.

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Pipelining without window limit on large mailboxes | `sync_all()` hangs indefinitely for mailboxes with 100+ messages; memory grows linearly | Implement windowed pipelining with max 4–8 outstanding commands | Any mailbox over ~50 messages on a high-latency connection |
| Full UIDL download on every connect (no caching) | Every incremental sync downloads the full UIDL list even when 0 new messages exist | Persist the UIDL cache to disk using `serde` + JSON or bincode; reconcile on reconnect | Mailboxes with 1000+ messages; the UIDL response alone is large |
| UIDL cache held in `HashMap<String, ()>` with `String::clone()` per entry | Memory pressure grows proportionally to number of cached UIDs | Use `Arc<str>` for shared UID strings; consider a bloom filter for presence checks at scale | Mailboxes with 10,000+ messages (uncommon but possible for aggregators) |
| MIME parsing inside the POP3 response read loop | Each message is parsed before the next is fetched; pipelining is defeated by per-message parse latency | Fetch all messages first (pipelined), then parse in a separate async pipeline | Any mail fetch + parse pipeline where MIME parsing is slower than network I/O |
| Reconnection on every transient network error without distinguishing error types | Reconnects on `ETIMEDOUT` (connection reset; reconnect is correct) but also reconnects on `-ERR [AUTH] ...` (authentication failure; reconnecting will always fail) | Classify errors: `Pop3Error::Io(e)` → reconnect eligible; `Pop3Error::AuthFailed` → do not retry without new credentials | First occurrence of an authentication failure with reconnect enabled |

---

## Security Mistakes

Domain-specific security issues for v3.0 features.

| Mistake | Risk | Prevention |
|---------|------|------------|
| Storing UIDL cache to disk with credentials embedded | Cache file leaks account information if filesystem permissions are loose | Cache file stores ONLY UIDs — no usernames, passwords, or hostnames unless the user explicitly provides a path with namespace-aware naming |
| Exponential backoff without a maximum attempt cap | Infinite retry loop holds credentials in memory and continuously attempts authentication against an account that may have been disabled or suspended | Cap maximum retries at 10 (configurable); after cap, return a non-retriable error requiring user action |
| MIME feature flag enabled by default pulling in transitive dependencies | Security surface of the crate grows unexpectedly; users who do not need MIME parsing are forced to compile and link MIME parsing code | Keep `mime` feature flag opt-in (not default); document that enabling it pulls in `mail-parser` |
| Reconnection retries authentication with credentials cached from initial connect | If credentials change between sessions (e.g., password rotation), the reconnect loop retries with stale credentials repeatedly until max attempts | Surface `Pop3Error::AuthFailed` immediately without retry; do not loop on authentication failures |

---

## "Looks Done But Isn't" Checklist

Things that appear complete but are missing critical pieces.

- [ ] **Pipelining:** Verify CAPA check is present — test against a mock server that does NOT advertise PIPELINING; confirm sequential fallback is used.
- [ ] **Pipelining:** Verify windowed sends — use a mailbox with 200 messages and confirm the test completes without hanging.
- [ ] **Pipelining:** Verify response ordering — use a test corpus where each RETR response has a different body length; confirm message N body is attributed to message N.
- [ ] **UIDL cache:** Verify ghost-entry pruning — delete a message in session 1, reconnect in session 2; confirm the deleted UID is no longer in the cache.
- [ ] **UIDL cache:** Verify account namespacing — construct two accounts whose servers assign identical UIDs; confirm messages from account B are not skipped due to account A's cache.
- [ ] **UIDL cache:** Verify fallback when server doesn't support UIDL — `client.incremental_sync()` returns a clear error (not a panic) when `UIDL` returns `-ERR`.
- [ ] **Reconnection:** Verify state reset — after simulated reconnect, `client.stat()` succeeds on the first call without manual re-authentication by the caller.
- [ ] **Reconnection:** Verify jitter present — run 10 simulated concurrent reconnection attempts; confirm retry intervals are NOT all identical.
- [ ] **Reconnection:** Verify auth failure does not retry — feed a `401 Authentication Failed` response; confirm the reconnection loop exits immediately rather than retrying.
- [ ] **Connection pool:** Verify per-account max-1 — attempt two concurrent checkouts for the same account; confirm the second blocks until the first is returned, not that it receives a second connection.
- [ ] **MIME integration:** Verify input is dot-unstuffed — parse a RETR response containing a dot-stuffed body line; confirm the MIME parser does not receive `..` prefixed lines.
- [ ] **MIME integration:** Verify multipart bodies — parse a `multipart/alternative` message; confirm both text and HTML parts are accessible.
- [ ] **MIME integration:** Verify encoded headers — parse a message with a `From:` header encoded in RFC 2047 quoted-printable; confirm the decoded display name is correct.

---

## Recovery Strategies

When pitfalls occur despite prevention, how to recover.

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Pipelining deadlock discovered post-publish | HIGH — requires API behaviour change | Add window size parameter to pipelining API; default to 4; publish patch release; add regression test for 200-message mailbox |
| UIDL cache namespace collision found in production | MEDIUM — data integrity issue but recoverable | Add account-key to cache file format in a new version; provide migration utility; clear and rebuild cache on format upgrade |
| Reconnection loop panics on second reconnect (future reuse) | LOW if caught in test; HIGH in production | Add test that simulates two consecutive failures; fix future creation to be inside the loop body; publish patch |
| Pool hands out second connection for same account | HIGH — IN-USE errors cascade to users | Add per-account checkout tracking to pool before shipping; this cannot be patched without breaking API |
| MIME parsing receives dot-stuffed data | MEDIUM — affects all messages with dot-stuffed lines | Fix `retr()` to guarantee dot-unstuffed output; add regression test with dot-stuffed message; patch release |
| Auth failure causes infinite reconnect loop | MEDIUM — resource drain, log spam | Add error classification to reconnection wrapper; `Pop3Error::AuthFailed` must be terminal (no retry); patch release with additional `non_retryable_errors` configuration point |

---

## Pitfall-to-Phase Mapping

How roadmap phases should address these pitfalls.

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Pipelining without CAPA check | Phase 1: Pipelining | Test against mock server not advertising PIPELINING; confirm sequential fallback is triggered |
| TCP deadlock from unbounded pipeline | Phase 1: Pipelining | `cargo test` with 200-message mock mailbox completes within 30s without timeout |
| Response ordering corruption | Phase 1: Pipelining | Test corpus includes varying-size RETR responses; verify body content matches message number |
| UIDL ghost entries not pruned | Phase 2: UIDL Caching | Test deletes a message in session 1, reconnects in session 2; deleted UID absent from cache |
| UIDL cache namespace collision | Phase 2: UIDL Caching | Test with two accounts assigned identical UIDs; messages from account B not skipped |
| Reconnection reuses exhausted future | Phase 3: Reconnection | Test simulates 3 consecutive failures; reconnection loop does not panic on 2nd+ attempts |
| Reconnection skips re-auth | Phase 3: Reconnection | `client.stat()` succeeds on first call after simulated reconnect without manual login() |
| Backoff without jitter | Phase 3: Reconnection | CI logs from parallel reconnect test show non-identical retry delays |
| Auth failure causes retry | Phase 3: Reconnection | Mock server returns auth failure; reconnect loop exits after first attempt, not after max retries |
| Pool allows >1 connection per account | Phase 4: Connection Pooling | Concurrent checkout test for same account blocks (second request) until first is returned |
| MIME receives dot-stuffed input | Phase 5: MIME Integration | Precondition check in v2.0 RETR tests; regression test confirms clean input to parse_mail() |
| Wrong MIME crate chosen | Phase 5: MIME Integration | Evaluate mailparse vs mail-parser against non-ASCII and multipart test corpus before committing |
| write_all cancel-safety in reconnect | Phase 3: Reconnection | Timeout placement review: timeout only wraps read, not write; code review checklist item |

---

## Sources

- [RFC 2449 — POP3 Extension Mechanism](https://datatracker.ietf.org/doc/html/rfc2449) — PIPELINING capability, client tracking requirements, transport window constraints (HIGH confidence)
- [RFC 1939 — Post Office Protocol Version 3](https://www.ietf.org/rfc/rfc1939.txt) — Exclusive maildrop lock (Section 8), UIDL uniqueness definition (Section 7), SHOULD NOT reuse UIDs (HIGH confidence)
- [Microsoft Learn — MS-STANOPOP3 UIDL](https://learn.microsoft.com/en-us/openspecs/exchange_standards/ms-stanopop3/ea3d0e3a-c478-4b1f-8678-82396dffdcac) — UIDL reuse documented; Outlook compliance issues with identical UIDs (MEDIUM confidence)
- [Cancelling async Rust — sunshowers.io](https://sunshowers.io/posts/cancelling-async-rust/) — `write_all` cancel unsafety, timeout on channel send loses messages, cancel-safe patterns (HIGH confidence)
- [How to handle reconnect & shutdown correctly in Tokio — Rust Users Forum](https://users.rust-lang.org/t/how-to-handle-reconnect-shutdown-correctly-in-tokio/105759) — Future completion is one-shot, reconnection loop must create fresh futures (MEDIUM confidence)
- [mailparse docs.rs](https://docs.rs/mailparse/) — `addrparse()` encoded-word gotcha, `dateparse()` limitations, 87% documentation coverage (HIGH confidence from official docs)
- [mail-parser docs.rs](https://docs.rs/mail-parser/latest/mail_parser/) — RFC 5322 + MIME compliance, zero-copy Cow<str> design, 41 charset support (HIGH confidence from official docs)
- [mailparse GitHub README](https://github.com/staktrace/mailparse) — Explicit statement that `\n` is accepted as line delimiter (HIGH confidence from source)
- [backon crates.io — BackON v1 release](https://xuanwo.io/2024/08-backon-reaches-v1/) — Active maintenance confirmed; backoff crate unmaintained confirmed (HIGH confidence)
- [Rust's backoff Crate: Why It's Unmaintained](https://magazine.ediary.site/blog/rusts-backoff-crate-why-its) — backoff crate deprecation confirmed (MEDIUM confidence)
- [backon docs.rs](https://docs.rs/backon/) — ExponentialBuilder with jitter API (HIGH confidence from official docs)
- [Tokio channels tutorial](https://tokio.rs/tokio/tutorial/channels) — oneshot channel pattern for response matching; message-passing for pipelining (HIGH confidence from official docs)
- [Common Mistakes with Rust Async — Qovery](https://www.qovery.com/blog/common-mistakes-with-rust-async) — Mutex across await, blocking in async context (HIGH confidence — multiple source corroboration)
- [POP3 mailbox locking real-world reports](https://www.hmailserver.com/forum/viewtopic.php?t=22361) — IN-USE error behavior confirmed; per-account lock, not per-IP (MEDIUM confidence)
- [Dovecot pop3-migration plugin docs](https://doc.dovecot.org/main/core/plugins/pop3_migration.html) — UIDL caching in persistent indexes; cache invalidation considerations (MEDIUM confidence)

---

*Pitfalls research for: Async Rust POP3 client library — v3.0 advanced features (pipelining, UIDL caching, reconnection, connection pooling, MIME integration)*
*Researched: 2026-03-01*
