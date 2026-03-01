# Stack Research

**Domain:** Async Rust network protocol client library (POP3)
**Researched:** 2026-03-01
**Confidence:** HIGH (core new crates), MEDIUM (pipelining internal design)

---

## Context: Scope of This Document

This is a **milestone supplement** to the existing v2.0 STACK.md. v2.0 validated:
- `tokio 1.49` — async runtime
- `rustls 0.23` / `tokio-rustls 0.26` — rustls TLS backend
- `openssl 0.10` / `tokio-openssl 0.6` — openssl TLS backend
- `thiserror 2` — typed error enum
- `regex 1` — response parsing

**Do not re-research or change any of the above.** This document answers only: what NEW crate
dependencies are needed for v3.0 features (pipelining, UIDL caching, automatic reconnection
with exponential backoff, connection pooling, and optional MIME integration)?

---

## Summary: New Dependencies for v3.0

| v3.0 Feature | New Crate | Version |
|---|---|---|
| Exponential backoff reconnection | `backon` | 1.6 |
| Connection pooling | `bb8` | 0.9 |
| UIDL cache persistence (JSON) | `serde` + `serde_json` | 1.0 |
| Optional MIME parsing | `mail-parser` | 0.11 (optional feature) |
| Pipelining | No new crates — `std::collections::VecDeque` + existing `tokio` | — |

**Net crate additions: 4 crates** (`backon`, `bb8`, `serde`, `serde_json`) plus one optional
(`mail-parser`). No new tokio features beyond what v2.0 already requires.

---

## Recommended Stack Additions

### Core Technologies Added in v3.0

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| `backon` | 1.6 | Exponential backoff for automatic reconnection | Current-generation retry crate. `.retry(ExponentialBuilder::default()).await` integrates directly at async call sites without wrapper functions. The older `backoff` crate is unmaintained (RUSTSEC-2025-0012); `tokio-retry 0.3.0` is at 0.3 since 2021 and has a less ergonomic API. `backon` supports jitter, max\_delay, max\_times, and works on tokio without any feature flags. |
| `bb8` | 0.9 | Connection pooling: pool of async POP3 connections | Designed for "any async connection type via `ManageConnection` trait." This is exactly the pattern needed: implement `ManageConnection` for the POP3 client struct to give bb8 create/validate/reclaim semantics. `deadpool` targets database-centric workloads; `mobc` adds more configuration complexity than needed. bb8 is the simplest fit for a custom async protocol. Version 0.9.1 is current. |
| `serde` | 1.0 | Derive `Serialize`/`Deserialize` for UIDL cache struct | Framework-level serialization; zero runtime overhead via derive macros. Version 1.0.228 (2025). |
| `serde_json` | 1.0 | Serialize UIDL cache to/from JSON on disk | Standard JSON backend for serde; 523K+ downloads; tokio-compatible (uses `tokio::fs` for async write, serde\_json for serialization in blocking task via `spawn_blocking`). Version 1.0.149 (2026-01-06). |

### Optional Feature: MIME Parsing

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `mail-parser` | 0.11 | Parse raw RFC 5322 + MIME message returned by RETR | Behind `mime` feature flag. Converts raw bytes from RETR into `Message` struct with `.text_bodies()`, `.html_bodies()`, `.attachments()`. 132K downloads; 0 external required dependencies (encoding\_rs is optional for CJK charsets). Has native `serde` feature flag for serializing parsed messages. |

**Why `mail-parser` over `mailparse`:**
- `mail-parser` provides a **flattened, human-friendly API** (`.text_bodies()`, `.html_bodies()`, `.attachments()`) vs. `mailparse`'s raw nested MIME tree — less work for the caller
- Zero required external dependencies (100% safe Rust)
- Supports 41 character sets including legacy CJK formats (encoding\_rs optional)
- Has a `serde` feature for serializing parsed messages
- `mailparse 0.16.1` has 523K downloads (very popular) but returns nested `ParsedMail` trees that callers must traverse manually; appropriate if users need full raw MIME access
- Either choice works — `mail-parser` is recommended because its API is lower friction for application developers

