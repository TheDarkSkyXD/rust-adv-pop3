# Phase 5: Pipelining - Research

**Researched:** 2026-03-01
**Domain:** Async Rust protocol pipelining / RFC 2449 / tokio I/O split / windowed concurrency
**Confidence:** HIGH (core protocol spec + tokio I/O patterns verified against official sources)

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PIPE-01 | Client can send multiple POP3 commands without waiting for each response when server advertises PIPELINING (RFC 2449) | RFC 2449 §6.6 specifies exact server/client obligations; windowed interleave pattern maps directly to `send_command` loop + `read_response` loop |
| PIPE-02 | Client automatically detects pipelining support via CAPA after authentication | CAPA is already implemented in Phase 3 (CMD-02); just parse the capability list for "PIPELINING" string |
| PIPE-03 | Client falls back to sequential mode when server does not advertise PIPELINING | Trivial conditional branch in batch methods: if `!self.pipelining_supported`, call single-command methods in a loop |
| PIPE-04 | Pipelined commands use a windowed send strategy to prevent TCP send-buffer deadlock | RFC 2449 mandates "must not exceed window size of underlying transport"; interleave send/drain loops with chunk size N (4–8) |
| PIPE-05 | Client provides batch methods (`retr_many`, `dele_many`) that pipeline automatically | New `pub async fn` on `Pop3Client`; PIPE-01..04 are implementation details hidden from caller |
| PIPE-05 (infra) | `src/client.rs` exposes `pub(crate)` reader/writer fields; `is_closed() -> bool`; `Pop3ClientBuilder` derives `Clone` | Infrastructure required by Phases 7 and 8; implement here without behavioural change |
</phase_requirements>

---

## Summary

POP3 pipelining (RFC 2449 §6.6) is a request-batching optimisation: the client sends N commands before reading any responses, turning N round-trips into one. The protocol is strictly ordered — the server processes commands in order and the client matches responses in order — so there is no response-routing complexity. The sole implementation hazard is a TCP send-buffer deadlock: if a client sends an unbounded batch of commands while the server's receive buffer is filling with RETR response data, both ends block waiting on each other. The fix is a **windowed send strategy**: send at most W commands (W = 4–8), drain W responses, repeat.

The Tokio ecosystem does not have a POP3 pipelining reference implementation to copy from. However, the Tokio tutorial's mini-redis and the biriukov.dev Tokio I/O series both document the exact patterns needed: `tokio::io::split` produces independent reader/writer handles, `BufWriter` batches command bytes before a single flush, and the windowed loop is expressible as a simple Rust `for chunk in commands.chunks(W)` loop followed by a matching read loop. No new crate dependencies are needed.

The client-side architecture change is minimal: `Transport` already holds separate `reader` (a `BufReader<ReadHalf>`) and `writer` (`WriteHalf`) since Phase 2 — so "split" is already done. The work is to (1) expose them as `pub(crate)`, (2) add windowed batch send/recv helpers on `Transport`, (3) add an `is_pipelining` flag set during CAPA parsing after auth, and (4) add `retr_many`/`dele_many` on `Pop3Client`.

