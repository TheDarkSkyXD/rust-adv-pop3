# Phase 8: Connection Pooling - Research

**Researched:** 2026-03-01
**Domain:** Async Rust connection pooling with bb8, per-account exclusivity enforcement, RFC 1939 mailbox locking
**Confidence:** HIGH (standard stack verified against official docs; architecture patterns verified against bb8 source and ecosystem practice)

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| POOL-01 | Client provides a connection pool for multi-account scenarios via `bb8` | bb8 0.9.1 is the mandated library; `ManageConnection` trait + `Pool` API documented below |
| POOL-02 | Pool enforces max 1 connection per mailbox (RFC 1939 exclusive lock) | bb8 `max_size(1)` per-account pool is the correct mechanism; `HashMap<AccountKey, Pool<M>>` pattern |
| POOL-03 | Pool documentation prominently warns that POP3 forbids concurrent access to the same mailbox | Implementation task: rustdoc on `Pop3Pool` struct must quote RFC 1939 §8 and explain the per-account model |

</phase_requirements>

---

## Summary

Phase 8 adds a `Pop3Pool` struct that manages a collection of POP3 connections, one per mailbox account. The key insight that shapes the entire design is that this is **not** a standard N-connection pool to the same resource. POP3 RFC 1939 mandates exclusive mailbox access — only one TCP connection may hold the maildrop lock at a time, and the server actively rejects second connections with `-ERR maildrop already locked`. Therefore, the pool is a map of per-account pools, each capped at `max_size(1)`.

The required library is bb8 0.9.1 (mandated by REQUIREMENTS.md — POOL-01). bb8 is agnostic to connection type via the `ManageConnection` trait. Since Rust 1.75, the trait uses native `impl Future` return types in method position (RPIT), so `#[async_trait]` is **not** needed. The implementation requires: a `Pop3ConnectionManager` struct implementing `ManageConnection`, a `Pop3Pool` outer struct wrapping `Arc<RwLock<HashMap<AccountKey, bb8::Pool<Pop3ConnectionManager>>>>`, and `checkout(account)` / return-by-drop semantics.

The STATE.md records two open blockers for this phase: (1) whether each `Pop3Pool` targets one account (single pool per struct instance) or supports multi-account with runtime enforcement, and (2) whether bb8 0.9 requires `#[async_trait]`. Both are now resolved by research: **bb8 0.9 uses native `impl Future`, no `async_trait` needed (MSRV 1.75)**; and the multi-account-with-runtime-enforcement pattern (HashMap of pools) is the correct architecture.

**Primary recommendation:** Implement `Pop3ConnectionManager` per-account, store `Arc<bb8::Pool<Pop3ConnectionManager>>` per account key in a `DashMap` or `Arc<RwLock<HashMap<...>>>`, set `max_size(1)` on each pool, and implement health checks via `NOOP` in `is_valid()`.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `bb8` | 0.9.1 | Async connection pool — provides `Pool<M>`, `PooledConnection`, `ManageConnection` trait | Mandated by POOL-01; 1.15M monthly downloads; official Tokio ecosystem choice; generic over connection type |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `dashmap` | latest (5.x or 6.x) | Concurrent `HashMap<AccountKey, Arc<bb8::Pool<M>>>` without a global `RwLock` | Use when pool supports multiple accounts — removes the outer `RwLock` bottleneck |
| `tokio::sync::RwLock` | tokio 1.x (already in deps) | Alternative to DashMap for the account→pool registry | Use if DashMap is rejected as a dependency; more explicit, slightly more contention |

**Dependency choice for registry:** DashMap is preferred because it eliminates the outer lock entirely through internal sharding. `Arc<RwLock<HashMap<...>>>` is acceptable if adding another crate dependency is undesirable (dashmap is not currently in Cargo.toml).

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `bb8` | `deadpool` | deadpool has no `Manager` trait overhead (simpler for unmanaged use), but is less well-known for custom protocol use; bb8 is mandated by POOL-01 |
| `bb8` | `mobc` | mobc also supports async and per-key patterns but has fewer downloads and is not mandated |
| `DashMap` for registry | `Arc<RwLock<HashMap>>` | RwLock adds contention on writes (pool creation); DashMap shards internally; difference is minor at POP3 scale |
| `bb8::Pool` per account | Single `bb8::Pool` with `max_size(N)` | Single pool to the same account violates RFC 1939; bb8 does not have built-in key-per-pool semantics |

