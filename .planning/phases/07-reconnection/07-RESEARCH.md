# Phase 7: Reconnection - Research

**Researched:** 2026-03-01
**Domain:** Rust async retry/backoff, Decorator pattern, session-state signaling API design
**Confidence:** HIGH

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| RECON-01 | Client provides automatic reconnection with exponential backoff on connection drop | `backon` 1.6.0 `ExponentialBuilder` + `.when()` filters to I/O errors only; `ReconnectingClient` decorator wraps `Pop3ClientBuilder` and re-invokes `connect()` + `login()` on each retry |
| RECON-02 | Reconnection retries only on I/O errors — authentication failures propagate immediately | `.when(|e| matches!(e, Pop3Error::Io(_) \| Pop3Error::ConnectionClosed))` passes retryable errors; `Pop3Error::AuthFailed` is NOT matched so it propagates immediately through `?` |
| RECON-03 | Reconnection explicitly surfaces session-state loss (DELE marks are not preserved) to caller | Return type of every fallible method on `ReconnectingClient` changes to `Result<(T, SessionReset)>`, OR a `SessionReset` flag is embedded in a wrapper enum — callers cannot ignore it at compile time |
| RECON-04 | Backoff uses jitter to prevent thundering herd | `ExponentialBuilder::default().with_jitter()` enables full-jitter (uniform random in `0..current_delay`); backed by `fastrand` 2.x — no extra dependencies |
</phase_requirements>

---

## Summary

Phase 7 implements `ReconnectingClient`: a Decorator struct that wraps `Pop3ClientBuilder` + credentials and transparently reconnects when an I/O error is detected. The core retry machinery comes from `backon` 1.6.0, which the STATE.md roadmap decisions already selected as an unconditional dependency. No alternative retry crate needs to be evaluated.

The single most important design decision is **how to surface session-state loss** (RECON-03). RFC 1939 specifies that DELE marks are pending until QUIT commits them. A reconnect silently discards all pending DELEs. If the API says nothing, callers can issue DELEs, get an I/O drop, reconnect, and believe the DELEs were applied — a silent data-loss bug. The correct API pattern is to make session-state loss **structurally unignorable**: returning a `(T, SessionReset)` pair (or a `CommandResult<T>` enum) forces callers to handle the reset case at compile time. Comparable crates in the ecosystem (`reconnecting-jsonrpsee-ws-client`, `reconnecting-websocket`) both use a variant-per-event approach to avoid callers silently ignoring reconnects.

The error classification strategy (`backon` `.when()`) is straightforward: retry on `Pop3Error::Io(_)` and `Pop3Error::ConnectionClosed` (added in Phase 5), propagate all other variants immediately. `Pop3Error::AuthFailed` must **never** be retried — it indicates wrong credentials, and retrying would lock the account on many servers.

**Primary recommendation:** Implement `ReconnectingClient` as a new `src/reconnect.rs` file containing a struct that holds a `Pop3ClientBuilder` + `username: String` + `password: String`. Use `backon` 1.6.0 with `ExponentialBuilder::default().with_jitter()`. For RECON-03, add a `SessionReset` ZST (zero-sized type) and return `Result<(T, Option<SessionReset>)>` from all methods — `None` on the first call after connect, `Some(SessionReset)` any time a reconnect occurred before the call returned.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `backon` | 1.6.0 | Exponential backoff retry with jitter via `ExponentialBuilder` | Already in roadmap decisions (STATE.md: "backon 1.6 is unconditional dependency"); actively maintained (25 releases, 1k GitHub stars); uses `fastrand` 2 internally so no extra RNG dependency is added |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `fastrand` | 2 (transitive via backon) | Uniform random jitter for backoff delays | Pulled in automatically by `backon`; no direct dependency needed |
| `tokio_test::io::Builder` | 0.4 (already in dev-deps) | Mock I/O for testing reconnect logic | Same mock infrastructure as all existing tests |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `backon` | `tokio-retry` | `tokio-retry` is older (last release 2021), less actively maintained; lacks `.when()` for conditional error classification without boilerplate wrapper |
| `backon` | Hand-rolled retry loop | Custom loop misses edge cases: max elapsed time tracking, jitter implementation, proper future re-creation on each attempt; backon explicitly solves the "futures can only be polled once" problem via `FnMut() -> Fut` |
| `backon` | `tower::retry` | Tower's retry middleware is valid for Tower's Service abstraction; this project does not use Tower and adding it just for reconnect adds heavy transitive deps for no benefit |

**Installation:**
```toml
# In [dependencies] section of Cargo.toml
backon = "1.6"
```

No feature flag needed — `backon` 1.6 enables `tokio-sleep` by default, which is what `.sleep(tokio::time::sleep)` requires.

---

## Architecture Patterns

### Recommended Project Structure

```
src/
├── reconnect.rs    # New: ReconnectingClient struct, SessionReset ZST, impl block
├── client.rs       # Unchanged (except ConnectionClosed added in Phase 5)
├── error.rs        # Unchanged (ConnectionClosed added in Phase 5)
└── lib.rs          # Add: pub use reconnect::{ReconnectingClient, SessionReset}
```

No modifications to `Transport`, `response.rs`, or `types.rs` are needed. The decorator only uses the public API of `Pop3Client`.

### Pattern 1: Decorator Struct Holding Builder + Credentials

**What:** `ReconnectingClient` stores a `Pop3ClientBuilder` (from Phase 4/5, which derives `Clone`) plus `username: String` and `password: String`. On every method call, it delegates to its inner `Pop3Client`. On I/O error, it drops the dead client and calls `backon` to reconnect.

**When to use:** The canonical Decorator pattern in Rust — wrap a type, intercept calls, add behavior before/after delegation. This avoids modifying `Pop3Client` internals and keeps the two concerns (protocol + resilience) separated.

**Why hold credentials:** Re-authenticating after reconnect requires `login(username, password)`. Without storing credentials, the caller would have to supply them again on every reconnect — defeating the purpose of transparent reconnection.

**Why hold builder (not just addr/timeout):** `Pop3ClientBuilder` (Phase 4) encodes TLS mode, hostname, timeout, and other config. Cloning it re-creates the full original connection configuration. This is why Phase 5's "builder derives Clone" is a hard prerequisite.

```rust
// Source: Decorator pattern; derived from project architecture
// File: src/reconnect.rs

use crate::{Pop3Client, Pop3ClientBuilder, Pop3Error, Result};
use std::time::Duration;

/// Zero-sized type indicating that a reconnect occurred before this result was produced.
/// Callers MUST check for this to detect that pending DELE marks were discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionReset;

/// A wrapper around `Pop3Client` that automatically reconnects on I/O errors.
///
/// Session state (pending DELE marks) is NOT preserved across reconnects.
/// Every method returns `Option<SessionReset>` to make state loss structurally
/// unignorable at the call site.
pub struct ReconnectingClient {
    builder: Pop3ClientBuilder,
    username: String,
    password: String,
    client: Pop3Client,
}
```

### Pattern 2: backon Retry with Conditional Error Classification

**What:** Wrap the connect+login sequence in a closure, retry it with `ExponentialBuilder::default().with_jitter()`, and use `.when()` to restrict retries to I/O/connection errors only.

**When to use:** Every reconnect attempt — both initial connection and each retry after a drop.

**Key detail:** The closure must be `FnMut() -> Future` because backon re-creates a fresh future on each attempt. This is the core design insight from backon's author: wrapping a ready Future doesn't work because a completed Future's `poll` may panic on re-poll.