**Primary recommendation:** Use the chunked interleave loop (not a separate spawned task) to pipeline commands within `retr_many`/`dele_many`. Keep `BufWriter<WriteHalf>` for batched sends. Keep `BufReader<ReadHalf>` for reads. No new crates required.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `tokio` | 1.x (already in Cargo.toml) | Async runtime, `io::split`, `BufReader`, `BufWriter` | Already the project runtime; `io::split` is the canonical way to get independent reader/writer handles in tokio |
| `tokio::io::BufWriter` | (part of tokio 1.x) | Buffer multiple command writes before a single TCP flush | Eliminates per-command syscall overhead; the mini-redis tutorial explicitly recommends this for protocol framing |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tokio_test::io::Builder` | 0.4 (already in dev-dependencies) | Mock sequential write/read expectations for pipelining tests | Already used for all other command tests; pipelining tests follow the same pattern |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Chunked interleave loop (in same task) | Spawned send + recv tasks with channels | Two-task approach needs `Arc<Mutex<Transport>>` or channel routing; adds complexity with no benefit since POP3 response order is guaranteed |
| `BufWriter` wrapping `WriteHalf` | `write_all` directly per command then flush | Direct write per command would flush after every command, defeating the batching purpose |
| Simple `chunks(W)` loop | Tokio `Semaphore` with N permits | Semaphore overkill for POP3's sequential response ordering; chunks are sufficient and clearer |

**Installation:** No new dependencies. All required types are already in `tokio` 1.x.

---

## Architecture Patterns

### Recommended Project Structure

No new files are needed. All changes land in existing source files:

```
src/
├── transport.rs     # Add: pub(crate) reader/writer fields, BufWriter wrapper,
│                    #      send_commands_batch(), is_closed()
├── client.rs        # Add: is_pipelining: bool field, retr_many(), dele_many(),
│                    #      post-auth CAPA probe for PIPELINING
├── error.rs         # Add: ConnectionClosed variant (required by Phase 7)
└── lib.rs           # Re-export ConnectionClosed if it becomes public
```

### Pattern 1: Windowed Interleave Loop

**What:** Send a chunk of W commands (via BufWriter + single flush), then read W responses, repeat until all commands are sent. This is the **only safe** way to pipeline over POP3 without risking TCP deadlock.

**When to use:** Inside `retr_many()` and `dele_many()` when `self.is_pipelining == true`.

**Concrete window size:** Use W = 4. The RFC warns about "window size of the underlying transport layer". In practice, a POP3 RETR response for a 1 MB message is ~1 MB. TCP default receive buffer is 87 KB on Linux. Sending 4–8 RETR commands before draining keeps at most 4–8 × (command bytes) in the server's receive buffer — the commands are tiny (~15 bytes each), so the risk is the server's *send* buffer filling before the client reads. W = 4 is a safe conservative value; it can be tuned upward later.

**Why this avoids deadlock:** If the client sends an unbounded batch of RETR commands and then starts reading, the server's RETR responses pile up in the TCP send buffer. If the server's send buffer fills before the client reads, the server blocks on write. Meanwhile the client is blocked trying to send more commands into its own write buffer. Both block. The windowed loop prevents this: after sending W commands, the client *drains W responses* before sending the next window.

**Example:**
```rust
// Source: Derived from RFC 2449 §6.6 requirement + tokio I/O split pattern
// Inside Transport (or Pop3Client::retr_many)

const PIPELINE_WINDOW: usize = 4;