**Installation (Cargo.toml additions for Phase 8):**
```toml
[dependencies]
bb8 = { version = "0.9", optional = true }
dashmap = { version = "6", optional = true }

[features]
pool = ["dep:bb8", "dep:dashmap"]
```

Both `bb8` and `dashmap` go behind a `pool` feature flag per the roadmap pattern for v3.0 crates.

---

## Architecture Patterns

### Recommended File Structure

```
src/
├── lib.rs           # add Pop3Pool to public re-exports (behind pool feature flag)
├── client.rs        # Pop3Client (unchanged by Phase 8)
├── pool.rs          # Pop3Pool, Pop3ConnectionManager, AccountKey — new file
└── ...
```

### Pattern 1: Per-Account bb8 Pool Registry

**What:** A `Pop3Pool` wraps a concurrent map from `AccountKey` to a `bb8::Pool<Pop3ConnectionManager>`. Each inner pool has `max_size(1)`, enforcing RFC 1939 exclusivity. Callers call `pool.get(account)`, which either lazily creates a new per-account pool or returns an existing one, then calls `bb8::Pool::get()` to check out the single connection. The call blocks (async) until the connection is available — naturally enforcing the "wait until the previous user releases" semantics required by POOL-02.

**When to use:** Whenever you need multi-account concurrent POP3 management. Each account independently blocks its own callers; accounts do not block each other.

**Conceptual sketch:**
```rust
// Source: derived from official bb8 0.9.1 docs + Rust forum per-tenant pattern
// (https://docs.rs/bb8/latest/bb8/, https://users.rust-lang.org/t/storing-connection-pools-in-hashmap/68605)

#[cfg(feature = "pool")]
pub struct Pop3Pool {
    pools: DashMap<AccountKey, Arc<bb8::Pool<Pop3ConnectionManager>>>,
}

#[cfg(feature = "pool")]
impl Pop3Pool {
    pub async fn get(&self, account: &AccountKey)
        -> Result<bb8::PooledConnection<'_, Pop3ConnectionManager>, Pop3PoolError>
    {
        // 1. Look up or create the per-account pool (lazy, holds dashmap ref briefly)
        let pool = self.get_or_create_pool(account).await?;
        // 2. Check out the single connection (blocks async until available)
        pool.get().await.map_err(Pop3PoolError::from)
    }

    async fn get_or_create_pool(&self, account: &AccountKey)
        -> Result<Arc<bb8::Pool<Pop3ConnectionManager>>, Pop3PoolError>
    {
        if let Some(pool) = self.pools.get(account) {
            return Ok(Arc::clone(&pool));
        }
        let manager = Pop3ConnectionManager::new(account.clone());
        let pool = Arc::new(
            bb8::Pool::builder()
                .max_size(1)               // RFC 1939: exactly one connection per mailbox
                .test_on_check_out(true)   // probe NOOP before handing to caller
                .connection_timeout(std::time::Duration::from_secs(30))
                .build(manager)
                .await?
        );
        self.pools.insert(account.clone(), Arc::clone(&pool));
        Ok(pool)
    }
}
```

**Critical design note:** The inner `Arc` around `bb8::Pool<M>` is mandatory. Without it, the dashmap (or RwLock) must stay locked for the entire duration the caller uses the connection, serializing all pool access. Cloning the `Arc` while briefly holding the map reference and then releasing the map guard is the standard pattern verified by the Rust forum expert consensus.

### Pattern 2: ManageConnection Implementation

**What:** `Pop3ConnectionManager` holds the connection credentials for one account. It implements the three bb8 trait methods.

