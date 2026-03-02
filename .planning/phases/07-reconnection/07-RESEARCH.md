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