pub(crate) async fn send_commands_windowed(
    &mut self,
    commands: &[String],
) -> Result<Vec<String>> {
    let mut all_responses = Vec::with_capacity(commands.len());

    for chunk in commands.chunks(PIPELINE_WINDOW) {
        // Phase 1: Send chunk of commands in one BufWriter flush
        for cmd in chunk {
            // Write to BufWriter internal buffer (no syscall yet)
            self.writer.write_all(cmd.as_bytes()).await?;
            self.writer.write_all(b"\r\n").await?;
        }
        // Single flush sends all buffered commands at once
        self.writer.flush().await?;

        // Phase 2: Read exactly chunk.len() responses
        for _ in chunk {
            let response = self.read_line().await?;
            all_responses.push(response);
        }
    }

    Ok(all_responses)
}
```

Note: `RETR` responses are multi-line; the read loop must call `read_multiline()` not `read_line()` for RETR. DELE responses are single-line.

### Pattern 2: Transport Writer Upgrade to BufWriter

**What:** The current `Transport.writer` is a bare `WriteHalf<InnerStream>`. For pipelining, wrap it in `tokio::io::BufWriter` so multiple `write_all` calls accumulate in memory before a syscall.

**When to use:** Phase 5 refactors `transport.rs` to use `BufWriter<WriteHalf<InnerStream>>`. All existing `send_command` calls gain the buffer implicitly; the explicit `flush()` in `send_command` remains correct.

**Example:**
```rust
// Source: tokio mini-redis connection.rs pattern (tokio-rs/mini-redis)
// In Transport struct definition:
pub(crate) struct Transport {
    reader: BufReader<io::ReadHalf<InnerStream>>,
    writer: BufWriter<io::WriteHalf<InnerStream>>,  // was: WriteHalf<InnerStream>
    timeout: Duration,
    encrypted: bool,
    // NEW:
    pub(crate) is_closed: bool,
}
```

**Existing `send_command` stays correct** because it calls `flush()` after every write — the BufWriter just adds a memory buffer; flushing still goes to TCP.

### Pattern 3: CAPA-Based Pipelining Flag

**What:** After successful `login()`, call `capa()` internally, check if the capability list contains `"PIPELINING"`, and store the result in `self.is_pipelining: bool`.

**When to use:** This is the only way PIPE-02 and PIPE-03 are satisfied. The flag avoids CAPA overhead on every batch call.

**Example:**
```rust
// In Pop3Client::login(), after setting state = Authenticated:
let caps = self.capa().await.unwrap_or_default(); // don't fail if CAPA fails
self.is_pipelining = caps.iter().any(|c| c.name == "PIPELINING");
```

### Pattern 4: Sequential Fallback

**What:** If `self.is_pipelining == false`, `retr_many` and `dele_many` call the single-message methods in a plain `for` loop.

**When to use:** PIPE-03 requirement. Must be transparent to caller.

**Example:**
```rust
pub async fn retr_many(&mut self, ids: &[u32]) -> Result<Vec<Message>> {
    if !self.is_pipelining {
        // Sequential fallback
        let mut results = Vec::with_capacity(ids.len());
        for &id in ids {
            results.push(self.retr(id).await?);
        }
        return Ok(results);
    }
    // Pipelined path
    self.retr_many_pipelined(ids).await
}
```

### Pattern 5: `is_closed()` Implementation

**What:** A `pub(crate) fn is_closed(&self) -> bool` on `Transport` that checks the `is_closed` field. The field is set to `true` when any `read_line()` returns `UnexpectedEof` or when `quit()` is called.

**Why:** Required by Phase 8 (bb8 connection pool health check) and Phase 7 (reconnect decorator). Not a live TCP probe — just a flag that tracks known-closed state.

**Example:**
```rust
// In Transport:
pub(crate) fn is_closed(&self) -> bool {
    self.is_closed
}