```rust
// Source: bb8 0.9.1 trait definition (https://docs.rs/bb8/latest/bb8/trait.ManageConnection.html)
// and test file patterns (https://github.com/djc/bb8/blob/main/bb8/tests/test.rs)

pub struct Pop3ConnectionManager {
    host: String,
    port: u16,
    username: String,
    password: String,
    tls_mode: TlsMode,
    builder: Pop3ClientBuilder,  // Clone is provided by Phase 5
}

impl bb8::ManageConnection for Pop3ConnectionManager {
    type Connection = Pop3Client;
    type Error = Pop3Error;

    // No #[async_trait] needed — bb8 0.9.1 uses native impl Future (MSRV 1.75)
    fn connect(&self) -> impl Future<Output = Result<Self::Connection, Self::Error>> + Send {
        async move {
            let mut client = self.builder.clone().connect().await?;
            client.login(&self.username, &self.password).await?;
            Ok(client)
        }
    }

    fn is_valid(
        &self,
        conn: &mut Self::Connection,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move {
            conn.noop().await?;
            Ok(())
        }
    }

    fn has_broken(&self, conn: &mut Self::Connection) -> bool {
        conn.is_closed()  // provided by Phase 5
    }
}
```

**Key insight on `has_broken`:** This is synchronous — it must not call `await`. Use the `is_closed()` method from Phase 5 which checks internal state without I/O.

**Key insight on `is_valid`:** This is called when `test_on_check_out(true)` is set. The POP3 NOOP command is the correct health probe — it sends `NOOP\r\n` and expects `+OK`. If the connection is dead, this fails and bb8 replaces the connection by calling `connect()` again.

### Pattern 3: Connection Return by Drop (RAII)

**What:** `bb8::PooledConnection<'_, M>` implements `Drop` — when it goes out of scope, the inner connection is returned to the pool automatically. The caller never explicitly "returns" a connection.

```rust
// Caller code — connection returns to pool when checkout_guard drops
{
    let checkout_guard = pool.get(&account_key).await?;
    checkout_guard.stat().await?;
    // checkout_guard drops here → connection returned to pool
    // Next caller waiting on pool.get() is unblocked
}
```

This is exactly the "block until first connection is returned" behavior required by POOL-02.

### Pattern 4: AccountKey Type

**What:** A typed key instead of a raw string prevents category errors (confusing host+user combinations).

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountKey {
    pub host: String,
    pub port: u16,
    pub username: String,
}
```

`Hash + Eq` are required for HashMap/DashMap keys. This is the standard Rust idiom.

### Anti-Patterns to Avoid

- **Single pool for multiple accounts:** Do NOT create one `bb8::Pool` with `max_size > 1` and have it service multiple mailbox accounts. RFC 1939 makes this protocol-impossible — each connection authenticates to one specific mailbox. A pool of N connections all go to the same mailbox — exactly what RFC 1939 forbids.
- **Not wrapping pool in `Arc`:** Storing `bb8::Pool<M>` directly in the map (without the inner `Arc`) requires holding the map lock for the entire duration of pool use, serializing all access across all accounts.
- **Holding DashMap `Ref` across `.await`:** A DashMap `Ref` (returned by `.get()`) holds an internal shard lock. Do NOT hold it across an `.await` point. Clone the `Arc<Pool<M>>` first, then release the `Ref`, then `.await`.
- **Calling `std::sync::Mutex::lock()` across `.await`:** If using `std::sync::RwLock` or `Mutex` for the outer map, hold the lock only long enough to clone the `Arc<Pool>`, then release before any `.await`. Blocking mutexes must never span await points in async code.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Connection health checks with retry | Custom health probe loop | `bb8::Pool` with `test_on_check_out(true)` + `is_valid()` impl | bb8 handles retry, replacement, and backoff between connection attempts automatically |
| Connection timeout enforcement | Custom `tokio::time::timeout` wrapper | `bb8::Builder::connection_timeout()` | bb8 Builder already provides configurable connection timeout; default is 30 seconds |
| Connection lifetime expiration | Custom timer per connection | `bb8::Builder::max_lifetime()` (default 30 min) and `idle_timeout()` (default 10 min) | bb8 runs a reaper task internally |
| "Fair" checkout ordering | Custom FIFO queue | `bb8::Builder::queue_strategy(QueueStrategy::Fifo)` | bb8 provides FIFO and LIFO strategies; default is FIFO which is correct for fairness |
| Per-account mutex | `tokio::sync::Mutex` per account + `HashMap` | `bb8::Pool` with `max_size(1)` per account | bb8's `get()` already implements async wait-for-available-connection; re-implementing is duplicated effort with worse error handling |

**Key insight:** bb8's `Pool::get()` with `max_size(1)` is a correct async per-account mutex with connection lifecycle management. Do not build a custom semaphore/mutex structure — it would replicate what bb8 already provides correctly (connection creation, health checking, timeout, and replacement).

---

## Common Pitfalls

### Pitfall 1: Authentication Failure Causes Hang Until Timeout

**What goes wrong:** If credentials are wrong, `connect()` calls `login()` which returns `Pop3Error::AuthFailed`. bb8 catches this error and — with `retry_connection(true)` (the default) — retries the connection until `connection_timeout` expires (30 seconds). The caller does not immediately see the authentication failure; they wait the full timeout.

**Why it happens:** bb8's retry loop is designed for transient network failures. Authentication failures are permanent but indistinguishable to bb8 without explicit handling.

**How to avoid:** Set `retry_connection(false)` on the builder. With retry disabled, authentication failures propagate immediately to the waiter channel rather than looping. Verified against bb8 issue #141 — the PR #153 fix specifically helps when retry is disabled.

**Warning signs:** `RunError::TimedOut` returned from `pool.get()` with ~30 second latency indicates this pattern.

```rust
bb8::Pool::builder()
    .max_size(1)
    .retry_connection(false)  // CRITICAL: propagate auth failures immediately
    .build(manager)
    .await
