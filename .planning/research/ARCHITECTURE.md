# Architecture Research

**Domain:** Async Rust POP3 client library — v3.0 advanced feature integration
**Researched:** 2026-03-01
**Confidence:** HIGH (protocol specs from RFCs; tokio patterns from official docs; bb8/backoff from verified source)

---

## Context: What v3.0 Builds On

v2.0 (phases 1-4) establishes the following baseline that v3.0 extends:

- `src/client.rs` — `Client` struct owning `BufReader<ReadHalf<AsyncStream>>` + `WriteHalf<AsyncStream>`, with `SessionState` enum
- `src/tls/mod.rs` — Feature-gated `AsyncStream` enum (`Plain`, `Rustls`, `OpenSsl`) implementing `AsyncRead + AsyncWrite`
- `src/response.rs` — Pure-function parser for POP3 response types
- `src/command.rs` — `Command` enum with wire-format serialization
- `src/error.rs` — `Pop3Error` typed enum via `thiserror`
- `Pop3ClientBuilder` — Fluent builder hiding TLS feature flag complexity

**v3.0 does not rewrite v2.0.** It adds five features on top of the existing async foundation:

| Feature | v2.0 Touchpoint | v3.0 Change Type |
|---------|----------------|-----------------|
| POP3 command pipelining (RFC 2449) | `Client` send/read loop | New: `PipelinedClient` wrapper or pipeline mode on `Client` |
| UIDL caching for incremental sync | None (UIDL exists, caching does not) | New: `UidlCache` struct + persistence layer |
| Automatic reconnection with exponential backoff | None | New: `ReconnectingClient` wrapper struct |
| Connection pooling | None | New: `Pop3Pool` via `bb8::ManageConnection` impl |
| Optional mailparse/MIME integration | `retr()` returns `String` | New: `mime` feature flag + typed `ParsedMessage` return |

---

## Standard Architecture

### System Overview (v3.0 Layer Added Above v2.0)

```
┌────────────────────────────────────────────────────────────────────────┐
│                        v3.0 ADVANCED API LAYER                          │
│                                                                         │
│  ReconnectingClient::new(builder, backoff_config) -> ReconnectingClient │
│      .stat() / .retr() / .uidl() ...   (transparent auto-reconnect)    │
│                                                                         │
│  Pop3Pool::builder(manager).build().await -> Pop3Pool                   │
│      pool.get().await -> PooledConnection (wraps Client)                │
│                                                                         │
│  PipelinedSession: send_command_batch() -> receive_responses()          │
│                                                                         │
│  UidlCache::load(path) / .save(path) / .filter_new(uidl_list)          │
│                                                                         │
│  #[cfg(feature = "mime")]                                               │
│  client.retr_parsed(msg_num) -> Result<ParsedMessage>                   │
├────────────────────────────────────────────────────────────────────────┤
│                        v2.0 PUBLIC API LAYER (unchanged)                │
│                                                                         │
│   Client::builder().connect(...).await -> Result<Client>                │
│   client.stat() / list() / retr() / uidl() / dele() / quit() ...       │
│   Types: StatResponse, MessageMeta, UidEntry, Pop3Error                 │
├────────────────────────────────────────────────────────────────────────┤
│                        v2.0 CONNECTION LAYER (unchanged)                │
│                                                                         │
│   src/client.rs — Client struct                                         │
│   - Owns: AsyncStream (feature-gated enum)                              │
│   - Owns: BufReader<ReadHalf> + WriteHalf                               │
│   - Tracks: SessionState (Authorization / Transaction / Update)         │
├────────────────────────────────────────────────────────────────────────┤
│                        v2.0 TRANSPORT LAYER (unchanged)                 │
│                                                                         │
│   src/tls/mod.rs — AsyncStream enum                                     │
│   Plain(TcpStream) | Rustls(TlsStream) | OpenSsl(SslStream)            │
│   All variants impl AsyncRead + AsyncWrite                              │
├────────────────────────────────────────────────────────────────────────┤
│                        v2.0 PROTOCOL LAYER (unchanged)                  │
│                                                                         │
│   src/response.rs  src/command.rs  src/error.rs                        │
└────────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities (v3.0 New Components)

| Component | File | Responsibility | Communicates With |
|-----------|------|---------------|-------------------|
| `PipelinedSession` | `src/pipeline.rs` | Sends multiple commands without awaiting each; reads responses in-order using a VecDeque command queue | `Client` internals (write half, read half); `command.rs`; `response.rs` |
| `UidlCache` | `src/cache.rs` | Persists seen UIDL strings between sessions; computes delta (new messages only) | `Client::uidl()` output; caller-provided persistence path |
| `ReconnectingClient` | `src/reconnect.rs` | Wraps `Client` + `Pop3ClientBuilder`; detects `Pop3Error::Io` and transparently reconnects using exponential backoff | `client.rs`; `backoff` crate or `tokio-retry` |
| `Pop3Pool` | `src/pool.rs` | `bb8::ManageConnection` impl; provides a bounded async connection pool for callers that manage N independent POP3 accounts (not the same mailbox) | `client.rs`; `bb8` crate |
| `ParsedMessage` | `src/mime.rs` | Wraps raw RETR output through `mailparse::parse_mail()`; exposes structured headers and body parts | `client.rs`; `mailparse` crate (optional) |
| `Pop3Manager` | `src/pool.rs` | Concrete `bb8::ManageConnection` struct; implements `connect()` and `is_valid()` using `Pop3ClientBuilder` | `Pop3ClientBuilder`; `bb8` |

---

## Recommended Project Structure (v3.0 Additions)

```
src/
├── lib.rs                  # Add pub use for new v3.0 types; feature-gate mime exports
├── client.rs               # UNCHANGED — v2.0 Client struct
├── command.rs              # UNCHANGED — Command enum
├── response.rs             # UNCHANGED — response parsers
├── error.rs                # EXTEND — add Pop3Error::Reconnect, Pop3Error::PoolError variants
├── tls/                    # UNCHANGED — AsyncStream enum + TLS backends
│
├── pipeline.rs             # NEW — PipelinedSession struct
├── cache.rs                # NEW — UidlCache struct + serde-based persistence
├── reconnect.rs            # NEW — ReconnectingClient struct + backoff logic
├── pool.rs                 # NEW — Pop3Manager (ManageConnection impl) + Pop3Pool type alias
└── mime.rs                 # NEW — ParsedMessage struct; #[cfg(feature = "mime")] only