// In Transport::read_line(), on EOF error:
// self.is_closed = true;
// return Err(Pop3Error::ConnectionClosed);
```

### Anti-Patterns to Avoid

- **Unbounded pipeline:** Sending all N commands before reading any response. For large N with RETR, this fills TCP buffers and deadlocks. Always use a bounded window.
- **Spawning separate read/write tasks:** Requires channel routing and `Arc<Mutex<Transport>>`. POP3's strict response ordering makes this unnecessary complexity. A single-task windowed loop is correct.
- **Mutexing the BufWriter across tasks:** The Turso blog post and Tokio docs both warn that holding a `Mutex` across `.await` deadlocks. Keep Transport single-owner.
- **Probing is_closed() via live TCP reads:** TCP does not reliably detect a dropped connection on a read with buffered data. Use a sentinel flag that is set on known-closed state (EOF or explicit `quit()`).
- **Calling CAPA on every batch operation:** Expensive. Cache the result in `is_pipelining: bool` during `login()`.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| I/O split for independent reader/writer | Custom `Arc<Mutex<Stream>>` split | `tokio::io::split` (already in use) | Already done in Phase 2; `ReadHalf`/`WriteHalf` are the canonical tokio types; `io::split` uses `Arc<Mutex>` internally but in a safe, tested way |
| Write buffering before batch flush | Manual `Vec<u8>` accumulation | `tokio::io::BufWriter` | Standard tokio type; default 8 KB buffer; avoids per-command syscall; handles partial writes |
| Response ordering | Request ID map or correlation channel | None — POP3 guarantees in-order processing | RFC 2449: "server MUST process each command in turn." No out-of-order responses possible. |
| Pipelining detection | Custom CAPA parser for PIPELINING | `capa()` method already in Phase 3 + `c.name == "PIPELINING"` check | `capa()` returns `Vec<Capability>` with `.name` field; direct string compare is sufficient |
| Window size / backpressure | Tokio `Semaphore`, bounded channels, complex state machines | Simple `commands.chunks(W)` loop | For a single-task single-connection sequential-response protocol, chunked loop is correct, auditable, and deadlock-proof |

**Key insight:** POP3's strict sequential response ordering eliminates all response-routing complexity. The only hard problem is TCP deadlock, which the windowed chunk loop solves without any concurrency primitives.

---

## Common Pitfalls

### Pitfall 1: TCP Send-Buffer Deadlock with Unbounded Pipeline

**What goes wrong:** Client sends 100 RETR commands before reading. Server starts sending 100 RETR responses. Server's TCP send buffer fills. Server's `write()` blocks. Meanwhile, the server is not reading client commands from its receive buffer (it's blocked writing). Client's `write()` eventually blocks too (client's send buffer filled). Both sides block. Deadlock.

**Why it happens:** POP3 RETR responses can be megabytes. TCP buffers are tens to hundreds of KB. Sending N responses before the client reads exhausts kernel buffer space.

**How to avoid:** Always use `commands.chunks(W)` with W ≤ 8. After sending a chunk, *read all responses for that chunk* before proceeding.

**Warning signs:** Test hangs indefinitely with no error. Only manifests with large messages and large batches.

### Pitfall 2: BufWriter Not Flushed After Batch Send

**What goes wrong:** Commands are written to BufWriter but `flush()` is never called. The server receives nothing. Client waits for responses that never arrive. Timeout eventually fires.

**Why it happens:** `BufWriter::write_all()` writes to an in-memory buffer; it does not touch the TCP socket. Flush is mandatory.

**How to avoid:** Always call `self.writer.flush().await?` after the send loop for each chunk, before the read loop.

**Warning signs:** Test times out after the configured timeout duration with no server-side activity.

### Pitfall 3: Wrapping TLS Streams with io::split (Platform-Specific)

**What goes wrong:** Some TLS implementations (OpenSSL especially) require reads and writes to be driven from the same task. Splitting into separate tasks causes panics or incorrect behavior.

**Why it happens:** TLS handshake state is not always thread-safe across independent halves. The biriukov.dev Tokio I/O Patterns article explicitly notes "TLS transports do not keep reads and writes independent."

**How to avoid:** Phase 5 must NOT spawn separate reader/writer tasks. The windowed loop runs in a single task, alternating between flush and drain, using the already-split `reader`/`writer` fields within the same `&mut self` context.

**Warning signs:** Intermittent panics or connection errors under TLS that do not reproduce under plain TCP.

### Pitfall 4: CAPA Call Failure Breaking Authentication

**What goes wrong:** `capa()` is called internally after `login()`. On servers that do not implement CAPA (rare but valid per RFC 1939), `capa()` returns `-ERR`. If the code propagates this error, `login()` appears to fail.

**Why it happens:** Not all POP3 servers support CAPA. The CAPA command itself is optional per RFC 1939.

**How to avoid:** Use `unwrap_or_default()` or match on the error and treat any CAPA failure as "no capabilities". Never propagate CAPA failure to the caller.

**Warning signs:** Login fails against older POP3 servers (e.g., some embedded mail servers) that do not implement CAPA.

### Pitfall 5: tokio_test::io::Builder Ordering with Pipelining

**What goes wrong:** The mock for a pipelining test specifies writes and reads in the wrong order, causing the mock to panic.

**Why it happens:** `tokio_test::io::Builder` enforces strict sequential ordering. If the test specifies `write(cmd1)`, `write(cmd2)`, `read(resp1)`, `read(resp2)` but the code writes cmd1 then reads resp1 (sequential mode), the mock panics because it expected write(cmd2) next.

**How to avoid:** Use separate mock builders for sequential mode tests and pipelining mode tests. In pipelining tests, pre-populate the mock with all writes first (the full batch), then all reads.

**Warning signs:** Test panics with "expected write, got read" or similar tokio_test assertion messages.

### Pitfall 6: Upgrading WriteHalf to BufWriter Breaks STARTTLS

**What goes wrong:** `upgrade_in_place()` in `transport.rs` reassembles the stream from its halves using `unsplit()`. If `writer` is now a `BufWriter<WriteHalf>`, `unsplit()` requires the raw `WriteHalf`, not the `BufWriter`.

**Why it happens:** `BufWriter::into_inner()` is needed to recover the `WriteHalf` before calling `unsplit()`.

**How to avoid:** In `upgrade_in_place()`, call `self.writer.into_inner()` to unwrap the `BufWriter` before calling `read_half.unsplit(write_half)`. Then re-wrap the new `WriteHalf` in a new `BufWriter` after the TLS handshake.

**Warning signs:** Compile error ("type mismatch: expected WriteHalf, found BufWriter<WriteHalf>") in `upgrade_in_place`.

---

## Code Examples

Verified patterns from official sources:

### Windowed Pipeline Loop

```rust
// Pattern: send W commands, flush, read W responses, repeat
// Source: Derived from RFC 2449 §6.6 + mini-redis BufWriter flush pattern
// (https://github.com/tokio-rs/mini-redis/blob/master/src/connection.rs)