```rust
// Source: backon 1.6.0 README + docs.rs/backon/latest/backon/
// File: src/reconnect.rs

use backon::{ExponentialBuilder, Retryable};
use std::time::Duration;

async fn reconnect(
    builder: &Pop3ClientBuilder,
    username: &str,
    password: &str,
) -> Result<Pop3Client> {
    let connect = || async {
        let mut client = builder.connect().await?;
        client.login(username, password).await?;
        Ok(client)
    };

    connect
        .retry(
            ExponentialBuilder::default()
                .with_jitter()                           // full jitter: uniform rand in (0, delay)
                .with_min_delay(Duration::from_secs(1))  // 1s initial (backon default)
                .with_max_delay(Duration::from_secs(60)) // 60s cap (backon default)
                .with_max_times(5),                      // stop after 5 attempts
        )
        .sleep(tokio::time::sleep)
        .when(|e| is_retryable(e))
        .await
}

/// Only retry on transient connection/I/O failures.
/// Authentication failures MUST NOT be retried — they are permanent and
/// retrying risks account lockout on servers with brute-force protection.
fn is_retryable(e: &Pop3Error) -> bool {
    matches!(e, Pop3Error::Io(_) | Pop3Error::ConnectionClosed)
}
```

### Pattern 3: SessionReset Signal in Return Type

**What:** Wrap successful results in `(T, Option<SessionReset>)`. `None` means no reconnect occurred. `Some(SessionReset)` means a reconnect happened before the value was produced — callers know all DELE marks from before the drop are gone.

**When to use:** Every public method on `ReconnectingClient` that delegates to `Pop3Client`.

**Why not a callback:** A callback approach (`on_reconnect: Box<dyn Fn()>`) lets callers ignore it by passing a no-op. The return-type approach makes ignoring it a conscious choice (callers can `let _ = reset` but must see it). The compile-time visibility is the key property.

**Why not a separate notification channel:** A channel approach requires callers to poll a side channel. The tuple return is simpler and keeps the async method self-contained.

```rust
// Source: Inspired by reconnecting-websocket Event enum pattern (docs.rs/reconnecting-websocket)
// and reconnecting-jsonrpsee-ws-client state signaling
// File: src/reconnect.rs

impl ReconnectingClient {
    /// Execute a POP3 operation, reconnecting transparently on I/O error.
    ///
    /// Returns `(result, Some(SessionReset))` if a reconnect occurred — all
    /// pending DELE marks from the previous session have been discarded.
    /// Returns `(result, None)` if the existing connection was used successfully.
    async fn execute<T, F, Fut>(&mut self, op: F) -> Result<(T, Option<SessionReset>)>
    where
        F: Fn(&mut Pop3Client) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        match op(&mut self.client).await {
            Ok(value) => Ok((value, None)),
            Err(e) if is_retryable(&e) => {
                // Connection dropped — reconnect with backoff
                self.client = reconnect(&self.builder, &self.username, &self.password).await?;
                // Retry the operation once on the fresh connection
                let value = op(&mut self.client).await?;
                Ok((value, Some(SessionReset)))
            }
            Err(e) => Err(e), // AuthFailed, Parse, etc. propagate immediately
        }
    }
}
```

### Anti-Patterns to Avoid

- **Retrying on `Pop3Error::AuthFailed`:** Never retry authentication failures. Many servers implement account lockout after N consecutive failed attempts. The `.when()` closure must explicitly NOT match `AuthFailed`.
- **Storing an `Option<Pop3Client>` inside `ReconnectingClient`:** An `Option` field means every method must unwrap it, adding runtime panics or error returns for a logically impossible None state. Hold an initialized `Pop3Client` directly.
- **Silent reconnect with no caller signal:** A `ReconnectingClient::stat()` that returns `Result<Stat>` with no session-reset indication silently discards DELE information. REQUIREMENTS.md explicitly places "Transparent auto-reconnect with silent DELE re-issue" in the **Out of Scope** table.
- **Using `backon` without `.sleep(tokio::time::sleep)`:** The `backon` crate requires an explicit sleep implementation. Omitting it causes a compile error (`Sleeper` bound not satisfied). Always wire `tokio::time::sleep` for async contexts.
- **Unbounded retry attempts (`without_max_times()`):** Without a cap, a permanently-wrong password causes `backon` to retry forever. The `is_retryable` function stops this for auth errors, but an I/O-persistent failure (server down) would loop forever. Set `with_max_times(N)` — 5 is a reasonable production default.
- **Sharing `ReconnectingClient` across tasks (`Arc<Mutex<_>>`):** `Pop3Client` takes `&mut self` on all commands — it is already `!Sync`. `ReconnectingClient` inherits this. Do not attempt to share it across tasks. Document this in rustdoc.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Exponential delay calculation with jitter | Custom delay loop with `rand::thread_rng()` | `backon::ExponentialBuilder::default().with_jitter()` | Full-jitter implementation is subtle (uniform in `0..cap`, not `0..base * 2^n`); backon handles the cap correctly; `fastrand` is already a transitive dep |
| Future re-creation on each retry | Wrapping the output `Future` directly in a retry loop | `FnMut() -> Fut` closure passed to `backon` | A completed `Future` must not be polled again (may panic). backon's `FnMut() -> Fut` design creates a fresh future per attempt. Custom retry loops often get this wrong. |
| Max-elapsed-time tracking | Storing `Instant::now()` and checking elapsed in loop | `ExponentialBuilder::with_total_delay()` | Edge cases around clock skew and overflow; backon handles it |
| Jitter RNG seeding | `thread_rng()` initialization | `backon` uses `fastrand` with automatic seed | `fastrand` is a smaller, fast RNG appropriate for backoff jitter; no need to pull in `rand` |

**Key insight:** The only non-trivial part of backoff is the jitter calculation and the future-re-creation contract. Both are solved by `backon`. Everything else in this phase is pure Rust structural code (decorator struct, return-type design).

---

## Common Pitfalls

### Pitfall 1: Retrying on AuthFailed Causes Account Lockout

**What goes wrong:** If `is_retryable` mistakenly includes `AuthFailed` or matches all errors (`|_| true`), the client retries failed logins up to `max_times`. Gmail, Outlook, and most production POP3 servers lock accounts after 5-10 consecutive failed logins from the same IP.

**Why it happens:** Writing `|_| true` as the `.when()` predicate, or forgetting to filter auth errors when the error type changes.

**How to avoid:** Use an explicit allowlist (`matches!(e, Pop3Error::Io(_) | Pop3Error::ConnectionClosed)`) rather than a denylist. Add a test that simulates `AuthFailed` from the mock and asserts the client does NOT retry (i.e., the error propagates after exactly 1 attempt, not 5).

**Warning signs:** Test with `max_times(3)` observing 3 `login()` calls in mock data when `AuthFailed` is returned on the first attempt.

### Pitfall 2: DELE Marks Silently Discarded Without Caller Awareness

**What goes wrong:** If the session-reset signal is in a log line but not the return type, callers assume DELEs from before the disconnect are still pending. On reconnect, the server starts a fresh session — no DELEs are marked. The caller re-issues commands without knowing this, potentially re-downloading already-processed messages.

**Why it happens:** Designing `ReconnectingClient::dele(id) -> Result<()>` (same signature as `Pop3Client::dele`) — looks clean, but hides the reconnect.

**How to avoid:** Use `-> Result<((), Option<SessionReset>)>` on all methods. Even void-returning methods like `dele()` must carry the reset signal.

**Warning signs:** Test that calls `dele()`, simulates a connection drop, then calls `dele()` again — if the second call succeeds silently, the caller has no way to know the first DELE was lost.

### Pitfall 3: Closure Capture of `&mut self` Fields Prevents Retry

**What goes wrong:** Attempting to write the retry closure as:
```rust
let connect = || async { self.client.login(...).await };
```
This tries to capture `self` mutably in an async closure while `self` is already borrowed. It fails to compile.