tests/
├── mock_server.rs          # EXTEND — add pipelined response sequences to mock
├── integration.rs          # EXTEND — add reconnect/pool/cache integration tests
└── pipeline_tests.rs       # NEW — unit tests for command batching and response matching

examples/
├── connect.rs              # UNCHANGED — v2.0 basic example
├── incremental_sync.rs     # NEW — UIDL cache + reconnect example
└── pool_usage.rs           # NEW — connection pool example
```

### Structure Rationale

- **`src/pipeline.rs` separate:** Pipelining changes the send/receive loop in a fundamental way. Isolating it prevents coupling it to the standard `Client` implementation, which must remain simple.
- **`src/cache.rs` separate:** UIDL persistence is I/O and serialization logic — not protocol logic. Keeping it in its own module allows swapping the persistence backend (file, SQLite, memory) without touching `client.rs`.
- **`src/reconnect.rs` wraps `Client`:** Reconnection is a policy, not a protocol concept. The `ReconnectingClient` wraps `Client` by value (not by ref) and rebuilds it using `Pop3ClientBuilder` on failure. This is the Decorator pattern: same method signatures, added behavior.
- **`src/pool.rs` uses `bb8`:** `bb8` is the standard async connection pool for tokio (analogous to `r2d2` for sync Rust). Implementing `bb8::ManageConnection` is 30 lines of code. The alternative (building a pool from scratch) is complex and error-prone.
- **`src/mime.rs` behind feature flag:** MIME parsing adds `mailparse` as a compile dependency. The feature flag keeps the core library zero-extra-dependency. Callers who do not need MIME parsing pay no cost.

---

## Architectural Patterns

### Pattern 1: Pipeline Queue via VecDeque (POP3 Pipelining)

**What:** RFC 2449 PIPELINING allows the client to send multiple commands without waiting for each response. The responses arrive in the same order the commands were sent (POP3 protocol guarantee). The client maintains a `VecDeque<CommandKind>` of outstanding commands. It sends all commands to the write half, then reads responses from the read half and pops command kinds from the queue to drive response parsing.

**When to use:** Only when `CAPA` confirms the server advertises `PIPELINING`. The capability is negotiated per-session before pipelining begins.

**Trade-offs:** Increases throughput for bulk operations (e.g., `DELE` for N messages after UIDL sync). Adds state — the `VecDeque` must be kept consistent; a command write failure requires flushing the entire queue before recovering. Must respect the TCP window size constraint from RFC 2449.

**Protocol constraint:** RFC 2449 explicitly states clients MUST track outstanding commands and match responses in order. Responses arrive sequentially — there is no multiplexed response matching as in HTTP/2.

**Example:**
```rust
// src/pipeline.rs

use std::collections::VecDeque;
use crate::{command::Command, response, error::Pop3Error};

pub struct PipelinedSession<'a> {
    client: &'a mut crate::Client,
    queue: VecDeque<Command>,
}