const PIPELINE_WINDOW: usize = 4;

// Inside retr_many_pipelined (on Pop3Client or Transport):
async fn retr_many_pipelined(&mut self, ids: &[u32]) -> Result<Vec<Message>> {
    self.require_auth()?;
    let mut messages = Vec::with_capacity(ids.len());

    for chunk in ids.chunks(PIPELINE_WINDOW) {
        // Send phase: all commands into BufWriter, one flush
        for &id in chunk {
            validate_message_id(id)?;
            let cmd = format!("RETR {id}");
            self.transport.writer.write_all(cmd.as_bytes()).await?;
            self.transport.writer.write_all(b"\r\n").await?;
        }
        self.transport.writer.flush().await?;

        // Receive phase: drain exactly chunk.len() responses
        for _ in chunk {
            let status = self.transport.read_line().await?;
            response::parse_status_line(&status)?;
            let body = self.transport.read_multiline().await?;
            messages.push(Message { body });
        }
    }

    Ok(messages)
}
```

### Transport BufWriter Upgrade

```rust
// Source: tokio::io::BufWriter docs (docs.rs/tokio/latest/tokio/io/struct.BufWriter.html)
// In Transport struct:
pub(crate) struct Transport {
    reader: BufReader<io::ReadHalf<InnerStream>>,
    writer: BufWriter<io::WriteHalf<InnerStream>>,  // upgrade from bare WriteHalf
    timeout: Duration,
    encrypted: bool,
    is_closed: bool,  // NEW: tracks known-closed state for Phase 7/8
}

// Construction (in connect_plain, connect_tls):
let (read_half, write_half) = io::split(inner);
Ok(Transport {
    reader: BufReader::new(read_half),
    writer: BufWriter::new(write_half),  // wrap in BufWriter
    timeout,
    encrypted: false,
    is_closed: false,
})
```

### upgrade_in_place BufWriter Unwrap

```rust
// Source: BufWriter::into_inner() pattern, needed for STARTTLS compatibility
// In Transport::upgrade_in_place():

