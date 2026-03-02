# Phase 8: Connection Pooling - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Provide a `Pop3Pool` that manages multiple POP3 accounts concurrently using bb8, enforcing RFC 1939's one-connection-per-mailbox constraint at the type level and in documentation. The pool is additive — a new module with a feature flag, sitting above the existing `Pop3Client` without modifying it.

</domain>

<decisions>
## Implementation Decisions

### Account Identity Model
- Use `Pop3ClientBuilder` as the account registration unit — callers pass a builder with connection info and credentials already set
- Internal key derived from (host, port, username) extracted from the builder — no new `AccountKey` type needed unless implementation demands it
- Credentials bundled at registration time (via `.credentials()` or `.apop()` on the builder) — checkout returns a ready-to-use authenticated connection
- Accounts can be added dynamically at any time via `pool.add_account(builder)` — no upfront-only restriction

### Pool Checkout API
- bb8's `PooledConnection<Pop3ConnectionManager>` returned on checkout — standard RAII, auto-returns to pool on drop
- Busy checkout blocks with a configurable timeout (bb8's `connection_timeout`) — natural async behavior, no immediate-error mode
- Pool gated behind a `pool` feature flag — adds bb8 dependency only when needed, keeps base crate lean for single-client users

### Connection Lifecycle
- NOOP probe on checkout via bb8's `test_on_check_out(true)` — guarantees callers get a live connection
- Pool manages raw `Pop3Client` connections — does NOT wrap with `ReconnectingClient` from Phase 7 (orthogonal layers; bb8 handles discard-and-recreate)
- `Pop3ConnectionManager` implements bb8's `ManageConnection` trait: `connect()` builds + authenticates via the stored builder, `is_valid()` sends NOOP, `has_broken()` checks `is_closed()`

### Error Model
- Granular error variants — callers can distinguish checkout timeout, connection failure, authentication failure programmatically
- thiserror with `#[from]` / `#[source]` for source chaining — consistent with existing `Pop3Error` pattern
- Error type wraps underlying `Pop3Error` for client-level failures

### Claude's Discretion
- Whether to expose bb8 types directly or wrap behind our own types (e.g., type alias vs newtype)
- Whether to implement `remove_account()` for deprovisioning
- Idle timeout and max lifetime defaults for pooled connections
- Failed connection handling on return (bb8 auto-discard vs explicit broken flag)
- Whether error type is a new `Pop3PoolError` enum or extensions to `Pop3Error` (aligned with feature flag boundary)

</decisions>

<specifics>
## Specific Ideas

- The pool is a registry of per-account bb8 pools, each with `max_size(1)` — not a traditional N-connection pool to one server
- Research already resolved the concurrency pattern: `tokio::sync::RwLock<HashMap>` with double-checked locking, `build_unchecked()` for synchronous pool creation inside the lock, return canonical pool not local candidate
- `min_idle(None)` is mandatory — connections created lazily on first checkout, preventing RFC 1939 violations during pool construction race conditions

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Pop3ClientBuilder` (src/builder.rs): Already `Clone`, has all connection info + credentials + TLS mode. Perfect as the pool's account registration unit
- `Pop3Client` (src/client.rs): Has `is_closed() -> bool` for health checking and `noop()` for liveness probes
- `Pop3Error` (src/error.rs): thiserror-derived enum with `ConnectionClosed`, `Timeout`, `AuthFailed` variants — pool errors can wrap these
- `ReconnectingClient` (src/reconnect.rs): Existing decorator pattern — pool is a separate concern, not layered on top

### Established Patterns
- Feature flags: `rustls-tls` / `openssl-tls` with `compile_error!` for mutual exclusion — `pool` flag follows same pattern
- Module structure: Each major feature is its own module (builder.rs, reconnect.rs) with re-exports in lib.rs
- thiserror for all error types with source chaining

### Integration Points
- `lib.rs`: Will add `pub mod pool` (gated behind `#[cfg(feature = "pool")]`) and re-export `Pop3Pool`, `Pop3ConnectionManager`, `Pop3PoolError`/error types
- `Cargo.toml`: Add `bb8` as optional dependency gated behind `pool` feature
- `Pop3ClientBuilder::connect()`: The connection manager calls this internally to create + authenticate connections

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 08-connection-pooling*
*Context gathered: 2026-03-01*