impl<'a> PipelinedSession<'a> {
    /// Send a batch of commands without awaiting responses.
    pub async fn send_batch(&mut self, commands: Vec<Command>) -> Result<(), Pop3Error> {
        use tokio::io::AsyncWriteExt;
        for cmd in commands {
            let wire = cmd.to_wire_format();
            self.client.writer.write_all(wire.as_bytes()).await
                .map_err(Pop3Error::Io)?;
            self.queue.push_back(cmd);
        }
        self.client.writer.flush().await.map_err(Pop3Error::Io)?;
        Ok(())
    }

    /// Read all pending responses in command-dispatch order.
    pub async fn receive_all(&mut self) -> Result<Vec<Result<String, Pop3Error>>, Pop3Error> {
        let mut results = Vec::with_capacity(self.queue.len());
        while let Some(cmd) = self.queue.pop_front() {
            let line = self.client.read_single_line().await?;
            results.push(response::parse_response_for_command(&cmd, &line));
        }
        Ok(results)
    }
}
```

### Pattern 2: Decorator Pattern (Automatic Reconnection)

**What:** `ReconnectingClient` owns a `Client` and a `Pop3ClientBuilder`. When any method returns `Pop3Error::Io` (network drop) or `Pop3Error::ConnectionReset`, the decorator reconnects using the builder and re-authenticates before retrying. The exponential backoff is implemented using the `backoff` crate (tokio feature), which provides `ExponentialBackoff` with jitter.

**When to use:** Long-running email sync applications where transient network drops should not require manual reconnect logic in application code.

**Trade-offs:** Transparent reconnect hides session state loss. If a `DELE` command was issued and the connection dropped before the server processed it, reconnection starts a fresh session — the deletion is not committed. This must be documented explicitly. The decorator cannot and should not handle this for callers.

**Critical invariant:** Reconnection resets to `SessionState::Authorization`. The caller's `ReconnectingClient` re-authenticates automatically (credentials held in the builder), but the caller is responsible for not assuming session state is preserved across a reconnect.

**Example:**
```rust
// src/reconnect.rs

use backoff::{ExponentialBackoff, future::retry};
use crate::{Pop3ClientBuilder, Client, Pop3Error};

pub struct ReconnectingClient {
    builder: Pop3ClientBuilder,
    inner: Option<Client>,
    backoff_config: ExponentialBackoff,
}

impl ReconnectingClient {
    pub fn new(builder: Pop3ClientBuilder, backoff: ExponentialBackoff) -> Self {
        Self { builder, inner: None, backoff_config: backoff }
    }

    async fn ensure_connected(&mut self) -> Result<&mut Client, Pop3Error> {
        if self.inner.is_none() {
            let builder = self.builder.clone();
            let client = retry(self.backoff_config.clone(), || async {
                builder.connect().await
                    .map_err(|e| backoff::Error::transient(e))
            }).await?;
            self.inner = Some(client);
        }
        Ok(self.inner.as_mut().unwrap())
    }

    pub async fn stat(&mut self) -> Result<crate::StatResponse, Pop3Error> {
        loop {
            match self.ensure_connected().await?.stat().await {
                Ok(r) => return Ok(r),
                Err(Pop3Error::Io(_)) => { self.inner = None; } // reconnect on next iteration
                Err(e) => return Err(e),
            }
        }
    }
    // ... mirror for each Client method
}
```

### Pattern 3: bb8 ManageConnection (Connection Pooling)

**What:** `bb8` is the standard async connection pool for tokio. The `ManageConnection` trait requires two methods: `connect()` (create a new connection) and `is_valid()` (health check an existing connection). Implementing this for `Client` gives callers a bounded pool of POP3 connections.

**Critical POP3 constraint:** POP3 servers lock the maildrop exclusively per session (RFC 1939 + RFC 2449 `[IN-USE]` response code). A connection pool is only valid if each pooled connection targets a **different mailbox**. Pooling connections to the same mailbox will cause every connection after the first to receive `-ERR [IN-USE]`. This constraint must be documented prominently. The pool is for multi-account mail processing, not parallelizing access to a single inbox.

**When to use:** Applications that process mail from N different accounts concurrently (e.g., an email gateway handling 50 customer inboxes).

**Trade-offs:** Pool overhead (health check on borrow, connection creation on pool miss) is negligible for POP3 where each operation takes tens of milliseconds. The pool size limit prevents runaway connection counts on the server.

**Example:**
```rust
// src/pool.rs

use bb8::{ManageConnection, Pool, PooledConnection};
use crate::{Pop3ClientBuilder, Client, Pop3Error};

pub struct Pop3Manager {
    builder: Pop3ClientBuilder,
}