---

## Pipelining: No New Crates Required

RFC 2449 pipelining is implemented entirely with existing `tokio` primitives and `std`:

```
Client writer task:
  1. BufWriter::write_all(command_bytes).await   ← existing tokio io-util
  2. VecDeque::push_back(pending_tag)            ← std::collections::VecDeque
  3. BufWriter::flush().await                    ← flushes batched writes

Client reader task:
  4. BufReader::read_line().await                ← existing tokio io-util
  5. VecDeque::pop_front()                       ← match response to command
```

The key: `BufWriter` accumulates multiple command writes before a single `flush()`, achieving
the pipelining window. The `VecDeque<PendingCommand>` tracks which response belongs to which
outstanding command (RFC 2449 requires responses in command order).

**What NOT to reach for:**
- `tokio::sync::mpsc` channels: Useful when pipelining spans multiple tasks, but for a
  single-connection client the simpler split-reader/writer approach with `VecDeque` avoids
  channel overhead. Channels add complexity only if the public API exposes concurrent
  callers on one connection — which conflicts with POP3's sequential command model.
- `bytes::Bytes`: Not needed; POP3 commands are short ASCII lines.

---

## UIDL Cache: Implementation Pattern

The UIDL cache persists a `HashMap<String, u32>` (UIDL string → message number at last sync)
to a user-supplied file path between sessions.

**In-memory:** `std::collections::HashMap<String, u32>` — no external crate
**Persistence:** `serde_json` for JSON serialization + `tokio::fs` for async file write

```toml
# In Cargo.toml [features]
uidl-cache = ["dep:serde", "dep:serde_json"]
```

```rust
// Serialization via serde derive — zero-cost abstraction
#[derive(serde::Serialize, serde::Deserialize)]
pub struct UidlCache {
    pub entries: HashMap<String, u32>,
}

// Write to disk (non-blocking path via spawn_blocking or tokio::fs)
let json = serde_json::to_string(&cache)?;
tokio::fs::write(&path, json).await?;
```

**Why JSON instead of bincode/MessagePack:**
- Human-readable for debugging (users can inspect/edit the cache file)
- No additional crate beyond `serde_json` (already needed)
- Acceptable performance at POP3 scale (mailbox UIDs are rarely > 50K entries)

**Why NOT `dashmap` or `moka`:**
- UIDL cache is single-session, single-task (no concurrent readers/writers)
- `Arc<RwLock<HashMap>>` is unnecessary — the cache is owned by the session
- Adds a dependency for zero benefit at this scale

---

## Cargo.toml Structure (v3.0 additions)

```toml
[features]
# Existing v2.0 features (unchanged)
openssl  = ["dep:openssl", "dep:tokio-openssl"]
rustls   = ["dep:rustls", "dep:tokio-rustls", "dep:webpki-roots"]

# New v3.0 features
uidl-cache    = ["dep:serde", "dep:serde_json"]
connection-pool = ["dep:bb8"]
mime          = ["dep:mail-parser"]

[dependencies]
# --- Existing v2.0 dependencies (do not change) ---
tokio        = { version = "1.49", features = ["net", "io-util", "macros", "rt-multi-thread"] }
thiserror    = "2.0"
regex        = "1"

openssl      = { version = "0.10", optional = true }
tokio-openssl = { version = "0.6", optional = true }
rustls       = { version = "0.23", default-features = false, features = ["logging", "std", "tls12", "ring"], optional = true }
tokio-rustls = { version = "0.26", optional = true }
webpki-roots = { version = "1.0", optional = true }

# --- New v3.0 dependencies ---
backon        = "1.6"                           # always included; exponential backoff is core reconnection logic
bb8           = { version = "0.9", optional = true }  # connection-pool feature
serde         = { version = "1.0", features = ["derive"], optional = true }  # uidl-cache feature
serde_json    = { version = "1.0", optional = true }  # uidl-cache feature
mail-parser   = { version = "0.11", optional = true }  # mime feature

[dev-dependencies]
tokio-test = "0.4"
tokio      = { version = "1.49", features = ["rt", "macros"] }
```