let old_reader = std::mem::replace(&mut self.reader, BufReader::new(placeholder_read));
let old_writer = std::mem::replace(&mut self.writer, BufWriter::new(placeholder_write));

let read_half = old_reader.into_inner();
let write_half = old_writer.into_inner();  // unwrap BufWriter to get WriteHalf
let inner_stream = read_half.unsplit(write_half);

// ... TLS handshake ...

let (new_read, new_write) = io::split(tls_inner);
self.reader = BufReader::new(new_read);
self.writer = BufWriter::new(new_write);  // re-wrap after upgrade
```

### CAPA-Based Pipelining Flag

```rust
// Source: Phase 3 capa() implementation (CMD-02, already complete)
// In Pop3Client::login(), after successful auth:

// Don't propagate CAPA error — not all servers support it
let caps = self.capa().await.unwrap_or_default();
self.is_pipelining = caps.iter().any(|c| c.name == "PIPELINING");
```

### is_closed Detection

```rust
// Source: Established tokio EOF detection pattern
// In Transport::read_line():
if n == 0 {
    self.is_closed = true;
    return Err(Pop3Error::ConnectionClosed);  // new variant, replaces old Io(UnexpectedEof)
}

// Accessor:
pub(crate) fn is_closed(&self) -> bool {
    self.is_closed
}
```

### Mock Test for Pipelining

```rust
// Source: tokio_test::io::Builder pattern (already used in this codebase)
// Test: pipelining sends all writes before reads

#[tokio::test]
async fn test_dele_many_pipelined() {
    // Mock: expect writes first, then reads (pipelining order)
    let mock = Builder::new()
        .write(b"DELE 1\r\n")
        .write(b"DELE 2\r\n")
        .write(b"DELE 3\r\n")
        .read(b"+OK message 1 deleted\r\n")
        .read(b"+OK message 2 deleted\r\n")
        .read(b"+OK message 3 deleted\r\n")
        .build();

    let mut client = build_authenticated_test_client_with_pipelining(mock);
    client.is_pipelining = true;  // force pipeline mode
    let results = client.dele_many(&[1, 2, 3]).await.unwrap();
    assert_eq!(results.len(), 3);
}
```

Note: With BufWriter, the three `write_all` calls accumulate in the buffer, and a single `flush()` sends all three commands as one TCP write. The `tokio_test::io::Builder` mock sees three sequential write calls followed by reads — this matches the builder's expectation ordering. (The mock validates call order, not TCP segment boundaries.)

### Deadlock Test (no-deadlock verification)

```rust
// Test: PIPE-04 — windowed strategy does not deadlock even for large batches
// Strategy: use a large ID list (e.g., 100) with small RETR responses
// and confirm it completes without hanging.