**Why it happens:** Not extracting the builder/credentials into local bindings before passing the closure to `backon`.

**How to avoid:** Extract all needed values into local `&`-references or clones before the closure:
```rust
let builder = &self.builder;
let username = self.username.as_str();
let password = self.password.as_str();
let connect = || async move { ... };
```

**Warning signs:** Compiler error "cannot borrow `self` as mutable more than once at a time".

### Pitfall 4: backon Requires `FnMut`, Not `Fn` or `FnOnce`

**What goes wrong:** Passing a closure that captures an owned value by move and then moves it in the async block compiles for `FnOnce` but panics on the second attempt because the value was already moved.

**Why it happens:** Writing `|| async move { let x = owned_value; ... }` where `owned_value` is consumed on first invocation.

**How to avoid:** Ensure the closure is `FnMut`: only capture `&` or `&mut` references, or values that implement `Clone` and clone them inside the closure:
```rust
let connect = || {
    let builder = builder.clone();
    async move { builder.connect().await? ... }
};
```

**Warning signs:** First retry attempt succeeds, second fails with a panic or compile error about moved value.

### Pitfall 5: ConnectionClosed Not Yet in Pop3Error When Phase 7 Begins

**What goes wrong:** Phase 7 depends on `Pop3Error::ConnectionClosed` added in Phase 5. If Phase 5 is incomplete, `is_retryable` cannot match it and the code may not compile, or worse, I/O-disconnected sessions are mistakenly matched by `Pop3Error::Io` only (which works but is less precise).

**Why it happens:** Phase dependency on Phase 5.

**How to avoid:** Phase 7 should not begin until Phase 5's `ConnectionClosed` variant is confirmed in `src/error.rs`. If needed, `Pop3Error::Io(ref e) if e.kind() == io::ErrorKind::ConnectionReset` is an acceptable interim fallback for matching broken connections.

**Warning signs:** Compiler error "no variant `ConnectionClosed` on `Pop3Error`".

### Pitfall 6: ExponentialBuilder Defaults — Jitter is Off by Default

**What goes wrong:** Using `ExponentialBuilder::default()` without `.with_jitter()` produces deterministic delays (1s, 2s, 4s, 8s...). Two clients started simultaneously retry in lockstep — the thundering herd problem persists even with exponential backoff.

**Why it happens:** `backon` defaults to `jitter: false`. The default exists for deterministic testing, not production use.

**How to avoid:** Always call `.with_jitter()` on the builder for production `ReconnectingClient`. Jitter adds uniform random noise in `(0, current_delay)`. RECON-04 explicitly requires jitter.

**Warning signs:** Regression test observing exact delay values 1000ms, 2000ms, 4000ms — jitter should produce non-deterministic values. Use `.with_jitter_seed(42)` in tests for reproducibility.

---

## Code Examples

Verified patterns from official sources:

### ExponentialBuilder Full Configuration

```rust
// Source: docs.rs/backon/latest/backon/struct.ExponentialBuilder.html (verified 2026-03-01)
use backon::{ExponentialBuilder, Retryable};
use std::time::Duration;

let backoff = ExponentialBuilder::default()
    .with_jitter()                            // Enable full jitter (uniform random in 0..delay)
    .with_min_delay(Duration::from_secs(1))   // Start at 1s
    .with_max_delay(Duration::from_secs(60))  // Cap at 60s
    .with_max_times(5);                       // Give up after 5 attempts
```

### backon Default Values (verified from docs.rs)

| Parameter | Default | Notes |
|-----------|---------|-------|
| `jitter` | `false` | Must call `.with_jitter()` explicitly for RECON-04 |
| `factor` | `2` | Doubles delay on each attempt |
| `min_delay` | `1s` | First delay is 1 second |
| `max_delay` | `60s` | Delays never exceed 60 seconds |
| `max_times` | `3` | Gives up after 3 total attempts by default |

### Retry with Error Classification

```rust
// Source: backon 1.6.0 README (github.com/Xuanwo/backon)
// .when() takes FnMut(&E) -> bool: return true to retry, false to propagate immediately

connect
    .retry(ExponentialBuilder::default().with_jitter())
    .sleep(tokio::time::sleep)
    .when(|e: &Pop3Error| matches!(e, Pop3Error::Io(_) | Pop3Error::ConnectionClosed))
    .notify(|err: &Pop3Error, dur: std::time::Duration| {
        // Optional: log retry attempts
        tracing::warn!("POP3 connection lost ({err:?}), retrying in {dur:?}");
    })
    .await
```

### SessionReset Return Type Pattern

```rust
// Source: Inspired by reconnecting-jsonrpsee-ws-client state event API
// (docs.rs/reconnecting-jsonrpsee-ws-client/latest/reconnecting_jsonrpsee_ws_client/)

/// Returns `Ok((Stat, None))` on success without reconnect.
/// Returns `Ok((Stat, Some(SessionReset)))` if a reconnect occurred.
/// The `SessionReset` value signals that all pending DELE marks were discarded.
pub async fn stat(&mut self) -> Result<(Stat, Option<SessionReset>)> {
    self.execute(|client| client.stat()).await
}

// Caller usage:
let (stat, reset) = reconnecting_client.stat().await?;
if let Some(SessionReset) = reset {
    // Session was reset — re-issue any DELEs that were pending before the drop
    eprintln!("Warning: session reset, DELE marks were discarded");
}
```

### Test Pattern with Deterministic Jitter

```rust
// Source: backon 1.6.0 docs — with_jitter_seed for reproducible tests
// Use a fixed seed in tests so retry timing is deterministic

let backoff = ExponentialBuilder::default()
    .with_jitter()
    .with_jitter_seed(42)  // Fixed seed: same delays every test run
    .with_max_times(2);
```

### Full ReconnectingClient Skeleton