#[async_trait::async_trait]
impl ManageConnection for Pop3Manager {
    type Connection = Client;
    type Error = Pop3Error;

    async fn connect(&self) -> Result<Client, Pop3Error> {
        self.builder.clone().connect().await
    }

    async fn is_valid(&self, conn: &mut Client) -> Result<(), Pop3Error> {
        // NOOP is the standard POP3 health check
        conn.noop().await
    }

    fn has_broken(&self, conn: &mut Client) -> bool {
        conn.is_closed()
    }
}

pub type Pop3Pool = Pool<Pop3Manager>;

pub async fn build_pool(
    builder: Pop3ClientBuilder,
    max_size: u32,
) -> Result<Pop3Pool, bb8::RunError<Pop3Error>> {
    let manager = Pop3Manager { builder };
    Pool::builder().max_size(max_size).build(manager).await
}
```

### Pattern 4: UIDL Cache with Hash-Set Delta Computation

**What:** After `client.uidl()` returns the full server UIDL list, the `UidlCache` computes the set difference between the server's current UIDs and previously seen UIDs. Only new UIDs are returned to the caller for retrieval. The seen-UID set is persisted between process runs.

**When to use:** Incremental email sync where downloading already-seen messages is undesirable (the common case for any long-running mail client).

**Persistence strategy:** The simplest correct approach is a `HashSet<String>` serialized via `serde_json` (or `serde_cbor` for binary). The file is read at startup and written after each sync. The `mime` feature flag does not affect caching.

**Trade-offs:** The cache can grow unboundedly as messages accumulate unless the caller also calls `cache.expire_deleted(current_uidl_list)` to remove UIDs for messages that no longer exist on the server. This cleanup step must be exposed explicitly — the library should not silently expire UIDs.

**Example:**
```rust
// src/cache.rs

use std::collections::HashSet;
use std::path::Path;
use serde::{Serialize, Deserialize};

#[derive(Default, Serialize, Deserialize)]
pub struct UidlCache {
    seen: HashSet<String>,
}

impl UidlCache {
    pub fn load(path: &Path) -> Result<Self, std::io::Error> {
        if path.exists() {
            let data = std::fs::read(path)?;
            Ok(serde_json::from_slice(&data)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let data = serde_json::to_vec(self)?;
        std::fs::write(path, data)
    }

    /// Returns UIDs not previously seen (new messages to download).
    pub fn filter_new<'a>(&self, uidls: &'a [crate::UidEntry]) -> Vec<&'a crate::UidEntry> {
        uidls.iter().filter(|e| !self.seen.contains(&e.uid)).collect()
    }

    /// Mark UIDs as seen after successful retrieval.
    pub fn mark_seen(&mut self, uid: String) {
        self.seen.insert(uid);
    }

    /// Remove UIDs that are no longer on the server (message deleted server-side).
    pub fn expire_deleted(&mut self, current_uidls: &[crate::UidEntry]) {
        let current: HashSet<&str> = current_uidls.iter().map(|e| e.uid.as_str()).collect();
        self.seen.retain(|uid| current.contains(uid.as_str()));
    }
}
```

### Pattern 5: Optional Feature Flag for MIME Parsing

**What:** The `mime` Cargo feature activates `mailparse` as an optional dependency and exposes additional methods on `Client` (or a wrapper) that return `ParsedMessage` instead of raw `String`. The core library remains zero-dependency for callers who only want transport.

**When to use:** When callers need structured access to email headers, body parts, and attachments after retrieval. Without the feature, `retr()` returns `String`; with the feature, `retr_parsed()` returns `Result<ParsedMessage>`.

**Trade-offs:** `mailparse` is a pure-Rust crate with no C dependencies. Enabling it is safe for cross-compilation. The parsing overhead (converting raw bytes to structured representation) is proportional to message size and happens in application memory — no additional I/O.

**Example:**
```rust
// src/mime.rs  — only compiled when feature = "mime"

use mailparse::{parse_mail, ParsedMail as MailparseParsedMail, MailParseError};
use crate::Pop3Error;

pub struct ParsedMessage<'a> {
    inner: MailparseParsedMail<'a>,
}

impl<'a> ParsedMessage<'a> {
    pub fn parse(raw: &'a [u8]) -> Result<Self, Pop3Error> {
        parse_mail(raw)
            .map(|inner| Self { inner })
            .map_err(|e| Pop3Error::ParseError(e.to_string()))
    }

    pub fn subject(&self) -> Option<String> {
        self.inner.headers.get_first_value("Subject")
    }

    pub fn body_text(&self) -> Result<String, Pop3Error> {
        self.inner.get_body()
            .map_err(|e| Pop3Error::ParseError(e.to_string()))
    }
}
```

```toml
# Cargo.toml — v3.0 additions

