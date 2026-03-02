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

---

## Supplement: Pool Lifecycle Management

**Researched:** 2026-03-01
**Domain:** bb8 shutdown semantics, reaper internals, account removal safety, Tokio runtime teardown, POP3-specific lifecycle
**Confidence:** HIGH (bb8 source verified from github.com/djc/bb8; Tokio runtime drop behavior from official docs.rs; RFC 1939 text quoted directly)

---

### 1. Pool Shutdown and Graceful Cleanup

#### How bb8::Pool Cleans Up When Dropped

bb8 does **not** implement a custom `Drop` for `Pool<M>`. When the last `Arc<Pool<M>>` (or `Pool<M>` itself, which wraps `Arc<SharedPool<M>>`) is dropped, Rust's default destructor fires and the `SharedPool<M>` is freed. This means:

- All idle connections stored inside `SharedPool` are dropped in place.
- `Drop` on a `Pop3Client` does **not** send QUIT. It simply closes the TCP stream.
- No QUIT command is ever sent to the server as a result of pool drop alone.

**This is intentional and correct for the error/abrupt-teardown case.** RFC 1939 states:

> "If a session terminates for some reason other than a client-issued QUIT command, the POP3 session does NOT enter the UPDATE state and MUST not remove any messages from the maildrop."

Abrupt TCP close (no QUIT) preserves all pending DELE marks as not-applied. This is the safe default.

#### The Reaper's Self-Terminating Design (Weak Reference)

The reaper task is spawned with a `Weak<SharedPool<M>>` reference, not a strong `Arc`. The reaper's run loop is:

```rust
// Source: github.com/djc/bb8/blob/main/bb8/src/inner.rs
async fn run(mut self) {
    loop {
        let _ = self.interval.tick().await;
        let pool = match self.pool.upgrade() {
            Some(inner) => PoolInner { inner },
            None => break,   // All Arc<SharedPool> are gone — self-terminate
        };
        let approvals = pool.inner.reap();
        pool.spawn_replenishing_approvals(approvals);
    }
}
```

**Key consequence:** When the last `Arc<SharedPool<M>>` is dropped (the `Pool<M>` and all clones of it go away), `Weak::upgrade()` returns `None` on the reaper's next tick, and the reaper task exits cleanly. No manual cancellation is needed. The reaper is self-terminating by design.

#### The `Pop3Pool` Registry Drop Chain

If `Pop3Pool` is dropped (or all `Arc<Pop3Pool>` references are released):
1. `DashMap<AccountKey, Arc<bb8::Pool<Pop3ConnectionManager>>>` is dropped.
2. Each `Arc<bb8::Pool<Pop3ConnectionManager>>` loses one reference. If no caller holds a checkout guard (which holds an `Arc<Pool>` via `get_owned()` or a lifetime-bound borrow), the `Arc` count reaches zero.
3. `SharedPool<M>` is freed, dropping all idle `Pop3Client` connections (TCP streams closed, no QUIT).
4. On the next reaper tick for each inner pool, `Weak::upgrade()` returns `None`, and the reaper task exits.

If a caller **does** hold a checkout guard, the `Arc<SharedPool<M>>` survives until that guard is dropped. This is safe — the connection continues to be valid, the pool just no longer accepts new checkouts from the registry level.

#### Should Pop3Pool Have an Explicit `shutdown()` Method?

**Yes, for intentional graceful teardown.** The `shutdown()` method should:
1. Remove all entries from the DashMap registry (so no new checkouts can happen).
2. Iterate the pools and — for each pool where a connection is currently idle — check it out briefly and call `quit()` on it.
3. Drop all the pools, releasing the `Arc<SharedPool<M>>` for each.

This is the only way to ensure QUIT is sent before the TCP connection closes, which is the correct behavior for intentional application shutdown.

**Recommended API:**