```rust
// Source: Project-specific, derived from backon README + Pop3Client API
// File: src/reconnect.rs

use crate::{Pop3Client, Pop3ClientBuilder, Pop3Error, Result};
use backon::{ExponentialBuilder, Retryable};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionReset;

pub struct ReconnectingClient {
    builder: Pop3ClientBuilder,
    username: String,
    password: String,
    client: Pop3Client,
}

fn is_retryable(e: &Pop3Error) -> bool {
    matches!(e, Pop3Error::Io(_) | Pop3Error::ConnectionClosed)
}

impl ReconnectingClient {
    pub async fn new(
        builder: Pop3ClientBuilder,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Self> {
        let username = username.into();
        let password = password.into();
        let client = connect_and_auth(&builder, &username, &password).await?;
        Ok(Self { builder, username, password, client })
    }

    async fn reconnect(&mut self) -> Result<()> {
        let builder = &self.builder;
        let username = self.username.as_str();
        let password = self.password.as_str();

        let connect = || async move {
            connect_and_auth(builder, username, password).await
        };

        self.client = connect
            .retry(
                ExponentialBuilder::default()
                    .with_jitter()
                    .with_min_delay(Duration::from_secs(1))
                    .with_max_delay(Duration::from_secs(60))
                    .with_max_times(5),
            )
            .sleep(tokio::time::sleep)
            .when(is_retryable)
            .await?;
        Ok(())
    }

    pub async fn stat(&mut self) -> Result<(crate::Stat, Option<SessionReset>)> {
        match self.client.stat().await {
            Ok(v) => Ok((v, None)),
            Err(e) if is_retryable(&e) => {
                self.reconnect().await?;
                let v = self.client.stat().await?;
                Ok((v, Some(SessionReset)))
            }
            Err(e) => Err(e),
        }
    }
    // ... same pattern for list(), retr(), dele(), uidl(), etc.
}

async fn connect_and_auth(
    builder: &Pop3ClientBuilder,
    username: &str,
    password: &str,
) -> Result<Pop3Client> {
    let mut client = builder.connect().await?;
    client.login(username, password).await?;
    Ok(client)
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `backoff` crate (ihrwein/backoff) | `backon` 1.6.0 | 2023 onwards | `backoff` is unmaintained; `backon` has cleaner ergonomics (method chaining on closures) and active maintenance |
| Hand-rolled retry loop in application code | Library-provided retry via `backon` `.retry()` trait extension | 2022 onwards | Eliminates per-project retry reimplementation; backon handles the future re-creation contract correctly |
| No reconnect — caller re-creates client on error | `ReconnectingClient` Decorator | Phase 7 | Callers get transparent reconnect for free while keeping session-state visibility |
| Reconnect without state-loss signal | `Option<SessionReset>` in return type | Phase 7 design decision | REQUIREMENTS.md explicitly forbids silent DELE re-issue; compile-time-visible signal enforces this |

**Deprecated/outdated:**
- `ihrwein/backoff`: Unmaintained as of 2022. GitHub shows no commits since then. The `backoff` crate is the wrong choice for new code.
- `tokio-retry`: Last released 2021. Does not support `.when()` for conditional error classification without a manual wrapper.

---

## Open Questions

1. **Method delegation scope: which Pop3Client methods should ReconnectingClient expose?**
   - What we know: The requirements say the caller "continues working after a simulated I/O drop" — this implies at minimum `stat()`, `list()`, `retr()`, `dele()`, `uidl()`.
   - What's unclear: Should `stls()`, `top()`, `capa()` be exposed? Should `quit()` be exposed (consuming self makes it fit naturally)?
   - Recommendation: Expose all transaction-state methods (`stat`, `list`, `uidl`, `retr`, `dele`, `top`, `noop`, `rset`) plus `quit()`. Skip TLS-upgrade (`stls()`) — it cannot be re-issued after reconnect in a useful way and complicates the builder state.

2. **`rset()` and `noop()` semantics after reconnect**
   - What we know: `rset()` un-marks all pending DELEs in the current session. After a reconnect, there are no pending DELEs to un-mark, so `rset()` is a no-op (but still valid to send). `noop()` is always valid.
   - What's unclear: Should `rset()` return `Some(SessionReset)` if a reconnect occurred even though there was nothing to reset?
   - Recommendation: Yes — return `Some(SessionReset)` on ANY reconnect, regardless of the method. The reconnect happened; the caller must know regardless of which method triggered it.

3. **TLS configuration in Pop3ClientBuilder (Phase 4 API not yet designed)**
   - What we know: Phase 4's builder is planned. Phase 7 depends on it being `Clone`.
   - What's unclear: Exact builder API shape (hostname, TLS mode, timeout as fields vs builder methods).
   - Recommendation: Phase 7 planning should note this dependency. If Phase 4 builder design changes, Phase 7 `new()` constructor signature may need adjustment.

4. **`execute()` helper vs explicit match in each method**
   - What we know: The `execute<T, F, Fut>(&mut self, op: F)` pattern is ergonomic but requires `op` to be `Fn(&mut Pop3Client) -> Fut`. Some commands need extra parameters (e.g., `retr(id: u32)`).
   - What's unclear: Whether `Fn(&mut Pop3Client) -> Fut` can capture parameters by closure or if each method needs its own match.
   - Recommendation: Use explicit `match` per method (as shown in the `stat()` example) rather than a generic `execute()`. The `execute()` helper only works cleanly for zero-argument methods; parameterized ones need closures capturing the arg, which can be tricky with lifetimes.

---

## Sources

### Primary (HIGH confidence)
- [docs.rs/backon/latest/backon/struct.ExponentialBuilder.html](https://docs.rs/backon/latest/backon/struct.ExponentialBuilder.html) — All ExponentialBuilder methods and defaults verified
- [docs.rs/crate/backon/latest/source/Cargo.toml](https://docs.rs/crate/backon/latest/source/Cargo.toml) — Confirmed `fastrand = {version = "2", default-features = false}` as sole RNG dependency; no `rand` or `getrandom`
- [github.com/Xuanwo/backon](https://github.com/Xuanwo/backon) — backon 1.6.0 confirmed (25 releases, active); README examples for `.retry()`, `.when()`, `.notify()`, `.sleep()`
- `F:\My Github Repos\Open Source Repos\rust-adv-pop3\src\error.rs` — Confirmed `Pop3Error` variants; `AuthFailed` is a distinct variant, `Io` wraps `std::io::Error`
- `F:\My Github Repos\Open Source Repos\rust-adv-pop3\.planning\STATE.md` — Confirmed "backon 1.6 is unconditional dependency" decision
- `F:\My Github Repos\Open Source Repos\rust-adv-pop3\.planning\REQUIREMENTS.md` — "Transparent auto-reconnect with silent DELE re-issue" is explicitly OUT OF SCOPE
- `F:\My Github Repos\Open Source Repos\rust-adv-pop3\src\types.rs` — SessionState enum and existing public types

### Secondary (MEDIUM confidence)
- [docs.rs/reconnecting-jsonrpsee-ws-client](https://docs.rs/reconnecting-jsonrpsee-ws-client/latest/reconnecting_jsonrpsee_ws_client/) — State-loss signaling pattern; `reconnect_started()` / `reconnected()` methods; `CallRetryPolicy` for side-effect control
- [docs.rs/reconnecting-websocket](https://docs.rs/reconnecting-websocket) — `Event<Message, State>` enum approach for separating data from connection events; shows industry pattern for surfacing reconnects structurally
- [docs.rs/stream-reconnect](https://docs.rs/stream-reconnect/latest/stream_reconnect/) — `UnderlyingStream` trait; `is_read_disconnect_error` / `is_write_disconnect_error` pattern for retryable error classification
- [rustmagazine.org: How I Designed the API for Backon](https://rustmagazine.org/issue-2/how-i-designed-the-api-for-backon-a-user-friendly-retry-crate/) — Core design: `FnMut() -> Fut` requirement; `Retry` struct implements `Future`; `.when()` conditional logic

### Tertiary (LOW confidence)
- [users.rust-lang.org: How to impl async client auto reconnect](https://users.rust-lang.org/t/how-to-impl-async-client-auto-reconnect-to-server-when-disconnect/65587) — Community pattern: separate tasks + channels; confirms decorator approach is simpler for non-multiplexed protocols like POP3
- [oneuptime.com: Exponential Backoff with Jitter in Rust](https://oneuptime.com/blog/post/2026-01-25-exponential-backoff-jitter-rust/view) — Hand-rolled jitter implementation for reference; confirms `backon` handles all of this correctly; `Full` jitter is the correct strategy for thundering herd

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — `backon` 1.6.0 confirmed via docs.rs and GitHub; already selected in STATE.md roadmap decisions; no alternatives needed
- Architecture: HIGH — Decorator pattern is mechanically obvious given `Pop3ClientBuilder` Clone + `Pop3Client` public API; backon API verified against official docs
- SessionReset API design: HIGH — REQUIREMENTS.md explicitly forbids silent DELE re-issue; comparable crates (jsonrpsee-ws, reconnecting-websocket) use structural state-event approaches; return-type approach is the most compile-time-visible option available
- Error classification (RECON-02): HIGH — `AuthFailed` as a distinct variant (confirmed in error.rs) makes exact matching possible; `ConnectionClosed` depends on Phase 5 completion
- Pitfalls: HIGH — Most are Rust borrow checker / backon API contract issues verifiable from first principles; account lockout pitfall is documented behavior for Gmail/Outlook

**Research date:** 2026-03-01
**Valid until:** 2027-03-01 (backon API is stable post-v1; reconnect pattern is protocol-agnostic and stable)

---

## Deep Dive: backon API Reference

**Added:** 2026-03-01 (checkpoint dig-deeper)
**Sources:** github.com/Xuanwo/backon source (exponential.rs), docs.rs/backon/latest/backon/struct.ExponentialBuilder.html, docs.rs/backon/latest/backon/struct.Retry.html — all HIGH confidence

### ExponentialBuilder Defaults (Verified from Source)

The following defaults are confirmed from the `ExponentialBuilder::new()` source in the backon repository:

| Parameter | Default Value | Type | Notes |
|-----------|--------------|------|-------|
| `jitter` | `false` | `bool` | Must call `.with_jitter()` — off by default by design (determinism for testing) |
| `factor` | `2.0` | `f32` | Doubles delay on each attempt |
| `min_delay` | `Duration::from_secs(1)` | `Duration` | First retry waits 1 second |
| `max_delay` | `Some(Duration::from_secs(60))` | `Option<Duration>` | Delay cap; call `.without_max_delay()` to remove |
| `max_times` | `Some(3)` | `Option<usize>` | 3 total attempts by default; call `.without_max_times()` to retry forever |

**Critical implication:** `max_times` defaults to `Some(3)`, meaning `ExponentialBuilder::default()` will give up after 3 attempts total. For Phase 7, this must be overridden with `.with_max_times(5)` (or higher) to match the production retry budget.

### ExponentialBuilder Method Signatures (Verified from docs.rs)

All methods are `const fn` except `with_jitter_seed`:

```rust
// Source: docs.rs/backon/latest/backon/struct.ExponentialBuilder.html
pub const fn with_jitter(self) -> Self
// Effect: jitter = true; adds uniform random delay in (0, current_delay) to each wait