#[tokio::test]
async fn test_retr_many_large_batch_no_deadlock() {
    let mut builder = Builder::new();
    let n: u32 = 100;
    // Add writes in PIPELINE_WINDOW chunks interleaved with reads
    for chunk_start in (1..=n).step_by(PIPELINE_WINDOW) {
        let chunk_end = (chunk_start + PIPELINE_WINDOW as u32 - 1).min(n);
        for id in chunk_start..=chunk_end {
            builder = builder.write(format!("RETR {id}\r\n").as_bytes().to_vec().as_slice().into());
        }
        for _ in chunk_start..=chunk_end {
            builder = builder
                .read(b"+OK\r\n")
                .read(b"body\r\n")
                .read(b".\r\n");
        }
    }
    let mock = builder.build();
    let mut client = build_authenticated_test_client_with_pipelining(mock);
    client.is_pipelining = true;
    let results = client.retr_many(&(1..=n).collect::<Vec<_>>()).await.unwrap();
    assert_eq!(results.len(), n as usize);
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `tokio-proto` crate for pipelining | Manual windowed loop with `tokio::io::split` | ~2019 (tokio-proto deprecated) | `tokio-proto` was removed from the ecosystem; the pattern it provided is now implemented manually using io::split + BufWriter |
| `actions-rs/*` GitHub Actions | `dtolnay/rust-toolchain` | ~2022 | Already adopted in Phase 2; no impact for Phase 5 |
| Separate spawned reader/writer tasks for pipelining | Single-task windowed interleave loop | Ongoing guidance in Tokio docs | TLS stream coupling makes two-task split unsafe; single-task loop avoids Arc/Mutex entirely |

**Deprecated/outdated:**
- `tokio-proto`: Removed from active development. The pipeline transport pattern it provided is now implemented directly with `io::split` + `BufWriter`.
- Sending all commands before reading any: Universally recognized as causing TCP deadlock with large responses. Windowed approach is the documented standard (RFC 2449, D.J. Bernstein's FTP pipelining notes).

---

## Open Questions

1. **Exact window size W**
   - What we know: RFC 2449 says "must not exceed transport window size". W = 4–8 is widely cited in email client implementations as safe. The Linux TCP receive buffer default is 87 KB.
   - What's unclear: Whether W should be configurable by the caller or hardcoded. For v3.0, hardcoding W = 4 as a `const` in transport.rs is simpler and sufficient.
   - Recommendation: Hardcode `PIPELINE_WINDOW: usize = 4` as a `pub(crate) const` in `transport.rs`. Make it easily discoverable for future tuning. Do not expose it in the public API yet.

2. **BufWriter size for RETR responses**
   - What we know: BufWriter default is 8 KB write buffer. This is for the outbound command side only — commands are ~15 bytes each, so the buffer holds hundreds of commands before needing a flush.
   - What's unclear: Whether BufWriter's default is appropriate or if a custom capacity is needed.
   - Recommendation: Use `BufWriter::new(write_half)` (default 8 KB). Commands are small; this is not a bottleneck. The BufWriter on the write side has no interaction with the large RETR response data (which flows through the read side).

3. **`Pop3ClientBuilder` derive Clone scope**
   - What we know: The Roadmap says Phase 5 must add `Clone` to `Pop3ClientBuilder` (PIPE-05 infrastructure item). `Pop3Client` cannot derive Clone (it owns a TCP connection). Only the *builder* (pre-connection configuration) gets Clone.
   - What's unclear: Whether Phase 4 has implemented the builder yet (Phase 4 is not started).
   - Recommendation: Phase 5 should implement `Pop3ClientBuilder` if Phase 4 has not, or add `#[derive(Clone)]` to the existing builder. The builder stores only serialisable config (address string, timeout duration, TLS flag) — all `Clone`-able.

4. **CAPA call failure path with no-TLS servers**
   - What we know: Some servers don't support CAPA (RFC 1939 does not require it). `capa()` will return a `Pop3Error::ServerError` or similar.
   - What's unclear: Whether the current `capa()` implementation handles `-ERR` responses gracefully or panics.
   - Recommendation: Wrap the `capa()` call in `login()` with `.unwrap_or_default()`. Verify `capa()` returns `Ok(vec![])` (empty) on `-ERR` responses rather than propagating an error. If `capa()` does propagate, add a wrapper.

---

## Validation Architecture

The `config.json` does not have `workflow.nyquist_validation: true` — it only has `workflow.research: true`. The existing test infrastructure (inline `#[cfg(test)]` modules with `tokio_test::io::Builder` mocks) is the established pattern. No separate test framework section is needed.

### Test Map

| Req ID | Behavior | Test Type | Automated Command |
|--------|----------|-----------|-------------------|
| PIPE-01 | `retr_many` sends all commands before reading all responses (pipelined) | Unit (mock) | `cargo test retr_many_pipelined` |
| PIPE-02 | After login, PIPELINING in CAPA sets `is_pipelining = true` | Unit (mock) | `cargo test pipelining_detected_via_capa` |
| PIPE-03 | Without PIPELINING in CAPA, `retr_many` falls back to sequential | Unit (mock) | `cargo test retr_many_sequential_fallback` |
| PIPE-04 | Large batch (100 IDs) completes without deadlock | Unit (mock) | `cargo test retr_many_large_batch_no_deadlock` |
| PIPE-05 | `dele_many` pipelines DELE commands | Unit (mock) | `cargo test dele_many_pipelined` |
| Infra | `is_closed()` returns true after EOF | Unit (mock) | `cargo test is_closed_after_eof` |
| Infra | `Pop3ClientBuilder` can be cloned | Unit (compile check + runtime) | `cargo test builder_is_clone` |

All tests run with `cargo test` (no additional tooling needed).

---

## Sources

### Primary (HIGH confidence)

- [RFC 2449 §6.6 — PIPELINING capability](https://www.rfc-editor.org/rfc/rfc2449.html) — authoritative specification; directly quotes "must not exceed window size of underlying transport"
- [tokio::io::BufWriter docs](https://docs.rs/tokio/latest/tokio/io/struct.BufWriter.html) — default buffer size, flush semantics, into_inner() for STARTTLS compat
- [tokio mini-redis connection.rs](https://github.com/tokio-rs/mini-redis/blob/master/src/connection.rs) — canonical Tokio protocol framing with BufWriter + flush pattern
- [tokio::io::split docs](https://docs.rs/tokio/latest/tokio/io/fn.split.html) — ReadHalf/WriteHalf independence guarantees
- [Tokio tutorial: Channels](https://tokio.rs/tokio/tutorial/channels) — documents that pipelining with mutex underutilizes connection; message-passing alternative
- [Tokio tutorial: Framing](https://tokio.rs/tokio/tutorial/framing) — BufWriter batch flush pattern

### Secondary (MEDIUM confidence)

- [Viacheslav Biriukov: Async Rust Tokio I/O Backpressure and Concurrency](https://biriukov.dev/docs/async-rust-tokio-io/1-async-rust-with-tokio-io-streams-backpressure-concurrency-and-ergonomics/) — verified with official Tokio docs; select! starvation and backpressure analysis
- [Viacheslav Biriukov: Tokio I/O Patterns](https://biriukov.dev/docs/async-rust-tokio-io/3-tokio-io-patterns/) — connection-driver pattern; TLS split hazard documentation
- [D.J. Bernstein: FTP Pipelining](https://cr.yp.to/ftp/pipelining.html) — foundational analysis of protocol pipelining deadlock avoidance; server must have TCP_NODELAY; client manages response ordering

### Tertiary (LOW confidence)

- [Rust forum: Buffering writes for pipelined request-response pattern](https://users.rust-lang.org/t/buffering-writes-for-a-pipelined-request-response-pattern/43279) — single-source forum post, useful for confirming copy-based streaming approach alternatives
- SMTP pipelining window size guidance — general ecosystem convention of 4–8 commands per window; no single authoritative source found for POP3 specifically

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all needed types are already in tokio 1.x; no new crates required; verified against official docs
- Architecture: HIGH — windowed interleave loop is directly mandated by RFC 2449; BufWriter pattern is verbatim from mini-redis tutorial
- Pitfalls: HIGH — TCP deadlock mechanism verified via RFC 2449 text and TCP flow control documentation; BufWriter flush omission is a documented beginner mistake; TLS split hazard documented by Biriukov
- Window size W=4: MEDIUM — conservative value derived from TCP buffer knowledge; exact number is not specified by any authoritative source for POP3 specifically

**Research date:** 2026-03-01
**Valid until:** 2026-06-01 (tokio 1.x API is stable; RFC 2449 is not changing)