```rust
impl Pop3Pool {
    /// Gracefully shut down all pooled connections.
    ///
    /// For each account with an idle connection, sends QUIT to the POP3 server
    /// before closing the TCP stream. This enters the POP3 UPDATE state on the
    /// server side, committing any pending DELE marks.
    ///
    /// Connections that are currently checked out are not waited on — they will
    /// be closed (without QUIT) when their checkout guard is dropped, because the
    /// pool is no longer available to receive them back.
    ///
    /// After this method returns, `get()` will return `Pop3PoolError::PoolShutDown`
    /// for all subsequent calls.
    pub async fn shutdown(&self) {
        // Step 1: drain the registry so no new checkouts can start
        let pools: Vec<Arc<bb8::Pool<Pop3ConnectionManager>>> =
            self.pools.iter().map(|e| Arc::clone(e.value())).collect();
        self.pools.clear();

        // Step 2: for each inner pool, attempt to get the idle connection
        // and send QUIT. If get() fails (connection in use or timed out), skip.
        for pool in pools {
            // Try non-blocking checkout — if busy, just drop and let the OS close
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                pool.get()
            ).await {
                Ok(Ok(mut conn)) => {
                    let _ = conn.quit().await;  // best-effort; ignore error
                }
                _ => {} // connection in use or timeout — just drop
            }
        }
        // Step 3: Arcs go out of scope → SharedPool freed → reaper self-terminates
    }
}
```

**Prescriptive decision:** Implement `Pop3Pool::shutdown()`. Document that applications **must** call it before exiting if they want QUIT to be sent. Do **not** implement `Drop` with blocking I/O — async I/O in `Drop` is not supported in Rust. The `Drop` impl should only do cleanup of in-memory state (clearing the DashMap). The POP3 QUIT is inherently async and must be driven by an explicit `shutdown()` call.

#### Should Pop3ConnectionManager Implement a Custom Drop?

**No.** `Pop3ConnectionManager` does not hold a live connection — it holds credentials and config for creating connections. The manager itself has nothing to clean up. The connection lifecycle is owned by bb8's `SharedPool` internals.

`Pop3Client` dropping a TCP stream without QUIT is correct for the error case (RFC 1939 mandates no UPDATE state, so messages are not deleted). Only intentional shutdown needs QUIT, and that is handled by `Pop3Pool::shutdown()`.

---

### 2. Removing Accounts from the Registry

#### Should Pop3Pool Support `remove_account(key)`?

**Yes, with important safety semantics.** Use cases: an account is being deleted from the application, credentials are being rotated, or the operator wants to force a pool reset for one account.

#### What Happens to In-Flight Checkouts

When `remove_account(key)` removes an entry from the DashMap:
- The DashMap entry's `Arc<bb8::Pool<Pop3ConnectionManager>>` loses one reference count.
- If a caller currently holds a checkout guard (`PooledConnection<'_, M>` or a `get_owned()` result), that guard holds a reference to the pool (either a lifetime-bound borrow keeping the `Arc` alive, or an owned clone via `get_owned()`). The `Arc` count does **not** reach zero.
- The checked-out connection remains valid and usable for the caller who holds it.
- When the caller drops the checkout guard, `put_back()` is called, which tries to return the connection to the pool. At this point, if the pool `Arc` was the only remaining one, the pool is freed. If other clones exist (from other checkout guards or concurrent `get()` calls in progress), the pool survives until those are also released.

**This is safe.** Rust's `Arc<T>` reference counting ensures the pool is not freed while any holder has a reference. There is no use-after-free risk.

#### Should Removal Wait Until All Checkouts Are Returned?

**No — not by default.** Waiting would require an async barrier synchronized with bb8's internal refcount, which bb8 does not expose. The safe pattern is:

```rust
impl Pop3Pool {
    /// Remove an account's pool from the registry.
    ///
    /// Any currently-checked-out connection for this account remains valid
    /// until the caller drops their checkout guard. After removal, no new
    /// checkouts for this account key are possible.
    ///
    /// Does NOT send QUIT to the server. If a graceful disconnect is needed,
    /// call `pool.get(account)` to acquire the connection and call `quit()`
    /// before calling `remove_account()`.
    pub fn remove_account(&self, account: &AccountKey) {
        self.pools.remove(account);
        // Arc refcount decremented. Pool freed when last checkout guard drops.
    }
}
```