```

### Pitfall 2: Holding Map Lock Across `await`

**What goes wrong:** Code holds a `DashMap::Ref` or `RwLock` guard across an `.await` point (e.g., during `bb8::Pool::builder().build().await`). The DashMap `Ref` holds an internal shard lock — holding it across an await point may cause a deadlock or panic.

**Why it happens:** `pool.build().await` is async. If you hold a DashMap reference while awaiting pool creation, the shard lock is held during I/O (which includes the initial connection attempt).

**How to avoid:** Check for existing pool first (release the DashMap ref), then create the pool outside the map lock, then insert. Use the entry API carefully or a two-phase check-then-insert pattern. Consider `tokio::sync::OnceCell` per account key for initialization-once semantics.

**Warning signs:** Deadlock on pool creation for new accounts under concurrent load.

### Pitfall 3: `max_size > 1` Violates RFC 1939

**What goes wrong:** Setting `max_size(2)` on a per-account pool means bb8 will attempt to maintain 2 connections to the same mailbox. The second connection attempt will receive `-ERR maildrop already locked` from the server, triggering bb8's retry loop until timeout.

**Why it happens:** RFC 1939 §8 states servers MUST acquire an exclusive-access lock on the maildrop. Two simultaneous connections to the same account is server-rejected.

**How to avoid:** Always `max_size(1)` on per-account pools.

**Warning signs:** Sporadic `-ERR maildrop already locked` errors appearing in `Pop3Error::ServerError` from `connect()`.

### Pitfall 4: `PooledConnection` Lifetime Makes Async Trait Bounds Harder

**What goes wrong:** `bb8::Pool::get()` returns `PooledConnection<'_, M>` with a lifetime tied to the pool reference. This lifetime makes it difficult to store checkout guards in async structs or return them across trait boundaries.

**Why it happens:** RAII lifetime-bound guards are correct but not ergonomic when stored.

**How to avoid:** Use `pool.get_owned()` instead of `pool.get()` to obtain a `PooledConnection<'static, M>`. Note the docs warn this makes it easier to "leak" the connection pool — only use `get_owned()` when necessary (e.g., when the guard must be stored in a struct or passed across spawned tasks).

**Warning signs:** Compiler errors about lifetime `'_` not living long enough when storing a `PooledConnection`.