**Rationale for `backon` being always-on (not optional):**

Automatic reconnection with exponential backoff is part of the core v3.0 client contract.
Making it optional creates conditional compilation paths in the reconnection logic that
provide no meaningful binary size savings (backon has zero runtime overhead when not
triggered). If binary size is critical, `backon`'s compile-time footprint is negligible
compared to TLS crates already in the graph. Keep it unconditional.

---

## bb8 ManageConnection Integration Point

The POP3 connection pool requires implementing `bb8::ManageConnection` for the `Pop3Client`
connection manager:

```rust
use bb8::ManageConnection;

pub struct Pop3ConnectionManager {
    host: String,
    port: u16,
    // TLS config, auth credentials, etc.
}

impl ManageConnection for Pop3ConnectionManager {
    type Connection = Pop3Client;  // the v2.0 client type
    type Error = Pop3Error;

    async fn connect(&self) -> Result<Pop3Client, Pop3Error> {
        // Build and authenticate a new POP3 connection
        Pop3ClientBuilder::new(&self.host, self.port)
            .connect().await
    }

    async fn is_valid(&self, conn: &mut Pop3Client) -> Result<(), Pop3Error> {
        // NOOP keeps the connection alive; use as health check
        conn.noop().await
    }

    fn has_broken(&self, _conn: &mut Pop3Client) -> bool {
        // Return true if the connection is known-broken
        // (e.g., after a write error sets an internal flag)
        false  // implement with interior broken flag on Pop3Client
    }
}
```

**Important POP3 constraint:** POP3 servers enforce a single active session per mailbox
(`[IN-USE]` lock). A connection pool therefore makes sense only for multi-mailbox scenarios
or test environments — document this clearly in the pool API. Do NOT pool connections to the
same mailbox from the same account concurrently.

bb8 pool defaults:
- `max_size`: 10 connections
- `connection_timeout`: 30 seconds
- `idle_timeout`: 10 minutes
- `max_lifetime`: 30 minutes
- `test_on_check_out`: true (calls `is_valid` before returning connection)

---

## backon ExponentialBuilder Configuration

```rust
use backon::{ExponentialBuilder, Retryable};

// Recommended reconnection policy for POP3
let backoff = ExponentialBuilder::default()
    .with_min_delay(Duration::from_secs(1))   // start at 1 second
    .with_max_delay(Duration::from_secs(60))  // cap at 60 seconds
    .with_max_times(5)                        // 5 attempts before giving up
    .with_factor(2.0)                         // double each time: 1s, 2s, 4s, 8s, 16s
    .with_jitter();                           // add jitter to avoid thundering herd

let result = connect_to_server
    .retry(backoff)
    .when(|e| e.is_transient())  // only retry transient errors (network, timeout)
    .await?;
```

backon defaults (for reference):
- `min_delay`: 1 second
- `max_delay`: 60 seconds
- `factor`: 2.0
- `max_times`: 3 attempts
- `jitter`: disabled

The recommended v3.0 policy uses 5 retries with jitter enabled.

---

## Alternatives Considered

| Recommended | Alternative | Why Not |
|-------------|-------------|---------|
| `backon 1.6` | `tokio-retry 0.3` | tokio-retry uses iterator-based API that's less ergonomic than backon's `.retry()` method chaining; last meaningful update 2021; no conditional retry on error type without `RetryIf` wrapper |
| `backon 1.6` | `backoff` crate | RUSTSEC-2025-0012: unmaintained; no longer updated |
| `bb8 0.9` | `deadpool 0.13` | deadpool's `Manager` trait is designed for database connections; bb8's `ManageConnection` has equivalent semantics with better documentation for custom async connection types |
| `bb8 0.9` | `mobc` | mobc adds configuration complexity (metrics, hooks) not needed for POP3; smaller ecosystem |
| `mail-parser 0.11` | `mailparse 0.16` | mailparse returns nested MIME tree requiring manual traversal; mail-parser provides flattened `.text_bodies()` / `.attachments()` API that reduces boilerplate for users |
| `serde_json` for UIDL cache | `bincode` or `postcard` | JSON is human-readable (users can inspect/repair cache files); serde\_json already in dependency graph for other potential uses; performance acceptable at POP3 mailbox scale |
| `std::collections::VecDeque` for pipelining | External queue crate | Pipelining requires only ordered FIFO of pending command tags; std VecDeque covers this with no dependency cost |