[features]
mime = ["dep:mailparse", "dep:serde", "dep:serde_json"]  # MIME parsing
pool = ["dep:bb8", "dep:async-trait"]                    # Connection pooling
reconnect = ["dep:backoff"]                               # Auto-reconnect

[dependencies]
# v2.0 deps (unchanged) ...

# v3.0 optional deps
mailparse      = { version = "0.15", optional = true }
serde          = { version = "1", features = ["derive"], optional = true }
serde_json     = { version = "1", optional = true }
bb8            = { version = "0.9", optional = true }
async-trait    = { version = "0.1", optional = true }   # Required by bb8::ManageConnection
backoff        = { version = "0.4", features = ["tokio"], optional = true }
```

---

## Data Flow

### Pipeline Command Flow

```
caller: session.send_batch(vec![Command::Dele(1), Command::Dele(2), Command::Dele(3)])
    |
    v
PipelinedSession::send_batch()
    |-- for each Command:
    |       writer.write_all(cmd.to_wire_format()).await?
    |       queue.push_back(cmd)
    |-- writer.flush().await?
    |
    v
[ TCP stream: "DELE 1\r\nDELE 2\r\nDELE 3\r\n" sent in one or few syscalls ]
    |
    v
caller: session.receive_all()
    |
    v
PipelinedSession::receive_all()
    |-- while let Some(cmd) = queue.pop_front():
    |       line = reader.read_line().await?    ("+OK message 1 deleted\r\n")
    |       result = response::parse_response_for_command(&cmd, &line)
    |       results.push(result)
    |
    v
Vec<Result<String, Pop3Error>>  -- one result per command, in order
```

### UIDL Incremental Sync Flow

```
caller: Application startup
    |
    v
UidlCache::load("~/.pop3_seen.json")
    |
    v
client.uidl(None).await?  ->  Vec<UidEntry> { msg_num: u32, uid: String }
    |
    v
cache.filter_new(&uidl_list)  ->  &[&UidEntry]  (UIDs not in seen set)
    |
    v
for entry in new_entries:
    |-- client.retr(entry.msg_num).await?  ->  raw message String
    |-- [caller processes message]
    |-- cache.mark_seen(entry.uid.clone())
    |
    v
cache.expire_deleted(&uidl_list)  (remove UIDs for server-deleted messages)
    |
    v
cache.save("~/.pop3_seen.json")
```

### Reconnection Flow

```
caller: reconnecting_client.stat().await
    |
    v
ReconnectingClient::stat()
    |-- ensure_connected()
    |       if inner.is_none():
    |           retry(backoff_config, || builder.connect().await)
    |           inner = Some(new_client)
    |
    |-- inner.as_mut().unwrap().stat().await
    |       -> Ok(result)  : return Ok(result)
    |       -> Err(Pop3Error::Io(_)):
    |               inner = None  (discard broken connection)
    |               loop back to ensure_connected()
    |       -> Err(other)  : return Err(other)  (auth failure, protocol error: do NOT retry)
    |
    v
Ok(StatResponse)
```

### Connection Pool Flow

```
caller: pool.get().await?  ->  PooledConnection<Pop3Manager>
    |                          (auto-returned to pool on drop)
    v
bb8 Pool internals:
    |-- If idle connection available:
    |       Pop3Manager::is_valid(conn).await?   (NOOP health check)
    |       return conn
    |-- If no idle connection and pool not full:
    |       Pop3Manager::connect().await?        (builder.connect())
    |       return new conn
    |-- If pool full (all in use):
    |       await until one returns (backpressure)
    |
    v