### Pitfall 5: `has_broken()` Must Be Synchronous

**What goes wrong:** Developers attempt to perform I/O (e.g., `noop()` call) inside `has_broken()` because it seems like the right place for a health check.

**Why it happens:** `has_broken()` is a synchronous `fn` — it cannot `.await`. Attempting to call async methods from it either fails to compile or requires blocking the thread.

**How to avoid:** `has_broken()` must only check cached/in-memory state — use `is_closed()` from Phase 5. All actual I/O-based health checking belongs in `is_valid()`.

**Warning signs:** Compile errors about calling `.await` in a non-async context inside `has_broken()`.

---

## Code Examples

Verified patterns from official sources:

### Complete ManageConnection Impl Pattern

```rust
// Source: bb8 0.9.1 trait definition https://docs.rs/bb8/latest/bb8/trait.ManageConnection.html
// Source: bb8 test file OkManager pattern https://github.com/djc/bb8/blob/main/bb8/tests/test.rs

use std::future::Future;

pub struct Pop3ConnectionManager {
    builder: Pop3ClientBuilder,  // Clone from Phase 5
    username: String,
    password: String,
}

impl bb8::ManageConnection for Pop3ConnectionManager {
    type Connection = Pop3Client;
    type Error = Pop3Error;

    // Native impl Future — no #[async_trait] macro required (bb8 MSRV 1.75)
    fn connect(&self) -> impl Future<Output = Result<Self::Connection, Self::Error>> + Send {
        let builder = self.builder.clone();
        let username = self.username.clone();
        let password = self.password.clone();
        async move {
            let mut client = builder.connect().await?;
            client.login(&username, &password).await?;
            Ok(client)
        }
    }

    fn is_valid(
        &self,
        conn: &mut Self::Connection,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move { conn.noop().await }
    }

    fn has_broken(&self, conn: &mut Self::Connection) -> bool {
        conn.is_closed()  // synchronous state check — Phase 5 prerequisite
    }
}
```

### Pool Builder Configuration

```rust
// Source: bb8 0.9.1 Builder docs https://docs.rs/bb8/latest/bb8/struct.Builder.html
let pool: bb8::Pool<Pop3ConnectionManager> = bb8::Pool::builder()
    .max_size(1)                                               // RFC 1939 exclusive lock
    .min_idle(Some(0))                                         // don't pre-connect (lazy)
    .test_on_check_out(true)                                   // NOOP probe on checkout
    .connection_timeout(std::time::Duration::from_secs(30))    // checkout wait limit
    .idle_timeout(Some(std::time::Duration::from_secs(300)))   // 5 min idle teardown
    .max_lifetime(Some(std::time::Duration::from_secs(1800)))  // 30 min max age
    .retry_connection(false)                                   // auth failures propagate immediately
    .build(manager)
    .await?;
```

### Per-Account Registry with DashMap

```rust
// Source: Rust forum pattern https://users.rust-lang.org/t/storing-connection-pools-in-hashmap/68605
// Inner Arc is critical — allows releasing DashMap shard lock before using pool

use dashmap::DashMap;
use std::sync::Arc;

pub struct Pop3Pool {
    pools: DashMap<AccountKey, Arc<bb8::Pool<Pop3ConnectionManager>>>,
    pool_config: PoolConfig,
}

impl Pop3Pool {
    pub async fn get(
        &self,
        account: &AccountKey,
    ) -> Result<bb8::PooledConnection<'_, Pop3ConnectionManager>, Pop3PoolError> {
        let pool = self.get_or_create(account).await?;
        pool.get().await.map_err(Pop3PoolError::from)
    }

    async fn get_or_create(
        &self,
        account: &AccountKey,
    ) -> Result<Arc<bb8::Pool<Pop3ConnectionManager>>, Pop3PoolError> {
        // Step 1: try read (brief lock held, released before any .await)
        if let Some(entry) = self.pools.get(account) {
            return Ok(Arc::clone(&*entry));
            // DashMap Ref dropped here — shard lock released before any .await
        }
        // Step 2: create new pool (no map lock held during async build)
        let manager = Pop3ConnectionManager::from_account(account);
        let new_pool = Arc::new(
            bb8::Pool::builder()
                .max_size(1)
                .retry_connection(false)
                .test_on_check_out(true)
                .build(manager)
                .await
                .map_err(Pop3PoolError::BuildFailed)?,
        );
        // Step 3: insert (another brief lock, handles racing inserts gracefully)
        self.pools.entry(account.clone())
            .or_insert_with(|| Arc::clone(&new_pool));
        Ok(new_pool)
    }
}
```