---

## What NOT to Add

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `dashmap` | Concurrent HashMap — overkill for UIDL cache which is single-owner single-task | `std::collections::HashMap` (no concurrency needed) |
| `moka` | Async-aware TTL cache — pipelining and UIDL do not need TTL eviction | `std::collections::HashMap` + `serde_json` |
| `bytes` crate | POP3 commands are short ASCII lines; no zero-copy buffer slicing needed | `String` and `Vec<u8>` |
| `async-trait` | Not needed: Rust 1.75+ native `async fn` in traits; `bb8::ManageConnection` uses `impl Future` returns | Native `async fn` in impl blocks |
| `tokio::sync::mpsc` for pipelining | Channels add cross-task overhead; pipelining is a single-task writer/reader split using `BufWriter` + `VecDeque` | `tokio::io::BufWriter` + `std::collections::VecDeque` |
| `once_cell` | MSRV is 1.80; `std::sync::LazyLock` covers all use cases from Rust 1.80+ | `std::sync::LazyLock` (already decided in v2.0) |

---

## Feature Flag Design (v3.0 additions)

```
              pop3 crate (v3.0)
               /      \         \          \
   [feature: openssl] [feature: rustls]  [feature: uidl-cache]  [feature: mime]
         |                  |                    |                     |
   openssl 0.10         rustls 0.23          serde 1.0           mail-parser 0.11
   tokio-openssl 0.6    tokio-rustls 0.26    serde_json 1.0
                        webpki-roots 1.0

              [feature: connection-pool]
                        |
                     bb8 0.9

              [always included]
                        |
                     backon 1.6
```

**Rules:**
- `uidl-cache` and `mime` are independent; both can be enabled simultaneously
- `connection-pool` does not require `uidl-cache` (they are orthogonal features)
- `backon` is always compiled in because reconnection is part of the core async client
- TLS backend selection is unchanged from v2.0 (`openssl` XOR `rustls`)

---

## Version Compatibility Matrix (v3.0 additions)

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| `backon 1.6` | `tokio 1.49` | backon is runtime-agnostic; provides `tokio::time::sleep` integration via trait; no feature flags required for tokio |
| `bb8 0.9.1` | `tokio 1.49` | bb8 is tokio-native; uses `tokio::time` internally; no conflicts |
| `serde 1.0.228` | all above | Pure macro crate; zero runtime; compatible with everything |
| `serde_json 1.0.149` | `serde 1.0.228` | serde\_json 1.0 requires serde 1.0; version ranges are compatible |
| `mail-parser 0.11` | all above | Zero external required dependencies; optional `encoding_rs 0.8` for CJK; `serde 1.0` for its own serde feature |

**MSRV impact of new crates:**
- `backon 1.6`: not explicitly documented; `edition = "2021"` (requires Rust 1.56+); in practice compatible with MSRV 1.80 already set by `LazyLock`
- `bb8 0.9`: targets Rust stable; compatible with MSRV 1.80
- `serde 1.0` / `serde_json 1.0`: long-standing; compatible with MSRV 1.80
- `mail-parser 0.11`: `edition = "2021"`; compatible with MSRV 1.80
- **No MSRV change from v2.0's 1.80 is required.**

---

## Installation