pub fn with_jitter_seed(self, seed: u64) -> Self
// Effect: sets FastRand seed for reproducible jitter — use 42 in tests, omit in production

pub const fn with_factor(self, factor: f32) -> Self
// Effect: multiplier per attempt; default 2.0 (doubling)

pub const fn with_min_delay(self, min_delay: Duration) -> Self
// Effect: minimum/initial delay; default 1s

pub const fn with_max_delay(self, max_delay: Duration) -> Self
// Effect: delay cap; default 60s

pub const fn without_max_delay(self) -> Self
// Effect: removes cap — delay keeps increasing; use only for idempotent operations

pub const fn with_max_times(self, max_times: usize) -> Self
// Effect: total attempt limit; default 3

pub const fn without_max_times(self) -> Self
// Effect: retry forever — NEVER use for reconnect (server-down loop)

pub const fn with_total_delay(self, total_delay: Option<Duration>) -> Self
// Effect: cumulative time budget; stops when next sleep would exceed budget
```

### Retry Struct Method Signatures (Verified from docs.rs/backon/latest/backon/struct.Retry.html)

```rust
// Source: docs.rs/backon/latest/backon/struct.Retry.html

// Set the sleep function — REQUIRED for async contexts
pub fn sleep<SN: Sleeper>(self, sleep_fn: SN) -> Retry<B, T, E, Fut, FutureFn, SN, RF, NF, AF>
// tokio::time::sleep is the correct SN for this project
// Omitting .sleep() causes: "the trait bound `DefaultSleeper: Sleeper` is not satisfied"
// in async contexts where DefaultSleeper is not tokio-based

// Set the error classification predicate
pub fn when<RN: FnMut(&E) -> bool>(self, retryable: RN) -> Retry<B, T, E, Fut, FutureFn, SF, RN, NF, AF>
// Closure receives &E (immutable reference to the error)
// Returns true to retry, false to propagate immediately
// Default (no .when()): all errors are retried — DO NOT rely on this