### Rustdoc Warning for POOL-03

```rust
/// A connection pool for managing multiple POP3 mailbox accounts concurrently.
///
/// # RFC 1939 Exclusive Mailbox Access
///
/// **POP3 forbids concurrent access to the same mailbox.** Per RFC 1939 §8:
/// > "the POP3 server then acquires an exclusive-access lock on the maildrop"
/// > "If the maildrop cannot be opened for some reason (for example, a lock can not
/// >  be acquired), the POP3 server responds with a negative status indicator."
///
/// This pool enforces that constraint at the library level: each mailbox account
/// is backed by an independent pool capped at **one connection**. A caller
/// attempting to check out a connection to an account that is already in use will
/// wait asynchronously until the previous caller drops their `PooledConnection`.
///
/// This is a **per-account** model, not a traditional N-connection pool. Multiple
/// accounts can be accessed concurrently; a single account cannot.
pub struct Pop3Pool { ... }
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `#[async_trait]` macro for async trait methods | Native `impl Future` / `async fn` in trait position | Rust 1.75 (Dec 2023) | No external proc-macro dependency; bb8 0.9 uses this |
| `r2d2` (sync) connection pools | `bb8` (async, tokio-based) | ~2018 onward | Required for async Rust; r2d2 blocks the thread |
| Manual per-connection mutex pattern | `bb8::Pool` with `max_size(1)` | Ecosystem maturity | bb8 handles health checks, retry, and lifecycle correctly |
| Single pool for all connections of a type | Per-account pools in a registry | Community-evolved pattern | Required when connections are per-credential (POP3, email accounts) |