conn.stat().await?   ->  StatResponse
conn dropped -> PooledConnection returned to pool (bb8 Drop impl)
```

### MIME Parsing Flow

```
caller: client.retr(1).await?  ->  String (raw RFC 822 message)
    |
    v  [#[cfg(feature = "mime")]]
ParsedMessage::parse(raw.as_bytes())
    |-- mailparse::parse_mail(raw_bytes)
    |       -> ParsedMail { headers, body, subparts }
    |
    v
ParsedMessage { inner: ParsedMail }
    |-- .subject()     -> Option<String>
    |-- .body_text()   -> Result<String>
    |-- .headers()     -> &[MailHeader]
    |-- .subparts()    -> &[ParsedMail]  (MIME parts)
```

---

## Integration Points: New vs Modified

### New Components

| Component | File | v3.0 Purpose | Integration Point |
|-----------|------|-------------|-------------------|
| `PipelinedSession` | `src/pipeline.rs` | Batch send + ordered receive for multiple commands | Borrows `&mut Client`; uses `client.writer` and `client.reader` directly (requires them to be `pub(crate)`) |
| `UidlCache` | `src/cache.rs` | Persist seen UIDs across sessions for incremental sync | Consumes output of `client.uidl(None)` (`Vec<UidEntry>`); standalone struct, no `Client` coupling |
| `ReconnectingClient` | `src/reconnect.rs` | Transparent reconnect on `Pop3Error::Io` with exponential backoff | Owns `Client` + `Pop3ClientBuilder`; delegates all methods to inner `Client` |
| `Pop3Manager` | `src/pool.rs` | `bb8::ManageConnection` impl — creates and validates `Client` connections | Uses `Pop3ClientBuilder::connect()` for `connect()`; uses `Client::noop()` for `is_valid()` |
| `Pop3Pool` | `src/pool.rs` | Type alias for `bb8::Pool<Pop3Manager>` | Exposed in `lib.rs` behind `pool` feature flag |
| `ParsedMessage` | `src/mime.rs` | Thin wrapper calling `mailparse::parse_mail()` on raw RETR output | Constructed from raw `String` returned by `client.retr()`; no `Client` coupling |

### Modified Components

| Component | What Changes | Why |
|-----------|-------------|-----|
| `src/error.rs` | Add `Pop3Error::ConnectionClosed` variant; possibly `PoolError(String)` | `ReconnectingClient` needs to distinguish "connection dropped" (retry) from "auth failed" (don't retry). `Pool` errors need representation. |
| `src/client.rs` | Expose `writer` and `reader` fields as `pub(crate)` | `PipelinedSession` needs direct access to the split halves to bypass the per-command await cycle. |
| `src/client.rs` | Add `is_closed() -> bool` method | `bb8::ManageConnection::has_broken()` requires a way to detect a dead connection without sending a command. |
| `Pop3ClientBuilder` | Implement `Clone` | `ReconnectingClient` and `Pop3Manager` must store the builder and clone it per reconnect/connection attempt. |
| `src/lib.rs` | Re-export `PipelinedSession`, `UidlCache`, `ReconnectingClient`, `Pop3Pool`, `ParsedMessage` behind their respective feature flags | Public API surface for v3.0 callers. |
| `Cargo.toml` | Add optional deps: `mailparse`, `serde`, `serde_json`, `bb8`, `async-trait`, `backoff` | Each behind its own feature flag; core library remains unchanged for callers who enable none. |

---

## Build Order for v3.0

Dependencies between v3.0 components determine implementation order:

1. **`src/error.rs` (extend)** — Add `ConnectionClosed` variant needed by `ReconnectingClient`. Nothing else can be built until error types are final.

2. **`src/client.rs` (modify)** — Add `pub(crate)` visibility to `reader`/`writer`; add `is_closed() -> bool`; impl `Clone` on `Pop3ClientBuilder`. These are prerequisites for `pipeline.rs` and `pool.rs`.

3. **`src/cache.rs` (new)** — No dependency on other new modules. Depends only on `UidEntry` from `response.rs` (already exists). Can be built and tested independently. Add `serde` derive on `UidEntry` in `response.rs` at the same time.

4. **`src/pipeline.rs` (new)** — Depends on modified `client.rs` (pub(crate) fields) and existing `command.rs`/`response.rs`. Build after step 2.

5. **`src/reconnect.rs` (new)** — Depends on `client.rs` + `Pop3ClientBuilder` Clone impl + `backoff` crate. Build after step 2.

6. **`src/pool.rs` (new)** — Depends on `client.rs` (is_closed method) + `Pop3ClientBuilder` Clone + `bb8` crate + `async-trait`. Build after step 2.

7. **`src/mime.rs` (new)** — Depends only on `Pop3Error` (error.rs) and `mailparse` crate. No dependency on other v3.0 modules. Can be built in parallel with steps 4-6.

8. **`src/lib.rs` (extend)** — Feature-gated re-exports for all new types. Written last.

9. **`tests/pipeline_tests.rs` (new)** — Built alongside `pipeline.rs` (step 4). Uses `tokio_test::io::Builder` to replay batched command/response sequences.

10. **`tests/integration.rs` (extend)** — Add reconnect, pool, cache, and MIME integration tests after all components exist.

11. **`examples/incremental_sync.rs`, `pool_usage.rs` (new)** — Written last; validate public API ergonomics.

**Dependency graph summary:**
```
error.rs (extend)
    └──> client.rs (modify: pub(crate) fields, is_closed, builder Clone)
              ├──> pipeline.rs (new)
              ├──> reconnect.rs (new)
              └──> pool.rs (new)

response.rs (extend: serde on UidEntry)
    └──> cache.rs (new)

error.rs (extend)
    └──> mime.rs (new)   [parallel to above]
```

---

## Scaling Considerations

This is a client library — scaling applies to how v3.0 features compose under real workloads.

| Concern | Approach | Notes |
|---------|----------|-------|
| High-volume batch deletion after UIDL sync | Use `PipelinedSession` to batch `DELE` commands | Without pipelining: N round trips; with pipelining: 1 write burst + N response reads. 10x-100x latency reduction on high-latency links. |
| Processing N mailboxes concurrently | Use `Pop3Pool` with `max_size = N` | Each pooled connection targets a different mailbox. POP3 server-side exclusive locking enforces this constraint. |
| Long-running daemons on flaky networks | Use `ReconnectingClient` with jitter | `ExponentialBackoff` with jitter prevents thundering-herd reconnects. Max elapsed time prevents infinite retry on permanent failures (e.g., credential change). |
| Large mailboxes (10K+ messages) | `UidlCache` delta computation stays O(N) via `HashSet` | Both `filter_new` and `expire_deleted` are O(N) hash lookups. JSON serialization of 10K UIDs is ~1MB — acceptable for startup cost. |
| Binary size | Each v3.0 feature is behind a feature flag | Callers who enable none of `mime`, `pool`, `reconnect` pay no binary size cost for any v3.0 dependency. |

---

## Anti-Patterns

### Anti-Pattern 1: Pooling Connections to the Same Mailbox

**What people do:** Create a `Pop3Pool` with `max_size = 4` pointing all connections at one user's inbox, expecting parallel message retrieval.

**Why it's wrong:** POP3 servers exclusively lock the maildrop on PASS/APOP (RFC 1939; RFC 2449 [IN-USE] code). Connections 2-4 will receive `-ERR [IN-USE]` and fail. The pool will report `Pop3Error::ServerError("[IN-USE]")` on every borrow beyond the first.

**Do this instead:** Use `Pop3Pool` only when processing multiple distinct mailboxes. For parallel access to a single mailbox, POP3 is the wrong protocol — IMAP supports concurrent access.

### Anti-Pattern 2: Retrying on Non-Transient Errors in ReconnectingClient

**What people do:** Configure `ReconnectingClient` to retry on all `Pop3Error` variants, including `Pop3Error::AuthFailed`.

**Why it's wrong:** An auth failure means the credentials are wrong — retrying will never succeed and causes account lockouts on servers that apply rate limiting after failed logins (most do).

**Do this instead:** Only retry on `Pop3Error::Io` (network errors) and `Pop3Error::ConnectionClosed`. All other errors — `AuthFailed`, `ServerError`, `ParseError`, `NotAuthenticated` — are non-transient and must be propagated to the caller without retry. The `backoff::Error::permanent()` variant exists exactly for this distinction.

### Anti-Pattern 3: Using PipelinedSession Without Checking CAPA

**What people do:** Use `PipelinedSession::send_batch()` unconditionally on every server.

**Why it's wrong:** RFC 2449 requires clients to only pipeline if the server's `CAPA` response includes `PIPELINING`. Servers that do not advertise it may reject pipelined commands, hang waiting for a response before processing the next command, or return garbled responses that corrupt the `VecDeque` command queue.

**Do this instead:** Call `client.capa().await?` and check for `PIPELINING` in the returned capability set before constructing a `PipelinedSession`. Provide a `PipelinedSession::is_supported(capa_list)` helper to make this check ergonomic.

### Anti-Pattern 4: Holding PooledConnection Across Awaits

**What people do:** Call `pool.get().await?`, then hold the returned `PooledConnection` across multiple unrelated `await` points (e.g., storing it in a struct across HTTP request handling).

**Why it's wrong:** The connection is removed from the pool for the entire duration it is held. For a pool of size 4, holding 4 connections indefinitely starves all other callers.

**Do this instead:** Hold `PooledConnection` only for the duration of a single logical mail operation (stat + optional retr + optional dele), then drop it. For structurally long-lived operations, use `ReconnectingClient` instead of the pool.

### Anti-Pattern 5: Silently Ignoring UidlCache.expire_deleted

**What people do:** Call `cache.filter_new()` and `cache.mark_seen()` but never call `cache.expire_deleted()`.

**Why it's wrong:** The cache grows without bound. Messages deleted server-side remain in the `seen` set forever. In long-running systems, the cache JSON file grows linearly with all messages ever seen.

**Do this instead:** After every UIDL sync, call `cache.expire_deleted(&current_uidl_list)` to remove UIDs that no longer exist on the server. This keeps the cache bounded to the current server mailbox size.

---

## Integration Points: External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| `mailparse` crate | Called from `ParsedMessage::parse(raw_bytes)` | Pure-Rust, no C deps. Behind `mime` feature flag. MSRV: compatible with project MSRV 1.80. |
| `bb8` crate | `ManageConnection` trait impl in `src/pool.rs` | Requires `async-trait` for the trait impl (or Rust 1.75+ native async fn in traits — verify compatibility). Behind `pool` feature flag. |
| `backoff` crate | `ExponentialBackoff` config + `backoff::future::retry()` in `src/reconnect.rs` | Requires `backoff` with `tokio` feature. Behind `reconnect` feature flag. |
| `serde` + `serde_json` | Serialize/deserialize `UidlCache` struct to/from JSON file | Behind `mime` feature flag (reuse) or dedicated `cache` feature flag. Adds to binary size only when enabled. |
| POP3 server (real) | Connection established by `Pop3Manager::connect()` using `Pop3ClientBuilder` | No change to the v2.0 connection model. `Pop3Manager` is just a factory wrapper. |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| `pipeline.rs` ↔ `client.rs` | Direct field access (`pub(crate)` on `reader`, `writer`) | Avoids adding pipeline-specific methods to `Client`; keeps `Client` interface clean |
| `reconnect.rs` ↔ `client.rs` | Method call delegation (all public `Client` methods mirrored) | Decorator pattern; `ReconnectingClient` calls `self.inner.stat()`, etc. |
| `pool.rs` ↔ `client.rs` | `ManageConnection::connect()` calls `Pop3ClientBuilder::connect()` | Pool does not reach into `Client` internals |
| `cache.rs` ↔ `response.rs` | `cache.rs` consumes `Vec<UidEntry>` from `client.uidl()` | Requires `serde` derives on `UidEntry` in `response.rs`; otherwise standalone |
| `mime.rs` ↔ `client.rs` | `retr()` returns `String`; `ParsedMessage::parse()` consumes it | Zero coupling at the type level; callers compose the two manually |

---

## Sources

- [RFC 2449 — POP3 Extension Mechanism](https://datatracker.ietf.org/doc/html/rfc2449) — PIPELINING capability specification; command ordering constraint; [IN-USE] response code. HIGH confidence (authoritative RFC).
- [RFC 1939 — Post Office Protocol Version 3](https://www.ietf.org/rfc/rfc1939.txt) — Exclusive maildrop locking on PASS; single-session constraint per mailbox. HIGH confidence (authoritative RFC).
- [docs.rs/bb8/latest](https://docs.rs/bb8/latest/bb8/) — `ManageConnection` trait, `Pool::builder()`, `PooledConnection` Drop behavior. HIGH confidence (official docs).
- [docs.rs/backoff/latest](https://docs.rs/backoff/) — `ExponentialBackoff`, `backoff::future::retry()`, tokio feature flag, `Error::transient()` vs `Error::permanent()`. HIGH confidence (official docs).
- [docs.rs/mailparse/latest](https://docs.rs/mailparse/) — `parse_mail(&[u8])`, `ParsedMail` struct, `get_body()`, header accessors. HIGH confidence (official docs).
- [tokio Channels documentation](https://tokio.rs/tokio/tutorial/channels) — oneshot and mpsc patterns for command-response matching in async Rust. HIGH confidence (official tokio docs).
- [How to Build Connection Pools with bb8 and deadpool in Rust (Jan 2026)](https://oneuptime.com/blog/post/2026-01-25-connection-pools-bb8-deadpool-rust/view) — bb8 pool builder pattern, test_on_check_out, practical usage. MEDIUM confidence (verified against official bb8 docs).
- [How to Implement Exponential Backoff with Jitter in Rust (Jan 2026)](https://oneuptime.com/blog/post/2026-01-25-exponential-backoff-jitter-rust/view) — Jitter rationale, thundering-herd prevention, backoff crate configuration. MEDIUM confidence (community article; patterns align with official backoff docs).
- [docs.rs/tokio-retry/latest](https://docs.rs/tokio-retry/) — `Retry::spawn()`, `ExponentialBackoff`, `jitter()` strategy. HIGH confidence (official docs).
- [Efficient Database Connection Management with bb8/deadpool in Rust — Leapcell](https://leapcell.io/blog/efficient-database-connection-management-with-sqlx-and-bb8-deadpool-in-rust) — bb8 ManageConnection implementation example. MEDIUM confidence (verified against official bb8 source).
- [GitHub hMailServer forum — -ERR [IN-USE] Unable to lock maildrop](https://forums.anandtech.com/threads/err-in-use-unable-to-lock-maildrop-mailbox-is-locked-by-pop-server.2110505/) — Real-world confirmation of POP3 exclusive maildrop locking. MEDIUM confidence (community, corroborates RFC 1939 + RFC 2449).

---

*Architecture research for: rust-adv-pop3 v3.0 — advanced features integration into v2.0 async base*
*Researched: 2026-03-01*