// Set the notification callback (fires before each retry sleep)
pub fn notify<NN: FnMut(&E, Duration)>(self, notify: NN) -> Retry<B, T, E, Fut, FutureFn, SF, RF, NN, AF>
// Closure receives (&E, Duration): the error and the upcoming sleep duration
// Useful for logging: tracing::warn!("reconnecting in {dur:?}: {err}")
// Does NOT affect retry logic — purely observational
```

### Jitter Internals

Jitter uses `fastrand` 2.x (the crate's transitive dep). When `.with_jitter()` is set, each computed delay `d` is replaced with a uniform random value in the open interval `(0, d)`. This is the "Full Jitter" strategy described in the AWS Architecture Blog on exponential backoff and jitter — empirically the best strategy for preventing thundering herd while maintaining reasonable average reconnect latency.

The delay sequence WITHOUT jitter for defaults (factor=2, min=1s, max=60s):
- Attempt 1 fails: wait 1s
- Attempt 2 fails: wait 2s
- Attempt 3 fails: wait 4s
- ... caps at 60s

The delay sequence WITH jitter: each value is replaced by `rand(0, d)` — no two clients retry at the same moment.

### tokio-sleep Feature

`tokio-sleep` is enabled by default in `backon` for non-wasm32 targets. This means `tokio::time::sleep` satisfies the `Sleeper` trait without any additional feature flag in `Cargo.toml`. The `backon = "1.6"` dependency line (no features specified) is sufficient.

---

## Deep Dive: SessionReset Design Decision

**Added:** 2026-03-01 (checkpoint dig-deeper)
**Decision:** Use `Result<(T, Option<SessionReset>)>` return type on all methods.

### Design Space Evaluated

Four approaches were researched. The chosen approach is documented first. The others are documented as rejected alternatives with rationale — so the planner does not re-evaluate them.

#### Chosen: Tuple Return `(T, Option<SessionReset>)`

**API shape:**
```rust
pub async fn stat(&mut self) -> Result<(Stat, Option<SessionReset>)>
pub async fn dele(&mut self, id: u32) -> Result<((), Option<SessionReset>)>
pub async fn list(&mut self, id: Option<u32>) -> Result<(Vec<ListEntry>, Option<SessionReset>)>
// ... all transaction-state methods follow the same pattern
```

**Pros:**
- Structurally unignorable: `let (stat, reset) = client.stat().await?` forces destructuring; the `reset` binding is visible even if unused (`#[must_use]` can make ignoring it a warning)
- Zero additional types: `SessionReset` is a ZST; `Option<SessionReset>` has zero runtime overhead (it's `Option<()>` equivalent)
- Composition with `?` still works: the outer `Result` propagates errors normally; session reset is orthogonal to error handling
- Callers that genuinely don't care can write `let (v, _) = client.stat().await?` — explicit and visible

**Cons:**
- Every call site must destructure: `client.stat().await?` no longer works; must write `client.stat().await?.0` or destructure
- The `()` return case for void methods (`dele`, `rset`, `noop`) is awkward: `Result<((), Option<SessionReset>)>` reads oddly
- Cannot be implemented via `Deref<Target=Pop3Client>` since the signatures differ from `Pop3Client`'s

**Verdict:** Chosen. The ergonomic cost (destructuring at every call site) is acceptable and is exactly the design intent — callers must acknowledge the session-reset possibility.

#### Rejected: `ReconnectResult<T>` Custom Wrapper Type

**API shape:**
```rust
pub struct ReconnectResult<T> {
    pub value: T,
    pub session_reset: bool,
}

pub async fn stat(&mut self) -> Result<ReconnectResult<Stat>>
```

**Pros:** Slightly more readable than tuple; named fields
**Cons:** Adds a new public type; callers can access `.value` without ever reading `.session_reset`; not meaningfully different from a tuple ergonomically; adds boilerplate (impl Display, Debug, From for ReconnectResult); adds a third type to the public API alongside `SessionReset` ZST — redundant

**Verdict:** Rejected. The named struct provides no compile-time enforcement advantage over a tuple. A caller can still ignore `.session_reset` silently. The ZST approach with `Option<SessionReset>` is simpler.

#### Rejected: Callback/Hook `on_reconnect: impl Fn()`

**API shape:**
```rust
pub struct ReconnectingClient {
    // ...
    on_reconnect: Box<dyn Fn() + Send>,
}

// Constructor:
ReconnectingClient::new(builder, user, pass)
    .on_reconnect(|| { /* handle session reset */ })
```

**Pros:** Caller code at individual call sites stays identical to `Pop3Client`; reconnect handling is centralized
**Cons:** Caller can pass a no-op `|| {}` and the compiler does not complain; state-reset handling is disconnected from the method call that triggered it; impossible to correlate which call triggered the reconnect; callers cannot condition reconnect logic on the result value; REQUIREMENTS.md intent is explicit state-loss notification "at the call site", not a global callback

**Verdict:** Rejected. A callback allows callers to silently ignore reconnects by providing a no-op. RECON-03 specifically requires that session-state loss "cannot be silently ignored."

#### Rejected: mpsc Event Channel

**API shape:**
```rust
let (client, mut reconnect_events) = ReconnectingClient::new(builder, user, pass).await?;
// ...
tokio::spawn(async move {
    while let Some(event) = reconnect_events.recv().await {
        // handle reconnect
    }
});
```

**Pros:** Reconnect events are decoupled from the command calls; polling the channel from a separate task is natural in Tokio
**Cons:** Two separate objects to manage (client + channel receiver); callers can simply not spawn the polling task, silently dropping all events; correlating which call triggered the reconnect requires additional event metadata; overkill for a non-multiplexed protocol — POP3 is inherently sequential, so the reconnect event always arrives in a defined order relative to the triggering method call; adds `mpsc` channel overhead to every method call even when no reconnect occurs

**Verdict:** Rejected. The channel approach is idiomatic for multiplexed protocols (JSON-RPC, WebSocket) where multiple concurrent requests may be in flight when a disconnect occurs. POP3 is strictly sequential — one command at a time — so the simpler tuple-return approach is strictly sufficient.

### Comparison with Ecosystem Crates

| Crate | Approach | Notes |
|-------|----------|-------|
| `reconnecting-jsonrpsee-ws-client` | `CallRetryPolicy` enum + side methods `reconnect_started()` / `reconnected()` | Multiplexed RPC; caller checks methods on the client to detect state; not compile-time forced |
| `reconnecting-websocket` (npm) | `Event<Message, State>` enum stream | Event-driven; caller polls the event stream; reconnect is an event variant |
| `deadpool` | Pool returns `Object<M>` wrapping a healthy connection; failed checkout is an `Err` | Pool transparency — no session-state concept; different domain |
| `bb8` | Same as deadpool — error on failed get, no session-state signaling | Different domain; Phase 8 concern |

None of these crates use the exact `(T, Option<ReconnectSignal>)` tuple approach, but all use some structurally distinct type to force caller awareness. The tuple approach is the simplest adaptation of this principle for a single-connection, sequential protocol.

### `#[must_use]` Annotation

Add `#[must_use]` to `SessionReset` to make the compiler warn when the reset flag is ignored in a `let _ = reset` pattern:

```rust
#[must_use = "check whether a session reset occurred to detect discarded DELE marks"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionReset;
```

This does not prevent `let _ = reset` (which is explicit and deliberate), but it catches unintentional omissions like:
```rust
// This triggers "unused variable" warning, nudging caller to handle reset:
let (stat, reset) = client.stat().await?;
println!("{}", stat.message_count);
// reset is never read — compiler warns
```

---

## Deep Dive: ReconnectingClient Method Matrix

**Added:** 2026-03-01 (checkpoint dig-deeper)

The following table covers every public method on `Pop3Client` (confirmed from `src/client.rs` read on 2026-03-01) and the decision for how it maps onto `ReconnectingClient`.

### Complete Method Decision Table

| Method | Signature on Pop3Client | Decision | Signature on ReconnectingClient | Rationale |
|--------|------------------------|----------|---------------------------------|-----------|
| `connect` | `async fn connect(addr, timeout) -> Result<Self>` | **HIDE (internal)** | Not exposed | Connection is managed internally by `ReconnectingClient::new()` and `reconnect()`; exposing it would let callers bypass the decorator |
| `connect_default` | `async fn connect_default(addr) -> Result<Self>` | **HIDE (internal)** | Not exposed | Same as above |
| `connect_tls` | `async fn connect_tls(addr, hostname, timeout) -> Result<Self>` | **HIDE (internal)** | Not exposed | TLS mode is encoded in the stored `Pop3ClientBuilder`; callers configure it via the builder before passing to `ReconnectingClient::new()` |
| `connect_tls_default` | `async fn connect_tls_default(addr, hostname) -> Result<Self>` | **HIDE (internal)** | Not exposed | Same as above |
| `greeting` | `fn greeting(&self) -> &str` | **EXPOSE as-is** | `fn greeting(&self) -> &str` | Read-only accessor; no I/O; no session-state implications; delegates directly to inner client |
| `state` | `fn state(&self) -> SessionState` | **EXPOSE as-is** | `fn state(&self) -> SessionState` | Read-only; always returns `Authenticated` when inner client is healthy; useful for callers |
| `is_encrypted` | `fn is_encrypted(&self) -> bool` | **EXPOSE as-is** | `fn is_encrypted(&self) -> bool` | Read-only; TLS state does not change after reconnect (builder preserves TLS config) |
| `stls` | `async fn stls(&mut self, hostname: &str) -> Result<()>` | **HIDE** | Not exposed | STLS upgrades a plaintext connection to TLS; after reconnect, the new connection is already in the TLS state configured by the builder; re-issuing STLS on an already-TLS connection is an error; exposing it complicates the API without benefit |
| `login` | `async fn login(&mut self, u, p) -> Result<()>` | **HIDE (internal)** | Not exposed | Login is performed automatically during `new()` and each `reconnect()`; exposing it lets callers call login twice, double-authenticate, or issue it after already authenticated (returns `NotAuthenticated` error per current impl) |
| `apop` | `async fn apop(&mut self, u, p) -> Result<()>` | **HIDE** | Not exposed | Deprecated on `Pop3Client`; APOP requires the server greeting timestamp which is only available at connect time; `ReconnectingClient` stores credentials for `login()` only; APOP reconnect support would require storing credentials in a special way — not worth the complexity for a deprecated auth method |
| `stat` | `async fn stat(&mut self) -> Result<Stat>` | **EXPOSE with reset** | `async fn stat(&mut self) -> Result<(Stat, Option<SessionReset>)>` | Core transaction command; must carry reset signal |
| `list` | `async fn list(&mut self, id: Option<u32>) -> Result<Vec<ListEntry>>` | **EXPOSE with reset** | `async fn list(&mut self, id: Option<u32>) -> Result<(Vec<ListEntry>, Option<SessionReset>)>` | Core transaction command; must carry reset signal |
| `uidl` | `async fn uidl(&mut self, id: Option<u32>) -> Result<Vec<UidlEntry>>` | **EXPOSE with reset** | `async fn uidl(&mut self, id: Option<u32>) -> Result<(Vec<UidlEntry>, Option<SessionReset>)>` | Core transaction command; must carry reset signal |
| `retr` | `async fn retr(&mut self, id: u32) -> Result<Message>` | **EXPOSE with reset** | `async fn retr(&mut self, id: u32) -> Result<(Message, Option<SessionReset>)>` | Core transaction command; must carry reset signal |
| `dele` | `async fn dele(&mut self, id: u32) -> Result<()>` | **EXPOSE with reset** | `async fn dele(&mut self, id: u32) -> Result<((), Option<SessionReset>)>` | Primary mutation command; DELE marks are exactly what is lost on reconnect; reset signal is critical here |
| `rset` | `async fn rset(&mut self) -> Result<()>` | **EXPOSE with reset** | `async fn rset(&mut self) -> Result<((), Option<SessionReset>)>` | If a reconnect occurred before `rset()`, the caller must know — there were no DELEs to reset anyway, but the session context changed |
| `noop` | `async fn noop(&mut self) -> Result<()>` | **EXPOSE with reset** | `async fn noop(&mut self) -> Result<((), Option<SessionReset>)>` | Keepalive; can trigger a connection-drop detection; must carry reset signal for consistency |
| `top` | `async fn top(&mut self, id: u32, lines: u32) -> Result<Message>` | **EXPOSE with reset** | `async fn top(&mut self, id: u32, lines: u32) -> Result<(Message, Option<SessionReset>)>` | Read-only preview; carries reset signal for consistency with all other methods |
| `capa` | `async fn capa(&mut self) -> Result<Vec<Capability>>` | **EXPOSE with reset** | `async fn capa(&mut self) -> Result<(Vec<Capability>, Option<SessionReset>)>` | Valid before and after auth; can be called on fresh reconnected session; carries reset signal |
| `quit` | `async fn quit(self) -> Result<()>` | **EXPOSE, SPECIAL** | `async fn quit(self) -> Result<()>` | Consumes `self` — no reconnect can occur (the client is gone); no session-reset signal needed; same signature as `Pop3Client::quit`; after `quit`, the `ReconnectingClient` is dropped |

### Design Rationale for `quit()`

`quit()` consumes `self` on `Pop3Client`, which is preserved on `ReconnectingClient`. This means:

- `ReconnectingClient::quit(self)` takes ownership, sends `QUIT`, and drops both the inner `Pop3Client` and the stored credentials
- No reconnect is attempted if `QUIT` fails with an I/O error — the session is ending anyway; the inner client is dropped on `quit` regardless
- The return type is `Result<()>` (no `Option<SessionReset>`) because there is no session to reset after quit — the decorator is gone

```rust
// Source: project-specific derivation from Pop3Client::quit pattern
pub async fn quit(self) -> Result<()> {
    self.client.quit().await
    // self (including builder, username, password) is dropped here
}
```

### No `Deref<Target=Pop3Client>`

Do NOT implement `Deref<Target=Pop3Client>` on `ReconnectingClient`. If `Deref` were implemented:
- Callers could bypass the reconnect logic by dereferencing to the inner `Pop3Client` and calling methods directly
- Return types would be inconsistent: `&Pop3Client` methods return `Result<T>` (no reset signal), while `ReconnectingClient` methods return `Result<(T, Option<SessionReset>)>`
- The compile-time guarantee of RECON-03 would be broken: callers could accidentally use the deref path and get no session-reset signal

Use explicit delegation (individual `pub async fn` wrappers per method) instead.

### Builder Integration

`Pop3ClientBuilder` does NOT get a `.with_reconnect()` method. Instead, `ReconnectingClient::new()` takes an already-configured builder. This keeps the builder's responsibility clean (connection configuration only) and avoids builder methods that return a fundamentally different type.

```rust
// Correct usage pattern:
let client = ReconnectingClient::new(
    Pop3ClientBuilder::new("pop.example.com")
        .port(995)
        .tls(),
    "user@example.com",
    "app-password",
).await?;

// NOT: Pop3ClientBuilder::new(...).with_reconnect(...) — this would change the builder's
// return type from Pop3Client to ReconnectingClient, which is unexpected
```

---

## Deep Dive: Error Classification Matrix

**Added:** 2026-03-01 (checkpoint dig-deeper)
**Sources:** `src/error.rs` (confirmed variants), doc.rust-lang.org/std/io/enum.ErrorKind.html (io::ErrorKind classification), backon docs (`.when()` semantics)

### Complete Pop3Error Retryability Matrix

Every variant of `Pop3Error` (confirmed from `src/error.rs`) is classified below. The `is_retryable()` function in `src/reconnect.rs` MUST match exactly the variants marked RETRY.

| Variant | Retryable? | Classification | Rationale |
|---------|-----------|----------------|-----------|
| `Pop3Error::Io(_)` | **CONDITIONAL** | See sub-table below | Wraps `std::io::Error`; retryability depends on the underlying `io::ErrorKind` — but in practice, match the whole variant and let `backon` handle attempts |
| `Pop3Error::ConnectionClosed` | **RETRY** | Transient | Server closed the connection (idle timeout, server restart, network interruption); reconnect should succeed |
| `Pop3Error::AuthFailed(_)` | **NEVER RETRY** | Permanent | Wrong credentials; retrying risks account lockout; distinct variant makes this unambiguous |
| `Pop3Error::ServerError(_)` | **NEVER RETRY** | Permanent (mostly) | Server rejected the command with `-ERR`; this is a protocol-level rejection, not a connectivity failure; see nuance note below |
| `Pop3Error::Tls(_)` | **NEVER RETRY** | Permanent (mostly) | TLS handshake or I/O failure; see nuance note below |
| `Pop3Error::InvalidDnsName(_)` | **NEVER RETRY** | Permanent | Malformed hostname; caller error; no amount of retrying fixes a bad DNS name |
| `Pop3Error::MailboxInUse(_)` | **NEVER RETRY** | Transient-but-unretryable | Server returned `[IN-USE]` RESP-CODE; another session holds the mailbox lock; retrying immediately will keep failing; retry is the caller's responsibility with a user-supplied delay, not the reconnect logic's |
| `Pop3Error::LoginDelay(_)` | **NEVER RETRY** | Transient-but-unretryable | Server returned `[LOGIN-DELAY]`; server demands a cooldown period before next login; the retry interval in `ExponentialBuilder` (1s–60s) is insufficient — server may require minutes; propagate to caller |
| `Pop3Error::SysTemp(_)` | **NEVER RETRY** | Transient-but-unretryable | Server returned `[SYS/TEMP]`; the server infrastructure has a temporary problem, but this is not a connection drop; reconnecting to the same server will likely get the same response; propagate to caller |
| `Pop3Error::SysPerm(_)` | **NEVER RETRY** | Permanent | Server returned `[SYS/PERM]`; requires manual intervention; never retry |
| `Pop3Error::Parse(_)` | **NEVER RETRY** | Permanent | Server sent a malformed response; retrying the same command will likely produce the same response; bug in the server or the response parser |
| `Pop3Error::NotAuthenticated` | **NEVER RETRY** | Logic error | Should be impossible in `ReconnectingClient` — reconnect always re-authenticates; if this surfaces, there is a bug in the reconnect logic, not a transient network issue |
| `Pop3Error::InvalidInput` | **NEVER RETRY** | Permanent | Caller supplied a string with CRLF injection; this is a caller bug; retrying the same invalid input will produce the same error |
| `Pop3Error::Timeout` | **RETRY** | Transient | Read timeout expired — the server or network is slow but not closed; retrying on a fresh connection is appropriate |

### `Pop3Error::Io(_)` Sub-Classification

`Pop3Error::Io` wraps `std::io::Error`. In the `is_retryable()` function, match the whole `Pop3Error::Io(_)` variant and treat it as retryable. Do NOT inspect `io::ErrorKind` inside `is_retryable()` — this would add complexity for minimal benefit and risks missing new `ErrorKind` variants added in future Rust versions.

The reasoning: any I/O error on a POP3 connection means the connection is no longer usable. The correct response is always to discard the socket and reconnect. Even for errors like `PermissionDenied` (which is "non-retryable" in a general sense), on a POP3 socket it almost certainly means the OS closed the connection for reasons outside our control, and a reconnect attempt is appropriate.

The one exception worth knowing about (for documentation purposes):

| io::ErrorKind | General Classification | On POP3 Socket | Actual Action |
|--------------|----------------------|----------------|---------------|
| `ConnectionReset` | Retryable | Definite drop | RETRY |
| `ConnectionAborted` | Retryable | Definite drop | RETRY |
| `BrokenPipe` | Retryable | Definite drop | RETRY |
| `UnexpectedEof` | Retryable | Server closed stream | RETRY |
| `TimedOut` | Retryable | Slow network | RETRY |
| `Interrupted` | Retryable | OS signal | RETRY |
| `WouldBlock` | Retryable | Tokio async context — should not appear | RETRY (harmless) |
| `ConnectionRefused` | Non-retryable (general) | Server not listening | RETRY (connection refused may be transient — server restarting) |
| `PermissionDenied` | Non-retryable (general) | OS firewall/socket closure | RETRY (socket-level permission errors are unusual; attempt reconnect) |
| `AddrNotAvailable` | Non-retryable (general) | Invalid local bind | RETRY up to max_times, then give up |

**Conclusion:** Match `Pop3Error::Io(_)` as a single retryable block. The `backon` max_times limit ensures we do not retry forever even if the error is truly permanent.

### `Pop3Error::Tls(_)` Nuance

TLS errors come in two flavours:

- **Certificate errors** (invalid cert, expired, hostname mismatch): these are permanent — no reconnect will fix them. Example: `CertificateUnknown`, `HandshakeFailure` due to cert rejection.
- **Transient TLS I/O errors** (connection closed mid-record, TLS alert received): these are connection drops wrapped in a TLS frame.

In this codebase, `Pop3Error::Tls(String)` is a backend-agnostic string — there is no structured way to distinguish cert errors from transient errors without parsing the string. The decision is:

**Classify `Pop3Error::Tls(_)` as NEVER RETRY.** Rationale:
- If it is a cert error, retrying is wrong and will fail again
- If it is a transient TLS I/O drop, it typically surfaces as `Pop3Error::Io(_)` (the underlying TCP read fails) before or instead of a TLS error
- The string-parsing approach to distinguish cert errors is fragile across both rustls and openssl backends
- Production: if a TLS error occurs repeatedly, the user needs to be informed — silent retries would mask the issue

### `Pop3Error::ServerError(_)` Nuance

`ServerError` is returned when the server sends `-ERR` and the error does not map to a more specific variant. This is a **protocol-level rejection**. Examples:

- `"-ERR no such message"` — the message number is wrong; retrying the same dele will fail again
- `"-ERR mailbox locked"` — same as `MailboxInUse` but without RESP-CODE; transient but not a connection drop

**Classify `Pop3Error::ServerError(_)` as NEVER RETRY.** These are protocol-level rejections that indicate the server understood and rejected the request. Reconnecting does not help. If the server is temporarily unavailable, it will not produce `-ERR` — it will produce an I/O error (connection drop), which IS retried.

### Definitive `is_retryable()` Implementation

```rust
// Source: project-specific; derived from error.rs variant analysis and io::ErrorKind docs
// File: src/reconnect.rs

/// Returns true if the error represents a transient connection failure that
/// should trigger a reconnect attempt.
///
/// Only `Pop3Error::Io` and `Pop3Error::ConnectionClosed` are retried.
/// All other variants are protocol-level errors, authentication failures,
/// or caller errors that reconnection cannot fix.
///
/// # Security note
/// `Pop3Error::AuthFailed` is explicitly excluded. Retrying authentication
/// failures risks account lockout on servers with brute-force protection
/// (Gmail, Outlook, and most production POP3 servers).
fn is_retryable(e: &Pop3Error) -> bool {
    matches!(e, Pop3Error::Io(_) | Pop3Error::ConnectionClosed | Pop3Error::Timeout)
}
```

Note that `Pop3Error::Timeout` is added to the retryable set (compared to the original research). A read timeout means the server is slow or the network is congested — the connection may still be alive, or may have gone silent. Reconnecting on a fresh connection is the correct response. This is consistent with how `Pop3Error::ConnectionClosed` is handled.

### Edge Cases

**DNS failure:** A DNS resolution failure manifests as `Pop3Error::Io(e)` where `e.kind()` is typically `io::ErrorKind::Other` (on most platforms). This IS retried by the current `is_retryable()`. This is acceptable: DNS failures are often transient (NXDOMAIN is not, but `SERVFAIL` is). `backon`'s `max_times(5)` limit ensures we do not retry forever.

**Connection refused:** `io::ErrorKind::ConnectionRefused` maps to `Pop3Error::Io(_)` and IS retried. This is appropriate: connection refused typically means the server is temporarily down or restarting. After max_times attempts, the error propagates to the caller.

**Address not available:** `io::ErrorKind::AddrNotAvailable` typically means the local network interface is down. This is retried — the network interface may come back up. After max_times, it propagates.

**Server at capacity:** Some servers return `-ERR [SYS/TEMP] server busy`. This is `Pop3Error::SysTemp(_)` and is NOT retried by the reconnect logic. It is not a connection drop — the connection succeeded and the server responded. Retrying immediately would produce the same response. This should be documented in `ReconnectingClient` rustdoc: "For `[SYS/TEMP]` errors, the caller is responsible for implementing a separate retry strategy."

### Summary: The `is_retryable` Allowlist

```
RETRY:
  - Pop3Error::Io(_)               — any I/O failure on the socket
  - Pop3Error::ConnectionClosed    — server closed connection cleanly
  - Pop3Error::Timeout             — read timeout expired

NEVER RETRY (all others):
  - Pop3Error::AuthFailed(_)       — SECURITY: never retry auth failures
  - Pop3Error::ServerError(_)      — protocol rejection
  - Pop3Error::Tls(_)              — TLS error (may be cert; cannot distinguish)
  - Pop3Error::InvalidDnsName(_)   — caller error
  - Pop3Error::MailboxInUse(_)     — caller must handle retry with longer delay
  - Pop3Error::LoginDelay(_)       — server demands cooldown; not reconnect's problem
  - Pop3Error::SysTemp(_)          — transient server error, not a connection drop
  - Pop3Error::SysPerm(_)          — permanent server error
  - Pop3Error::Parse(_)            — server sent bad response
  - Pop3Error::NotAuthenticated    — logic error in reconnect code
  - Pop3Error::InvalidInput        — caller error (CRLF injection)
```

---

## Updated Sources (Deep Dive Additions)

### Primary (HIGH confidence) — added in deep dive
- [docs.rs/backon/latest/backon/struct.ExponentialBuilder.html](https://docs.rs/backon/latest/backon/struct.ExponentialBuilder.html) — All `const fn` method signatures confirmed
- [docs.rs/backon/latest/backon/struct.Retry.html](https://docs.rs/backon/latest/backon/struct.Retry.html) — `.when()`, `.notify()`, `.sleep()` exact generic signatures confirmed
- [github.com/Xuanwo/backon/blob/main/backon/src/backoff/exponential.rs](https://github.com/Xuanwo/backon/blob/main/backon/src/backoff/exponential.rs) — `ExponentialBuilder::new()` defaults confirmed: jitter=false, factor=2.0, min=1s, max=Some(60s), max_times=Some(3)
- [doc.rust-lang.org/std/io/enum.ErrorKind.html](https://doc.rust-lang.org/std/io/enum.ErrorKind.html) — io::ErrorKind variants and retryability classification
- `F:\My Github Repos\Open Source Repos\rust-adv-pop3\src\client.rs` — Complete public method list confirmed (stat, list, uidl, retr, dele, rset, noop, top, capa, quit, greeting, state, is_encrypted, connect*, login, apop, stls)
- `F:\My Github Repos\Open Source Repos\rust-adv-pop3\src\error.rs` — All 13 Pop3Error variants confirmed; Timeout and ConnectionClosed present