**Deprecated/outdated:**
- `async_trait` macro: No longer required for bb8 implementors targeting Rust 1.75+. The `async_trait` crate is still useful for `dyn`-compatible traits but is not needed here.
- `bb8 0.7/0.8`: These older versions had documented deadlock issues under high concurrency (GitHub issues #67, #122). Version 0.9.x resolved these. Use 0.9.1.

---

## Open Questions

1. **`Pop3Pool` scope: per-instance vs. singleton**
   - What we know: The roadmap calls for multi-account pooling. `Pop3Pool` could be a per-application singleton (wrapping all accounts) or the caller creates separate pools per account.
   - What's unclear: Which API surface is more ergonomic for the library consumer. The per-application singleton (DashMap of pools) is the more complete solution.
   - Recommendation: Implement `Pop3Pool` as a per-application singleton with a DashMap registry. Document that callers typically create one pool per application and pass it around via `Arc<Pop3Pool>`.

2. **Dependency decision: DashMap vs. `tokio::sync::RwLock<HashMap>`**
   - What we know: Both work. DashMap avoids the outer lock entirely through sharding. `RwLock<HashMap>` is already available via tokio (already a dependency).
   - What's unclear: Whether adding dashmap as a dependency is acceptable.
   - Recommendation: Use `tokio::sync::RwLock<HashMap<AccountKey, Arc<bb8::Pool<M>>>>` initially to avoid a new dependency. DashMap can be adopted later if profiling shows contention.

3. **`PooledConnection` return type in public API**
   - What we know: `PooledConnection<'_, M>` has a lifetime; `PooledConnection<'static, M>` (from `get_owned()`) does not.
   - What's unclear: Whether exposing bb8's `PooledConnection` type directly in the public API is desirable, or whether it should be wrapped in a newtype.
   - Recommendation: Expose `bb8::PooledConnection` directly (type alias or re-export). A newtype wrapper adds complexity without clear benefit. Document that it implements `Deref<Target = Pop3Client>`.

4. **Error type for pool operations**
   - What we know: `bb8::RunError<Pop3Error>` wraps `TimedOut` and `User(Pop3Error)`. Callers need to handle both.
   - What's unclear: Whether `Pop3Error` should gain a `PoolError` variant or a separate `Pop3PoolError` type is introduced.
   - Recommendation: Introduce a separate `Pop3PoolError` enum with `CheckoutTimeout` and `Connection(Pop3Error)` variants. Keeps the base `Pop3Error` clean.

---

## Sources

### Primary (HIGH confidence)

- [bb8 ManageConnection trait](https://docs.rs/bb8/latest/bb8/trait.ManageConnection.html) — trait definition, MSRV 1.75 requirement, impl Future signature, no async_trait needed
- [bb8 Builder struct](https://docs.rs/bb8/latest/bb8/struct.Builder.html) — all builder methods and defaults: max_size(10), test_on_check_out(true), connection_timeout(30s), retry_connection(true), idle_timeout(10min), max_lifetime(30min)
- [bb8 Pool struct](https://docs.rs/bb8/latest/bb8/struct.Pool.html) — get(), get_owned(), state(), lifecycle
- [bb8 0.9.1 Cargo.toml](https://github.com/djc/bb8/blob/main/bb8/Cargo.toml) — version 0.9.1, Rust 1.75 MSRV, tokio deps (rt, sync, time), parking_lot default feature
- [bb8 test.rs source](https://github.com/djc/bb8/blob/main/bb8/tests/test.rs) — OkManager, NthConnectionFailManager, BrokenConnectionManager example impls
- [RFC 1939 §8](https://www.rfc-editor.org/rfc/rfc1939.html) — exclusive mailbox lock on AUTHORIZATION state entry, -ERR maildrop already locked
- [Rust Blog: async fn in traits stabilized](https://blog.rust-lang.org/2023/12/21/async-fn-rpit-in-traits/) — Rust 1.75 native RPIT/async fn in trait

### Secondary (MEDIUM confidence)

- [Rust Forum: Storing connection pools in hashmap](https://users.rust-lang.org/t/storing-connection-pools-in-hashmap/68605) — expert confirmation of Arc<Pool> pattern inside Arc<RwLock<HashMap>>; inner Arc critical for avoiding serialized access
- [bb8 GitHub issue #141: auth failure hang](https://github.com/djc/bb8/issues/141) — retry_connection(false) workaround for auth failure; partially fixed in PR #153
- [bb8 GitHub issue #67: Timed out in bb8](https://github.com/djc/bb8/issues/67) — unfair mutex under high load in older versions; resolved in 0.9

### Tertiary (LOW confidence — web search, single source)

- [OneUptime: bb8 vs deadpool comparison](https://oneuptime.com/blog/post/2026-01-25-connection-pools-bb8-deadpool-rust/view) — general comparison; use bb8 for custom connections; deadpool for simpler cases
- [generalistprogrammer.com bb8 guide](https://generalistprogrammer.com/tutorials/bb8-rust-crate-guide) — usage patterns, 1.15M monthly downloads stat

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — bb8 0.9.1 mandated by REQUIREMENTS.md; trait signature and MSRV verified against official docs.rs
- Architecture (per-account pool registry): HIGH — inner Arc pattern verified against Rust forum expert advice; DashMap/RwLock tradeoffs are well-understood
- Architecture (ManageConnection impl): HIGH — derived from bb8 test.rs source + official trait definition
- Pitfalls: MEDIUM-HIGH — auth failure hang (issue #141) and timeout issues (issue #67) are documented bugs with known mitigations; has_broken sync constraint is from official trait definition
- Open questions: MEDIUM — API surface decisions (error type, newtype vs re-export) are judgment calls that the planner should make explicit in PLAN.md tasks

**Research date:** 2026-03-01
**Valid until:** 2026-06-01 (bb8 is stable; unlikely to change in 90 days; re-verify if Rust edition or async trait syntax changes)
