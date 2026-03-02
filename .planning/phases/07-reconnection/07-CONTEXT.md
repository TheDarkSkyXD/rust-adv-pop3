# Phase 7: Reconnection - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Automatic reconnection with exponential backoff and jitter via Decorator pattern. `ReconnectingClient` wraps `Pop3ClientBuilder` + credentials and transparently reconnects when an I/O error is detected, while making session-state loss explicit so callers cannot accidentally re-issue DELE marks against a fresh session. The inner `Pop3Client` API is unchanged.

</domain>

<decisions>
## Implementation Decisions

### Session-state loss signaling
- Wrapper return type: every fallible method returns `Result<Outcome<T>>` where `Outcome` is an enum with `Fresh(T)` and `Reconnected(T)` variants
- Compile-time enforcement — callers must handle the reconnection case; they cannot ignore it
- `Outcome<T>` carries only the value, no metadata (attempt count, elapsed time, etc.)
- Convenience methods: `into_inner() -> T` and `is_reconnected() -> bool` for callers who want ergonomic access
- Initial connect also goes through the retry loop — if the server is temporarily down at construction time, the caller gets backoff retries from the start
- First successful command after construction returns `Fresh(T)`

### Retry scope & method coverage
- All mailbox commands are wrapped with auto-reconnect: `stat`, `list`, `uidl`, `retr`, `dele`, `rset`, `noop`, `top`, `capa`, `retr_many`, `dele_many`, `unseen_uids`, `fetch_unseen`, `prune_seen`
- `quit(self)` consumes `ReconnectingClient`, sends a best-effort QUIT to the inner client, silently succeeds if connection is already dead (no retry — intent is to disconnect)
- `login()` / `apop()` are NOT exposed on `ReconnectingClient` — auth credentials are stored at construction time and re-auth happens automatically during reconnect
- The connect() method returns an already-authenticated wrapper

### Backoff configuration
- Separate `ReconnectingClientBuilder` struct that takes a `Pop3ClientBuilder` as input — clean separation of connection config from reconnection config
- Configurable parameters with sensible defaults: `.max_retries(n)` (default 3), `.initial_delay(dur)` (default 1s), `.max_delay(dur)` (default 30s), `.jitter(bool)` (default true)
- Optional `.on_reconnect(|attempt, error| { ... })` callback for logging/metrics — informational only, cannot cancel the retry
- If caller doesn't want reconnection, they use `Pop3Client` / `Pop3ClientBuilder` directly — no pass-through mode needed

### Error classification
- **Retryable** (trigger reconnect + re-auth + retry): `Io`, `ConnectionClosed`, `Timeout`, `SysTemp`
- **Non-retryable** (propagate immediately): `AuthFailed`, `ServerError`, `MailboxInUse`, `LoginDelay`, `SysPerm`, `Parse`, `NotAuthenticated`, `InvalidInput`, `Tls`, `InvalidDnsName`
- Rationale: `Timeout` treated as connection issue (stale socket); `SysTemp` is explicitly transient per RFC 3206; `MailboxInUse`/`LoginDelay` are server-level rejections where retry won't help

### Claude's Discretion
- Which non-async accessors to expose on `ReconnectingClient` (greeting, state, is_encrypted, is_closed, supports_pipelining)
- Internal implementation of the retry loop (backon integration details)
- Whether `ReconnectingClient` lives in its own module (`reconnect.rs`) or is placed in an existing module
- `Outcome<T>` trait implementations (Debug, Clone, PartialEq, etc.)
- Documentation structure and examples

</decisions>

<specifics>
## Specific Ideas

- `ReconnectingClient` follows the Decorator pattern — wraps `Pop3ClientBuilder` internally, not `Pop3Client`
- Research already selected `backon` 1.6.0 as the retry crate (unconditional dependency per STATE.md decisions)
- API style: `ReconnectingClientBuilder::new(pop3_builder).max_retries(3).connect().await`
- The `on_reconnect` callback is optional and informational — it gives callers a hook for logging without forcing a dependency on `tracing`

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Pop3ClientBuilder` (src/builder.rs): Already `Clone` + `Debug`, stores hostname/port/timeout/tls_mode/auth — perfect for ReconnectingClient to clone and re-invoke on each reconnect
- `Pop3Error` enum (src/error.rs): Has all the variants needed for error classification — `ConnectionClosed`, `Io`, `Timeout`, `AuthFailed`, `SysTemp`, etc.
- `is_closed()` on Pop3Client and Transport: Detects when connection has dropped — useful for pre-checking before attempting commands

### Established Patterns
- Builder pattern: `Pop3ClientBuilder` uses consuming `self` methods returning `Self`, terminal `.connect().await` — ReconnectingClientBuilder should follow the same pattern
- Error handling: All methods return `Result<T, Pop3Error>` — ReconnectingClient changes this to `Result<Outcome<T>, Pop3Error>`
- Feature-gated TLS: TLS methods use `#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]` — ReconnectingClient doesn't need to worry about this since it delegates to Pop3ClientBuilder

### Integration Points
- `lib.rs` re-exports: Will need to add `pub use reconnect::{ReconnectingClient, ReconnectingClientBuilder, Outcome}`
- `Pop3ClientBuilder.connect()` is the reconnection entry point — called on each retry
- `Pop3Client.login()` / `Pop3Client.apop()` called automatically during re-auth after reconnect

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 07-reconnection*
*Context gathered: 2026-03-01*
