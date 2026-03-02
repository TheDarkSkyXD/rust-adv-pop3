# Phase 8: Connection Pooling — Race Conditions and Concurrency Pitfalls

**Researched:** 2026-03-01
**Domain:** DashMap entry API atomicity, bb8 `build()` connection behavior, async double-checked locking, RFC 1939 race implications
**Confidence:** HIGH (DashMap source verified; bb8 internals.rs and api.rs verified; tokio RwLock docs verified)
**Supersedes:** None — this supplements `08-RESEARCH.md` with deeper concurrency analysis

---

## Summary

The `get_or_create` pattern in `08-RESEARCH.md` contains a latent race condition that is safe in practice for DashMap (due to `or_insert_with` atomicity at the shard level) but problematic for `tokio::sync::RwLock<HashMap>` (no atomic check-then-insert). The bb8 `build()` behavior when `min_idle` is `None` is fully confirmed: **no connection is made at build time**. DashMap's `or_insert_with` closure **executes under the shard write lock**, which means the closure must never `.await` — doing so holds a blocking parking_lot lock across an await point, which will deadlock the Tokio runtime. The correct fix is to use `build_unchecked()` instead of `build()` for the DashMap pattern, or adopt a proper double-checked locking pattern with `tokio::sync::RwLock<HashMap>`.

---

## 1. Race Condition Analysis: `get_or_create` with DashMap

### What the Current Pattern Does

```rust
// From 08-RESEARCH.md (PROBLEMATIC as written)
async fn get_or_create(&self, account: &AccountKey) -> Result<Arc<bb8::Pool<...>>> {
    // Check-1: fast path
    if let Some(entry) = self.pools.get(account) {
        return Ok(Arc::clone(&*entry));
    }
    // WINDOW: both Task A and Task B can pass this check simultaneously
    let new_pool = Arc::new(
        bb8::Pool::builder()
            .max_size(1)
            .build(manager)      // <-- async, takes time
            .await?
    );
    // Insert: DashMap entry API
    self.pools.entry(account.clone())
        .or_insert_with(|| Arc::clone(&new_pool));
    Ok(new_pool)
}
```

### The Specific Race: Two Tasks, Same New Account

When Task A and Task B both call `get_or_create` for a new account at the same time:

1. Task A calls `self.pools.get(account)` — returns `None`.
2. Task B calls `self.pools.get(account)` — also returns `None` (A has not inserted yet).
3. Task A calls `bb8::Pool::builder()...build(manager).await` — starts building pool #1.
4. Task B calls `bb8::Pool::builder()...build(manager).await` — starts building pool #2.
5. Both pools complete. Task A calls `self.pools.entry(account).or_insert_with(...)` — inserts pool #1.
6. Task B calls `self.pools.entry(account).or_insert_with(...)` — entry is now occupied, closure is **not called**, pool #2 is dropped.
7. Both tasks return their respective `Arc<Pool>` — but Task A's Arc and Task B's Arc point to **different pool objects**.