**If the caller needs a graceful QUIT before removal:**

```rust
// Caller-side pattern for graceful account removal:
if let Ok(mut conn) = pool.get(&account_key).await {
    let _ = conn.quit().await;  // sends QUIT, server releases lock
    // conn drops here → put_back → pool returns connection
}
pool.remove_account(&account_key);
// After remove, the inner pool's Arc refcount is 1 (held by the idle entry
// in SharedPool). Since we just returned the connection via quit() and drop,
// and remove_account drops the last external Arc, the pool is freed.
```

#### Race Condition: Concurrent get() and remove_account()

A subtle race exists: caller A calls `get()`, extracts the `Arc<Pool>` from the DashMap (releases the DashMap shard lock), then the scheduler runs caller B who calls `remove_account()` removing the DashMap entry. Caller A still has the `Arc<Pool>` clone (extracted before the remove), so it can proceed with `pool.get().await` safely — the pool object exists in memory, just not in the registry map anymore.

This is the correct behavior: the remove only prevents future registry lookups; it does not invalidate existing references. The Rust `Arc` guarantees this.

---

### 3. Idle Connection Reaping

#### bb8 Reaper: When It Is Spawned

The reaper task is spawned in `PoolInner::new()` **only when** `max_lifetime` or `idle_timeout` is configured:

```rust
// Source: github.com/djc/bb8/blob/main/bb8/src/inner.rs
if inner.statics.max_lifetime.is_some() || inner.statics.idle_timeout.is_some() {
    let start = Instant::now() + inner.statics.reaper_rate;
    let interval = interval_at(start.into(), inner.statics.reaper_rate);
    tokio::spawn(Reaper { interval, pool: Arc::downgrade(&inner) }.run());
}
```

**Implication for Pop3Pool:** Because the recommended builder sets both `idle_timeout` and `max_lifetime`, the reaper is always spawned. It requires an active Tokio runtime to exist when the pool is built. This means:
- `bb8::Pool::builder().build(manager).await` must be called within a `tokio::Runtime` context.
- The spawned reaper task is associated with the Tokio runtime it was spawned on.

#### Reaper Interval (Default: 30 Seconds)