```bash
# Users who want all v3.0 features
cargo add pop3 --features "rustls,uidl-cache,connection-pool,mime"

# Users who only want reconnection (always available — backon is unconditional)
cargo add pop3 --features rustls

# Users who want UIDL caching for incremental sync
cargo add pop3 --features "rustls,uidl-cache"

# Users who want connection pooling (multi-mailbox)
cargo add pop3 --features "rustls,connection-pool"

# Library development setup
cargo add --dev tokio-test
```

---

## Sources

- [docs.rs/backon/latest](https://docs.rs/backon/latest/backon/) — version 1.6.0 confirmed; `ExponentialBuilder` API (min\_delay, max\_delay, factor, max\_times, jitter) verified. Confidence: HIGH.
- [docs.rs/backon/latest/backon/struct.ExponentialBuilder.html](https://docs.rs/backon/latest/backon/struct.ExponentialBuilder.html) — defaults (1s, 60s, factor 2.0, 3 retries, jitter off) verified. Confidence: HIGH.
- [rustmagazine.org/issue-2/how-i-designed-the-api-for-backon](https://rustmagazine.org/issue-2/how-i-designed-the-api-for-backon-a-user-friendly-retry-crate/) — backon API philosophy and ergonomic advantages over older crates. Confidence: HIGH.
- [magazine.ediary.site — backoff crate unmaintained](https://magazine.ediary.site/blog/rusts-backoff-crate-why-its) — RUSTSEC-2025-0012 confirmed. Confidence: HIGH.
- [docs.rs/bb8/latest/bb8/trait.ManageConnection.html](https://docs.rs/bb8/latest/bb8/trait.ManageConnection.html) — ManageConnection trait signature (version 0.9.1) verified; `connect()`, `is_valid()`, `has_broken()` methods confirmed. Confidence: HIGH.
- [docs.rs/bb8/0.9.1/bb8/struct.Builder.html](https://docs.rs/bb8/0.9.1/bb8/struct.Builder.html) — Pool builder defaults (max\_size 10, connection\_timeout 30s, idle\_timeout 10m, max\_lifetime 30m, test\_on\_check\_out true) verified. Confidence: HIGH.
- [oneuptime.com — bb8 vs deadpool comparison 2026](https://oneuptime.com/blog/post/2026-01-25-connection-pools-bb8-deadpool-rust/view) — "Choose bb8 if you need flexibility or work with custom connection types" confirmed. Confidence: MEDIUM (blog post, not official docs).
- [docs.rs/mail-parser/latest](https://docs.rs/mail-parser/) — version 0.11.2 confirmed; feature flags (serde, full\_encoding, rkyv) verified via raw Cargo.toml. Confidence: HIGH.
- [github.com/stalwartlabs/mail-parser Cargo.toml](https://raw.githubusercontent.com/stalwartlabs/mail-parser/main/Cargo.toml) — zero required external dependencies confirmed; optional: `encoding_rs 0.8`, `serde 1.0`, `rkyv 0.8`. Confidence: HIGH.
- [lib.rs/email](https://lib.rs/email) — download counts: mailparse 523K, mail-parser 132K — mailparse more widely deployed but mail-parser growing. Confidence: MEDIUM.
- [docs.rs/serde/latest](https://docs.rs/serde/latest/) — version 1.0.228 (2025-09-27) confirmed. Confidence: HIGH.
- [docs.rs/serde_json/latest](https://docs.rs/serde_json/latest/) — version 1.0.149 (2026-01-06) confirmed. Confidence: HIGH.
- [tokio.rs/tokio/tutorial/channels](https://tokio.rs/tokio/tutorial/channels) — tokio mpsc + oneshot channel pattern for pipelining confirmed; VecDeque approach noted as simpler for single-task use. Confidence: HIGH.
- [datatracker.ietf.org/doc/html/rfc2449](https://datatracker.ietf.org/doc/html/rfc2449) — RFC 2449 PIPELINING capability: server processes commands in order; client tracks outstanding commands in order; BufWriter window approach confirmed. Confidence: HIGH.

---

*Stack research for: rust-adv-pop3 v3.0 advanced features*
*Researched: 2026-03-01*