**Step 7 is the problem.** Task B returns its `new_pool` Arc (pool #2), which was dropped from the map immediately at step 6. Task B's callers hold a reference to a pool that is no longer the canonical pool for this account. If Task B's caller now calls `pool.get().await`, they get a connection from the abandoned pool #2, not the registered pool #1.

**Consequence:** Two distinct `bb8::Pool` objects exist for the same account simultaneously. Both have `max_size(1)`. If both have active connections, two simultaneous TCP connections to the same mailbox exist, violating RFC 1939.

### Does bb8 `build()` with `min_idle = None` Attempt a Connection?

**No.** Verified against bb8 `internals.rs`:

```rust
// bb8 internals.rs — the wanted() method
pub(crate) fn wanted(&mut self, config: &Builder<M>) -> ApprovalIter {
    let available = self.conns.len() as u32 + self.pending_conns;
    let min_idle = config.min_idle.unwrap_or(0);  // None → 0
    let wanted = min_idle.saturating_sub(available);  // 0 - 0 = 0
    self.approvals(config, wanted)  // approvals for 0 connections
}
```

And from `api.rs`:

```
// build() calls start_connections(), which calls:
pub(crate) async fn start_connections(&self) -> Result<(), M::Error> {
    let wanted = self.inner.internals.lock().wanted(&self.inner.statics);
    // wanted = 0 when min_idle is None
    let mut stream = self.replenish_idle_connections(wanted);
    while let Some(result) = stream.next().await {
        result?;
    }
    Ok(())
}
```

When `min_idle` is `None`, `wanted()` returns 0, `start_connections()` iterates zero times, and **no TCP connection is attempted**. `build()` with `min_idle = None` is equivalent in effect to `build_unchecked()`: neither makes a network call.

**Conclusion for the race:** In the race scenario, both Task A and Task B do call `build()`, but since `min_idle` is `None`, neither actually opens a TCP connection. The duplicate pools are created in-memory only. This means the race does **not** cause an RFC 1939 violation in the common `min_idle = None` configuration. However, the abandoned pool object in Task B's Arc is still a correctness bug: subsequent calls through that Arc create a connection from a pool that is unknown to the registry. If another task later calls `get_or_create` for the same account, it gets pool #1; but Task B's caller still has pool #2. Two separate connections can now coexist for one account.

**If `min_idle` is set to `Some(1)` (or any non-zero value), the race becomes an RFC 1939 violation.** Both tasks will call `connect()` during `build()`, and both connections will authenticate to the same mailbox. The second connection will receive `-ERR maildrop already locked` — which bb8 will interpret as a connection error and retry (or fail with timeout if `retry_connection(false)`).

### The Fix: Return the Inserted Pool, Not the Local Pool

The fix is to return the pool that was actually inserted into the map, not the locally constructed one:

```rust
async fn get_or_create(
    &self,
    account: &AccountKey,
) -> Result<Arc<bb8::Pool<Pop3ConnectionManager>>, Pop3PoolError> {
    // Fast path: account already has a pool
    if let Some(existing) = self.pools.get(account) {
        return Ok(Arc::clone(&*existing));
        // DashMap Ref (shard read lock) dropped here — before any .await
    }

    // Slow path: build a new pool (no map lock held during async build)
    let manager = Pop3ConnectionManager::from_account(account.clone());
    let candidate = Arc::new(
        // Use build_unchecked() — no initial connection attempt,
        // avoids the blocking start_connections() call entirely,
        // and is honest: we are deferring to first checkout.
        bb8::Pool::builder()
            .max_size(1)
            .min_idle(None)              // confirmed: no connection at build time
            .retry_connection(false)
            .test_on_check_out(true)
            .build_unchecked(manager),   // sync, no .await — no RFC 1939 risk
    );

    // Insert only if absent; get back whatever is canonical
    let canonical = self.pools
        .entry(account.clone())
        .or_insert_with(|| Arc::clone(&candidate));
    //   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    // DashMap acquires shard write lock here.
    // If entry is Vacant, closure runs → inserts candidate clone.
    // If entry is Occupied (a racing task won), closure is NOT called.
    // In both cases, canonical is a Ref<Arc<Pool>> to what's in the map.

    Ok(Arc::clone(&*canonical))
    // Shard write lock (via canonical Ref) released when canonical drops here.
    // We return a clone of what's actually in the map — correct in all cases.
}
```

**Key change:** The function returns `Arc::clone(&*canonical)` — the pool that is actually registered — not `new_pool`. Whether this task won or lost the race, the caller always gets the canonical pool for this account.

**Critical constraint:** The closure passed to `or_insert_with` **must not contain any async code**. It executes under the DashMap shard write lock (a parking_lot `RwLockWriteGuard`). Holding that lock across an `.await` point will starve other tasks trying to access the same shard and deadlock the Tokio runtime. The use of `build_unchecked()` (synchronous) instead of `build().await` (async) inside the closure is mandatory.

---

## 2. DashMap Shard Lock Behavior

### Internal Structure

DashMap stores data as:
```rust
shards: Box<[CachePadded<RwLock<HashMap<K, V>>>]>
```

Each shard is a `parking_lot::RwLock`. The number of shards defaults to the number of logical CPU cores times 4 (minimum 1, always a power of two). Two keys that hash to the same shard share a lock.

### Entry API Lock Guarantee

`DashMap::entry(key)` acquires a **shard write lock** and holds it for the lifetime of the returned `Entry`. Both `OccupiedEntry` and `VacantEntry` hold an `RwLockWriteGuardDetached`. The `or_insert_with` implementation is:

```rust
pub fn or_insert_with(self, value: impl FnOnce() -> V) -> RefMut<'a, K, V> {
    match self {
        Entry::Occupied(entry) => entry.into_ref(),
        Entry::Vacant(entry) => entry.insert(value()),  // closure called HERE, under write lock
    }
}
```

The closure `value()` is called **inside the `insert()` call, while the write lock is held**. This is atomic at the shard level: no other task can insert the same key between the check and the insert.

**Atomicity guarantee:** For a given shard, `entry().or_insert_with()` is an atomic check-then-insert. Two tasks racing on the same key cannot both succeed at inserting — one will see `Occupied` and skip the closure.

**`or_insert` vs `or_insert_with` atomicity:** Identical. Both execute the value construction while holding the write lock. The only difference is that `or_insert` evaluates its argument eagerly before the entry lookup (the value is computed regardless of whether the key exists), while `or_insert_with` defers evaluation to inside the write-locked section (the closure runs only if the entry is vacant). For a cheap `Arc::clone`, the difference is irrelevant. For `build().await`, `or_insert_with` would defer evaluation — but that `await` cannot happen under the lock.

### Can DashMap Deadlock from Same-Shard Collisions?

Yes, but only through specific misuse patterns:

**Pattern A (self-deadlock):** A single task holds a `Ref` or `RefMut` to the map and then attempts another map operation. The shard is already write-locked; re-entering the same lock deadlocks. This is documented in DashMap issue #74.

**Pattern B (async deadlock):** A task holds a `Ref`, `RefMut`, or `Entry` and then yields to the async runtime via `.await`. Other tasks waiting to acquire the same shard lock will spin (parking_lot spin-locks do not cooperate with the Tokio scheduler), blocking their worker threads. If all worker threads are blocked, the runtime deadlocks. This affects all versions of DashMap. The fix in v4+ is improved, but the fundamental rule remains: **never `.await` while holding a DashMap reference**.

**Pattern C (probabilistic race):** Two tasks call `get_mut(a)` and `get_mut(b)` where `a` and `b` hash to the same shard. If both tasks hold their guards simultaneously, neither can proceed. The probability is `1/shard_count` per pair. Documented in DashMap issue #74.

**For our `get_or_create` pattern:** Pattern C does not apply because we release the `Ref` from the fast path before the slow path begins, and the `entry()` call in the slow path is a single uninterrupted operation. Pattern A does not apply because we do not re-enter the map while holding a reference. Pattern B applies only if we put `.await` inside the `or_insert_with` closure — which `build_unchecked()` prevents.

### Starvation Under Write-Heavy Workloads

`tokio::sync::RwLock` is write-preferring (FIFO queue). parking_lot's `RwLock` (used by DashMap) is also write-preferring by default. Under heavy write contention, readers queue behind pending writers. For the connection pool registry, writes only happen at initial pool creation (once per account), so write contention is bounded and not a concern in practice.

---

## 3. TOCTOU with `tokio::sync::RwLock<HashMap>` Alternative

### Why RwLock Doesn't Have DashMap's Lock Problem

`tokio::sync::RwLock` is async-aware. Holding a `RwLockReadGuard` or `RwLockWriteGuard` across an `.await` is safe in the sense that it does not block a thread — the guard implements `Send` (when `T: Send`) and other tasks can be scheduled. However, it serializes all access to the protected data through the lock, which is a performance concern, not a correctness one, for our workload.

### There Is No Lock Upgrade in tokio RwLock

`tokio::sync::RwLock` does not provide a read-to-write upgrade operation. You cannot atomically upgrade from a read guard to a write guard. The correct pattern requires dropping the read guard and re-acquiring a write guard, with a second check inside the write lock (double-checked locking).

### Correct Double-Checked Locking Pattern

```rust
async fn get_or_create(
    &self,
    account: &AccountKey,
) -> Result<Arc<bb8::Pool<Pop3ConnectionManager>>, Pop3PoolError> {
    // Check-1: read lock (allows concurrent readers)
    {
        let guard = self.pools.read().await;
        if let Some(pool) = guard.get(account) {
            return Ok(Arc::clone(pool));
        }
        // guard dropped here — read lock released
    }

    // Slow path: build candidate outside any lock
    let manager = Pop3ConnectionManager::from_account(account.clone());
    let candidate = Arc::new(
        bb8::Pool::builder()
            .max_size(1)
            .min_idle(None)
            .retry_connection(false)
            .test_on_check_out(true)
            .build_unchecked(manager),
    );

    // Check-2: write lock — mandatory second check
    let mut guard = self.pools.write().await;
    if let Some(existing) = guard.get(account) {
        // A racing task inserted while we built our candidate.
        // Discard candidate; return what's actually in the map.
        return Ok(Arc::clone(existing));
    }
    // We won the race — insert and return
    let pool = Arc::clone(&candidate);
    guard.insert(account.clone(), candidate);
    Ok(pool)
}
```

**Why the second check is mandatory:** Between dropping the read lock (end of the first block) and acquiring the write lock, another task may have inserted the pool. Without the second check, two pools get inserted for the same account. The second check inside the write lock is the atomic gate — only one task can hold the write lock, so at most one task inserts.

**Why `build_unchecked()` is used here too:** If you used `build().await` between Check-1 and Check-2, you could have multiple tasks building pools simultaneously (the same race as with DashMap). This is acceptable — the wasted work is bounded. But since `min_idle = None` means `build()` makes no network call anyway, `build_unchecked()` is preferred: it is synchronous (no await, no scheduling delay), and it avoids any potential blocking in `start_connections()`.

**Note on `build()` vs `build_unchecked()` in the write lock:** You cannot call `build().await` while holding a write lock on a `tokio::sync::RwLock`. Tokio's RwLock allows holding a guard across `.await`, but holding an exclusive write lock during a long-running async operation (like a network connection attempt) serializes all other map access for the duration. With `build_unchecked()`, the write lock is held only for the fast synchronous insert — correct.

### The Full Struct with RwLock

```rust
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Pop3Pool {
    pools: RwLock<HashMap<AccountKey, Arc<bb8::Pool<Pop3ConnectionManager>>>>,
    // No outer Arc needed if Pop3Pool itself is Arc-wrapped by the caller
}
```

---

## 4. bb8 Internal Concurrency and `build()` Behavior

### Confirmed: `build()` with `min_idle = None` Makes No Connection

From bb8 source (verified):

| Call | `min_idle` | Effect |
|------|------------|--------|
| `build().await` | `None` | `wanted() = 0`, `start_connections()` iterates zero times, **no TCP connection** |
| `build().await` | `Some(0)` | same as `None` — `0.saturating_sub(0) = 0`, **no TCP connection** |
| `build().await` | `Some(1)` | `wanted() = 1`, calls `connect()` once, **one TCP connection attempted** |
| `build_unchecked()` | any | spawns `spawn_start_connections()` in background, returns immediately |

**Decision: Use `build_unchecked()` for lazy pool creation.** When a pool is created speculatively (as happens in the race), there is no wasted connection attempt. The first actual connection occurs when a caller calls `pool.get().await`. This is the correct lazy model for our use case — we do not want to connect until a caller requests a connection.

### What Happens When Two Tasks Both Call `build_unchecked()`?

Each call creates an independent `bb8::Pool` struct wrapping an `Arc<SharedPool<M>>`. Both pools spawn their own background reaper tasks (if `max_lifetime` or `idle_timeout` is set). If both pools end up registered (i.e., neither is abandoned), two reaper tasks exist for the same account. This is a resource leak.

With the corrected pattern (returning the canonical pool and discarding the losing candidate), the losing pool's `Arc` is dropped. If no other references exist, the pool is dropped. The reaper task, if any, holds a `Weak` reference to the `SharedPool` and will observe the pool as gone, stopping. So **abandoned pools from losing the race do not leak reaper tasks**, provided no external code holds a reference to them.

For zero-overhead pool creation (no reaper task spawned at all), configure without lifetime/idle timeout on the builder. Reaper tasks only spawn if `max_lifetime.is_some() || idle_timeout.is_some()`.

### bb8's Internal Connection Creation: When Does `connect()` Actually Run?

1. `build_unchecked()` or `build().await` with `min_idle = None`: connection deferred to first `pool.get().await`.
2. First `pool.get().await`: bb8 checks the pool (empty), calls `ManageConnection::connect()`, awaits it, and returns the connection.
3. Connection drop (RAII): connection returned to pool's idle list.
4. Second `pool.get().await`: pool checks idle list (non-empty), optionally runs `is_valid()` via NOOP, returns connection.

There is no concurrent connection creation within a `max_size(1)` pool: bb8 uses an internal permit-based semaphore. With `max_size(1)`, exactly one permit exists. Any `pool.get()` call while the one connection is checked out waits for the permit to be returned.

---

## 5. Definitive Recommendations

### 5.1 Use `build_unchecked()` in the Registry Pattern

Use `build_unchecked()` for pool construction inside `get_or_create`. It is synchronous, prevents any RFC 1939 risk from duplicate `build()` calls, and is semantically honest (defer connection to first checkout).

### 5.2 Return the Canonical Pool, Not the Local Pool

Always return `Arc::clone(&*canonical)` where `canonical` is obtained from `self.pools.entry(...).or_insert_with(...)`. Never return the locally-built `candidate` directly. This ensures all callers, whether they won or lost the creation race, share the same pool object.

### 5.3 Never `.await` Under a DashMap Reference

The DashMap `or_insert_with` closure runs under the shard write lock. The closure must be synchronous. `build_unchecked()` satisfies this. `build().await` does not — do not use `build().await` inside the closure.

### 5.4 If Using `tokio::sync::RwLock<HashMap>`, Apply Double-Checked Locking

Use the three-phase pattern: read-check (drop guard), build candidate, write-check (second check inside write lock, return winner). The second check prevents double-insertion by racing tasks. Return `Arc::clone(existing)` if the second check finds an entry, not your candidate.

### 5.5 Set `min_idle(None)` (the default)

Never set `min_idle` to `Some(N)` where `N >= 1` in a registry pattern. Setting a non-zero `min_idle` causes `build().await` to attempt actual connections, which in the race scenario causes multiple simultaneous TCP connections to the same mailbox, violating RFC 1939.

---

## 6. Complete Corrected Implementation

### With DashMap

```rust
use dashmap::DashMap;
use std::sync::Arc;

pub struct Pop3Pool {
    pools: DashMap<AccountKey, Arc<bb8::Pool<Pop3ConnectionManager>>>,
    config: PoolConfig,
}

impl Pop3Pool {
    pub async fn get(
        &self,
        account: &AccountKey,
    ) -> Result<bb8::PooledConnection<'_, Pop3ConnectionManager>, Pop3PoolError> {
        let pool = self.get_or_create(account)?;
        pool.get().await.map_err(Pop3PoolError::from)
    }

    // Note: NOT async — build_unchecked() is synchronous
    fn get_or_create(
        &self,
        account: &AccountKey,
    ) -> Result<Arc<bb8::Pool<Pop3ConnectionManager>>, Pop3PoolError> {
        // Fast path: brief read lock, released before return
        if let Some(existing) = self.pools.get(account) {
            return Ok(Arc::clone(&*existing));
            // DashMap Ref (shard read lock) dropped here
        }

        // Slow path: build candidate entirely outside the map lock
        let manager = Pop3ConnectionManager::from_account(account.clone());
        let candidate = Arc::new(
            bb8::Pool::builder()
                .max_size(1)               // RFC 1939: one connection per mailbox
                .min_idle(None)            // no initial connection — lazy
                .retry_connection(false)   // auth failures propagate immediately
                .test_on_check_out(true)   // NOOP health check on checkout
                .connection_timeout(std::time::Duration::from_secs(30))
                .build_unchecked(manager), // synchronous — safe under shard lock
        );

        // Atomic check-then-insert under shard write lock.
        // or_insert_with closure runs ONLY if entry is Vacant (synchronous Arc::clone is safe).
        // Returns a RefMut pointing to whatever is canonical in the map.
        let canonical = self.pools
            .entry(account.clone())
            .or_insert_with(|| Arc::clone(&candidate));

        // Clone the canonical Arc before the RefMut (shard write lock) drops
        let result = Arc::clone(&*canonical);
        drop(canonical); // explicit drop — shard write lock released here

        Ok(result)
    }
}
```

### With `tokio::sync::RwLock<HashMap>`

```rust
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Pop3Pool {
    pools: RwLock<HashMap<AccountKey, Arc<bb8::Pool<Pop3ConnectionManager>>>>,
    config: PoolConfig,
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
        // Phase 1: read lock — multiple tasks can pass simultaneously
        {
            let guard = self.pools.read().await;
            if let Some(pool) = guard.get(account) {
                return Ok(Arc::clone(pool));
            }
            // guard drops here — read lock released before slow path
        }

        // Phase 2: build candidate outside any lock
        // build_unchecked() is synchronous — no await, no RFC 1939 risk
        let manager = Pop3ConnectionManager::from_account(account.clone());
        let candidate = Arc::new(
            bb8::Pool::builder()
                .max_size(1)
                .min_idle(None)
                .retry_connection(false)
                .test_on_check_out(true)
                .connection_timeout(std::time::Duration::from_secs(30))
                .build_unchecked(manager),
        );

        // Phase 3: write lock — serialize final insertion
        let mut guard = self.pools.write().await;

        // Second check: a racing task may have inserted while we built candidate
        if let Some(existing) = guard.get(account) {
            // We lost the race — discard candidate, return canonical pool
            // (candidate Arc is dropped here — no connection was made, no leak)
            return Ok(Arc::clone(existing));
        }

        // We won the race — insert candidate as canonical
        let pool = Arc::clone(&candidate);
        guard.insert(account.clone(), candidate);
        Ok(pool)
        // guard drops here — write lock released
    }
}
```

---

## 7. Comparison: DashMap vs. RwLock for This Pattern

| Property | DashMap | `tokio::sync::RwLock<HashMap>` |
|----------|---------|-------------------------------|
| Race correctness (with fixes) | Correct — `or_insert_with` is atomic per shard | Correct — double-checked locking with write-lock second check |
| `await` across lock | Forbidden (parking_lot lock, not async-aware) | Allowed (tokio-aware, yields instead of spinning) |
| `get_or_create` async | No (must use `build_unchecked`) | Yes (both `build_unchecked` and `build().await` possible in right positions) |
| Deadlock risk | Real if reference held across `await` | Lower — Tokio scheduler handles yielding |
| Dependency | Requires `dashmap` crate | Uses `tokio` (already a dependency) |
| Performance | Sharded — lower contention on read-heavy workloads | Single lock — all readers/writers compete |
| Code complexity | Simpler (no second check needed) | Slightly more code (three phases) |

**Decision: Use `tokio::sync::RwLock<HashMap>` as the default.** This avoids a new dependency (`dashmap` is not currently in `Cargo.toml`), is free from DashMap's async deadlock footgun, and the double-checked locking pattern is well-understood. At POP3 scale (tens of accounts, not thousands), the single lock's contention is not measurable.

If `dashmap` is added for performance reasons in the future, use the corrected DashMap pattern above. Do not use the pattern from the original `08-RESEARCH.md` directly (it returns the local pool, not the canonical pool).

---

## 8. Pitfall Additions (Supplement to `08-RESEARCH.md`)

### Pitfall 6: `or_insert_with` Closure Must Be Synchronous

**What goes wrong:** Placing `build(manager).await` inside the `or_insert_with` closure. This executes an async operation while the DashMap shard write lock (a parking_lot lock) is held. Tokio will attempt to yield to the scheduler at the `.await` point, but the parking_lot lock is not yielded. Other tasks waiting on the same shard spin on the lock, blocking their worker threads. With enough spinning tasks, all Tokio worker threads block, deadlocking the runtime.

**How to avoid:** Build the pool **before** calling `.entry().or_insert_with(...)`. Pass a pre-built `Arc::clone(&candidate)` as the closure body — a synchronous, trivially cheap operation.

**Warning signs:** Random hangs during new account first-access under concurrent load. The runtime appears frozen. `tokio::time::timeout` on `pool.get()` may or may not fire (depends on whether the timeout task itself is scheduled).

### Pitfall 7: Returning the Local Pool Instead of the Canonical Pool

**What goes wrong:** After `self.pools.entry(account).or_insert_with(|| Arc::clone(&new_pool))`, returning `new_pool` (the local Arc) instead of cloning from the map entry. If this task lost the race, `new_pool` is a different `Arc<Pool>` from what is in the map. Callers using this Arc get connections from an unregistered pool.

**How to avoid:** Return `Arc::clone(&*canonical)` where `canonical` is the `RefMut` returned by `or_insert_with`. This always points to what is actually in the map, regardless of race outcome.

**Warning signs:** Under concurrent load with new accounts, some callers' connections are not governed by the per-account `max_size(1)` limit. Two tasks can simultaneously check out connections for the same account (they each have a different pool object). RFC 1939 violations follow.

### Pitfall 8: Setting `min_idle(Some(1))` in a Registry Pattern

**What goes wrong:** `build().await` with `min_idle(Some(1))` causes an immediate `connect()` call. In the race scenario, both Task A and Task B call `connect()`, opening two TCP connections to the same POP3 server for the same account. The second connection receives `-ERR maildrop already locked`.

**How to avoid:** Use `min_idle(None)` (default) for all per-account pools in the registry. Connections are created lazily on first `pool.get()` call.

**Warning signs:** `-ERR maildrop already locked` errors appearing in logs immediately after the first connection to a new account, especially under concurrent access.

---

## Sources

### Primary (HIGH confidence — source code verified)

- [bb8 api.rs source](https://github.com/djc/bb8/blob/main/bb8/src/api.rs) — `build()` calls `start_connections()` which awaits; `build_unchecked()` calls `spawn_start_connections()` and returns synchronously
- [bb8 internals.rs source](https://github.com/djc/bb8/blob/main/bb8/src/inner.rs) — `wanted()` method: `min_idle.unwrap_or(0).saturating_sub(available)` — confirmed `None` produces 0 connections
- [DashMap entry.rs source](https://github.com/xacrimon/dashmap/blob/master/src/mapref/entry.rs) — `or_insert_with` closure called under `RwLockWriteGuardDetached`; both `or_insert` and `or_insert_with` hold the write lock during value construction
- [bb8 Builder docs](https://docs.rs/bb8/latest/bb8/struct.Builder.html) — `build()` doc: "will not be returned until it has established its configured minimum number of connections"; `build_unchecked()` doc: "does not wait for any connections to be established"
- [tokio RwLock docs](https://docs.rs/tokio/latest/tokio/sync/struct.RwLock.html) — no read-to-write upgrade; FIFO write-preferring fairness; `read().await` and `write().await` are async-aware
- [tokio OnceCell docs](https://docs.rs/tokio/latest/tokio/sync/struct.OnceCell.html) — `get_or_try_init` waits for concurrent initializer; only one closure executes — suitable for single-value init, not per-key HashMap

### Secondary (MEDIUM confidence — documented bugs and issues)

- [DashMap issue #74: single-threaded deadlock](https://github.com/xacrimon/dashmap/issues/74) — deadlock when holding `RefMut` and calling other map operations; probability `1/shard_count^2` for different-key deadlock
- [DashMap issue #79: async deadlock](https://github.com/xacrimon/dashmap/issues/79) — parking_lot spin-lock held across `.await` causes task starvation and runtime deadlock; v4+ improvements noted
- [pnpm/pacquet PR #200: DashMap → RwLock](https://github.com/pnpm/pacquet/pull/200) — real-world migration from DashMap to `RwLock<HashMap>` to fix async deadlock in Tokio context; comparable performance on benchmarks
- [RFC 1939 §8](https://www.rfc-editor.org/rfc/rfc1939.html) — exclusive-access lock on maildrop; `-ERR maildrop already locked` on second connection attempt