The `reaper_rate` defaults to **30 seconds** (verified from bb8 issue #157). The reaper wakes every 30 seconds and checks all idle connections against `idle_timeout` and `max_lifetime`. This means:
- If `idle_timeout = 5 min`, the connection may live up to `5 min + 30 sec` before being reaped (the reaper fires 30s late).
- For POP3, where servers enforce a 10-minute autologout, a 5-minute `idle_timeout` means connections are reliably reaped well before the server drops them. The 30-second reaper lag is acceptable.

#### What the Reaper Does to Expired Connections

The `reap()` function (from `internals.rs`) uses `retain()` to remove expired entries from the idle connection queue:

```rust
// Source: github.com/djc/bb8/blob/main/bb8/src/internals.rs
self.conns.retain(|conn| {
    let mut keep = true;
    if let Some(timeout) = config.idle_timeout {
        if now - conn.idle_start >= timeout { keep &= false; }
    }
    if let Some(lifetime) = config.max_lifetime {
        if conn.conn.is_expired(now, lifetime) { keep &= false; }
    }
    keep
});
```

The removed `IdleConn<Pop3Client>` is simply dropped — `retain()` discards them. This means:
- **No QUIT is sent.** The TCP stream is closed by `Drop` on `Pop3Client` (or the underlying `TcpStream`).
- The server sees an abrupt connection close. Per RFC 1939, no UPDATE state is entered, no messages are deleted.

**Verdict for POP3:** bb8's reaper dropping connections without QUIT is **correct** for idle connections. An idle pooled POP3 connection has no pending DELE marks (callers using the pool for DELE would not be letting the connection sit idle in the pool). The lock is released by the server when it detects the TCP close.

#### Should Idle Connections Send QUIT Before Being Dropped?

**No, not in the reaper path.** The reaper is synchronous (`retain()` is a sync closure) and cannot send async QUIT. Attempting to block on QUIT inside `retain()` would deadlock or panic. This is correct behavior — the reaper is purely a resource-cleanup mechanism, not a protocol-state machine.

If the caller needs QUIT on idle connections (for log audit trails on the server side), they must call `Pop3Pool::shutdown()` explicitly, which performs async QUIT before dropping pools.

#### Correct idle_timeout for POP3

| Server | Documented Idle Timeout | Source |
|--------|------------------------|--------|
| RFC 1939 minimum | 10 minutes | RFC 1939 §8 |
| Dovecot POP3 default | 10 minutes | Dovecot docs (CLIENT_IDLE_TIMEOUT_MSECS) |
| Gmail POP3 | ~7-10 minutes (estimated from reports) | Gmail community forums |
| Courier POP3 | 10 minutes | Courier docs |
| Exchange/Outlook | ~10 minutes | Microsoft docs |

**Recommendation:** Set `idle_timeout = 5 minutes` (300 seconds). This is half the RFC 1939 minimum server autologout timer, providing a comfortable safety margin. A connection idle for 5 minutes in the pool will be reaped before the server's 10-minute timer fires, preventing the server from dropping the connection first and leaving the pool with a broken connection.

```rust
// Recommended idle_timeout for POP3 pool builder
.idle_timeout(Some(std::time::Duration::from_secs(300)))   // 5 min (half RFC min)
.max_lifetime(Some(std::time::Duration::from_secs(1800)))  // 30 min max age
```

---

### 4. Connection Lifecycle After Errors

#### bb8 Connection State Machine

bb8 connections pass through these states (inferred from source and trait docs):

```
CHECKOUT FLOW:
  pool.get() called
       │
       ▼
  Wait for available slot (or create new connection via connect())
       │
       ▼
  [If test_on_check_out = true] → call is_valid()
       │                              │
       │                              ├─ Ok(()) → connection is healthy
       │                              └─ Err(_) → connection dropped; retry connect()
       │
       ▼
  [Always] → call has_broken()
       │         │
       │         ├─ false → proceed
       │         └─ true  → connection dropped; retry connect()
       │
       ▼
  PooledConnection<'_, M> returned to caller

RETURN FLOW (PooledConnection dropped):
  put_back() called
       │
       ├─ is_expired(max_lifetime) → drop (no QUIT)
       ├─ [has_broken() called again at return] → drop (no QUIT)
       └─ otherwise → returned to idle queue
```

#### If is_valid() Fails

`is_valid()` returning `Err(_)` tells bb8 the connection is dead. bb8 drops the connection (no QUIT) and creates a new one by calling `connect()`. For `Pop3ConnectionManager`, `connect()` does full `tcp_connect + login`, so the next caller gets a freshly authenticated connection.

**With `retry_connection(false)`** (recommended for POP3): if `connect()` fails (auth error, network error), the error propagates immediately to the caller rather than looping until `connection_timeout`.

#### If has_broken() Returns true

`has_broken()` is called before checkout (after `is_valid()`) and also when the connection is returned to the pool. If `true`:
- The connection is dropped (no QUIT).
- A replacement connection is created via `connect()`.
- This is the primary mechanism for detecting TCP half-open states and stale connections without sending a probe command.

For `Pop3ConnectionManager`, `has_broken()` calls `conn.is_closed()` — a synchronous flag check on the connection's internal state (set to `true` after a server `-ERR` on connection close, after `quit()` is called, or after an I/O error marks the stream as dead).

#### If connect() Fails

With `retry_connection(false)` (the recommended setting for POP3):
- Authentication failures (`Pop3Error::AuthFailed`) propagate immediately to the caller.
- Network failures (`Pop3Error::Io`) propagate immediately.
- `pool.get()` returns `RunError::User(Pop3Error::AuthFailed)` immediately (not after 30s timeout).

With `retry_connection(true)` (bb8 default — do NOT use for POP3):
- bb8 retries `connect()` in a loop until `connection_timeout` (30s) elapses.
- Auth failures cause a 30-second hang before the caller sees an error.

#### Connection State Summary Table

| Scenario | bb8 Action | QUIT Sent? | POP3 Lock Released? |
|----------|-----------|-----------|---------------------|
| `is_valid()` fails (NOOP error) | Drop connection, create new | No | Server detects TCP close, releases lock |
| `has_broken()` = true on checkout | Drop connection, create new | No | Server detects TCP close |
| `has_broken()` = true on return | Drop connection | No | Server detects TCP close |
| Connection returned normally | Put back in idle queue | No (connection reused) | Lock retained (expected) |
| Idle timeout fires (reaper) | Drop connection | No | Server detects TCP close |
| `pop3Pool::shutdown()` | Check out, send QUIT, drop | Yes | Server releases lock cleanly |
| `pool.get()` fails with `is_valid()` error, `retry_connection(false)` | Error propagates immediately | No | TCP close releases lock |

---

### 5. Graceful Shutdown with Tokio

#### What Happens to bb8's Reaper When the Runtime Shuts Down

When `tokio::Runtime` is dropped (or `shutdown_timeout()` is called), spawned tasks are cancelled at their next `.await` yield point. The reaper's run loop is:

```rust
loop {
    let _ = self.interval.tick().await;  // ← task is cancelled here
    ...
}
```

When the runtime drops, the `interval.tick().await` future is cancelled. The reaper task ends. Because it holds only a `Weak<SharedPool<M>>` (not a strong reference), cancelling the reaper does not prevent the pool from being freed. The pool's `Arc<SharedPool<M>>` is held by the `Pool<M>` struct itself, which is freed through normal Rust drop order as the application stack unwinds.

**Important:** If the reaper is cancelled mid-tick (between `tick()` returning and `reap()` completing), no connections are half-reaped. The `reap()` function is synchronous and does not yield — it runs to completion atomically.

#### Runtime Drop Behavior

From Tokio official docs:
- `Runtime::drop()` waits indefinitely for all spawned work to stop.
- Tasks spawned via `tokio::spawn` keep running until they yield, then are dropped.
- The reaper's `interval.tick().await` is a yield point — on runtime drop, the reaper is cancelled there and exits.

**Risk: Runtime drops before all pool checkouts are completed**

If a caller holds a `PooledConnection` and the runtime is shut down:
- The `PooledConnection`'s Drop impl calls `put_back()`, which may try to return the connection to the pool's internal queue.
- If the pool's inner `SharedPool` is already freed (all `Arc` references gone), this is safe — `put_back()` finds the pool gone and just drops the connection.
- If `put_back()` itself tries to spawn a replenishment task (via `spawn_replenishing_approvals`), and the runtime is gone, `tokio::spawn` will panic with "no runtime available".

**Mitigation:** Ensure all `PooledConnection` guards are dropped **before** dropping the `Pop3Pool` or the Tokio runtime. The standard application pattern is:

```rust
// Correct shutdown order:
// 1. Stop accepting new work
// 2. Wait for in-flight requests (which hold checkout guards) to complete
// 3. Call pool.shutdown() (sends QUIT to idle connections)
// 4. Drop Pop3Pool
// 5. Runtime exits (via #[tokio::main] return or explicit shutdown)
```

#### Tokio Signal Handler Pattern

For production applications, the recommended pattern:

```rust
// Source: tokio::signal + Pop3Pool shutdown pattern
#[tokio::main]
async fn main() {
    let pool = Arc::new(Pop3Pool::new(config));

    // ... application logic using pool ...

    // Ctrl+C or SIGTERM handler
    tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");

    // Graceful shutdown: send QUIT to all idle connections
    pool.shutdown().await;

    // Runtime exits cleanly — reaper task is cancelled, all idle connections
    // already had QUIT sent, checked-out connections are abandoned (no QUIT).
}
```

**Does bb8 spawn any tasks that need explicit cancellation?**

bb8 spawns exactly **one** background task per inner pool: the reaper. It is self-terminating via the `Weak` reference pattern and requires no explicit cancellation. It also spawns short-lived replenishment tasks (`spawn_replenishing_approvals`) during pool maintenance — these are transient and complete quickly.

No manual task handle management is needed for bb8.

#### Risk of Panics if Runtime Drops Before Pool

The risk is in `put_back()` calling `tokio::spawn` when the runtime no longer exists. This panic occurs only if:
1. A `PooledConnection` is held past the runtime shutdown, AND
2. Returning that connection triggers a replenishment spawn.

**For `max_size(1)` per-account pools:** Replenishment is never triggered by `put_back()` — the pool is already at capacity (1). No `tokio::spawn` is called during `put_back()` for a full pool. This means the panic risk is **eliminated** for the per-account-singleton pattern used in `Pop3Pool`. This is a subtle but important safety advantage of the `max_size(1)` design.

---

### 6. POP3-Specific Lifecycle

#### RFC 1939 Session State and QUIT

POP3 has three session states:
- **AUTHORIZATION**: Before authentication. Lock not yet held.
- **TRANSACTION**: After authentication. Server holds exclusive maildrop lock. DELE marks accumulate.
- **UPDATE**: Entered only via QUIT. Server commits all DELEs and releases the lock.

```
TCP connect
     │
     ▼
AUTHORIZATION state
  USER / PASS → +OK
     │
     ▼
TRANSACTION state  ←─── Pool connections live here
  DELE 1, DELE 2...
  NOOP (health check)
     │
     ├── QUIT → UPDATE state → server deletes marked messages → lock released → TCP close
     │
     └── TCP close without QUIT → session aborts → NO messages deleted → lock released by server
```

**The pool holds connections in TRANSACTION state.** The maildrop lock is held the entire time a connection is idle in the pool. This is unavoidable — POP3 does not have a "pause session" command. This is why `max_size(1)` is mandatory: two connections to the same mailbox would fight over the lock.

#### TCP Close Without QUIT — Correct for Error Scenarios

RFC 1939 explicitly requires that abrupt session termination (no QUIT) must NOT enter UPDATE state:

> "If a session terminates for some reason other than a client-issued QUIT command, the POP3 session does NOT enter the UPDATE state and MUST not remove any messages from the maildrop."

This means bb8's default "just drop the connection" behavior (no QUIT) for error scenarios is:
- **Correct** for `is_valid()` failures — the connection was dead anyway; no DELE marks were applied.
- **Correct** for `has_broken()` = true — same reasoning.
- **Correct** for reaper expiry — idle connections have no pending DELE marks from the pool's perspective (callers using DELE would have quit and released before returning the connection, or the session-reset semantics from Phase 7 apply).

The only scenario where QUIT is **important** is intentional, controlled shutdown where the caller has issued DELE commands through the pooled connection and wants those deletions committed.

#### Should is_valid() Failure Trigger QUIT Before Drop?

**No.** If `is_valid()` (NOOP) fails, the connection is already broken — either the TCP stream is dead or the server returned an error. Attempting QUIT on a broken connection would also fail, likely with an I/O error. The correct action is to drop the connection immediately. This:
- Avoids a second failed I/O attempt.
- Is correct per RFC 1939 (no UPDATE state, no accidental deletions).
- Allows bb8 to proceed immediately to creating a replacement connection.

**Prescriptive decision:** Do not send QUIT in `is_valid()` error paths or `has_broken()` = true paths. Only send QUIT in `Pop3Pool::shutdown()` for intentional application exit, and expose `quit()` in the checked-out connection for callers who want to explicitly commit DELEs before returning the connection.

#### Connection Lifecycle Diagram

```
pop3Pool.get(account)
      │
      ▼
bb8 allocates connection slot (max_size=1, blocks if occupied)
      │
      ▼
is_valid() check (NOOP command):
  +OK → healthy, proceed
  Err → drop TCP (no QUIT), call connect() to replace
      │
      ▼
PooledConnection returned to caller
      │
    [caller uses: stat(), list(), retr(), dele(), etc.]
      │
      ├─── Normal drop (caller done)
      │         → put_back() → connection returned to idle queue
      │         → idle_start = Instant::now()
      │         → will be reaped after idle_timeout (5 min)
      │         → reaper calls retain() → connection dropped (no QUIT)
      │
      ├─── Caller calls conn.quit() explicitly, then drops guard
      │         → QUIT sent → server commits DELEs → TCP close
      │         → put_back() sees broken connection → discards
      │         → next get() calls connect() to create fresh session
      │
      ├─── I/O error during caller's operation
      │         → error returned to caller
      │         → caller drops guard
      │         → put_back() → has_broken()=true → discard (no QUIT)
      │         → next get() calls connect()
      │
      └─── Pop3Pool::shutdown() called
                → pool.get(account) (brief timeout)
                → conn.quit() sent (if connection available)
                → drop guard → DashMap entry removed → Arc freed
                → Pool dropped → reaper self-terminates via Weak
```

#### Lock Retention During Pool Idle Time

**Important operational consequence:** A `Pop3Client` checked into a pool and sitting idle holds the POP3 maildrop lock on the server. This means:
- No other mail client (Thunderbird, Outlook, mobile app) can access that mailbox while the connection is pooled.
- The `idle_timeout = 5 minutes` setting limits this lock retention to 5 minutes of idle time.
- Applications that do not need continuous access should call `conn.quit()` before releasing the checkout guard. This sends QUIT, releases the lock, and causes the pool to create a fresh connection on next checkout.

**Document this prominently in rustdoc on `Pop3Pool`.** POP3's exclusive lock model makes connection pooling more impactful than with stateless protocols (HTTP, Redis).

---

### Supplement Sources

#### Primary (HIGH confidence — source code verified)

- [bb8 inner.rs source](https://github.com/djc/bb8/blob/main/bb8/src/inner.rs) — `PoolInner::new()` reaper spawn with `Weak<SharedPool>`, `Reaper::run()` self-termination logic, `reaper_rate` interval
- [bb8 internals.rs source](https://github.com/djc/bb8/blob/main/bb8/src/internals.rs) — `reap()` function: `retain()` drops expired connections; no QUIT sent; idle_timeout and max_lifetime comparisons
- [bb8 api.rs source](https://github.com/djc/bb8/blob/main/bb8/src/api.rs) — `PooledConnection::drop()`: `put_back()` called with connection state; `ConnectionState::Extracted` bypass
- [bb8 issue #157: Reaper default frequency conflict](https://github.com/djc/bb8/issues/157) — reaper_rate default is 30 seconds; timing conflict with short idle_timeout values
- [bb8 Builder docs](https://docs.rs/bb8/latest/bb8/struct.Builder.html) — `idle_timeout` default 10 min, `max_lifetime` default 30 min, `test_on_check_out` default true, `reaper_rate` used by tests
- [Tokio Runtime::drop docs](https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html) — "waits forever"; `shutdown_timeout()` for bounded wait; tasks cancelled at next yield point
- [RFC 1939 §8](https://www.rfc-editor.org/rfc/rfc1939) — autologout timer minimum 10 minutes; QUIT required for UPDATE state; abrupt close MUST NOT delete messages

#### Secondary (MEDIUM confidence)

- [bb8 issue #123: Reaper violates min_idle](https://github.com/djc/bb8/issues/123) — reaper behavior with min_idle; `reap_all_idle_connections` option added
- [Dovecot POP3 timeouts](https://doc.dovecot.org/2.3/admin_manual/timeouts/) — POP3 client idle timeout 10 minutes; matches RFC 1939 minimum
- [Tokio task docs](https://docs.rs/tokio/latest/tokio/task/) — spawned task cancellation at yield points on runtime shutdown
