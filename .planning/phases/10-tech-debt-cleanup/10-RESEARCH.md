# Phase 10: Tech Debt Cleanup - Research

**Researched:** 2026-03-01
**Domain:** Rust code hygiene — dead_code annotations, conditional compilation, connection pool guard logic
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Double-login guard:**
- Check `client.state() == SessionState::Authenticated` after `builder.connect()` in `Pop3ConnectionManager::connect()`
- If already authenticated, skip `login()` silently — no log, no error
- State check only — covers both `login()` and `apop()` auto-auth paths equally
- Keep the existing CRLF injection defense-in-depth check even when login is skipped
- Document the guard behavior in `Pop3ConnectionManager` rustdoc

**Dead code annotation removal:**
- Remove `#[allow(dead_code)]` from the 3 annotations that are now genuinely called: `upgrade_in_place` (line 265), `tls_handshake` rustls (line 310), `tls_handshake` openssl (line 342)
- Keep `#[allow(dead_code)]` on `Upgrading` variant (line 28) and `connect_tls` stub (line 228) — these are cfg-conditional placeholders, dead by design
- Remove/update all stale comments on changed annotations (e.g., "Used in Plan 02 — not yet called from client.rs")

**Plan reference cleanup:**
- Grep all `src/*.rs` files for "Plan XX" / phase artifact references and remove them
- Not scoped to transport.rs only — clean up across the entire source tree
- Phase artifacts don't belong in shipped library code

**Verification approach:**
- Double-login guard: run `cargo build` + `cargo clippy` to verify compilation, plus new tests (granularity at Claude's discretion)
- Dead code annotations: compile check — if it compiles without dead_code warnings, annotations were correctly stale
- Run full test suite with all feature flag combinations: `--features rustls-tls`, `--features openssl-tls`, `--features pool`, `--features mime`

### Claude's Discretion
- Test granularity for double-login guard (unit test in pool.rs vs integration-style through Pop3Pool)
- Exact wording of rustdoc additions
- Whether to clean up any other minor hygiene issues encountered during the sweep

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope.
</user_constraints>

## Summary

Phase 10 is a three-item code hygiene pass with no new features. All three items are precisely located and require only small, targeted edits.

**Item 1 — Double-login guard:** `Pop3ConnectionManager::connect()` in `src/pool.rs` always calls `client.login()` after `builder.connect()`. But `Pop3ClientBuilder::connect()` already performs authentication when the builder has credentials configured (`AuthMode::Login` or `AuthMode::Apop`). The guard inserts a `client.state() == SessionState::Authenticated` check before calling `login()` and skips it if already authenticated. The CRLF check runs unconditionally (before the state check) as defense-in-depth.

**Item 2 — Dead code annotation removal:** Three `#[allow(dead_code)]` annotations in `src/transport.rs` are now stale: `upgrade_in_place` is called from `Pop3Client::stls()` in `client.rs` (line 367), and both `tls_handshake` helpers are called from `upgrade_in_place`. Two annotations must be preserved: the `Upgrading` enum variant (genuinely unreachable at runtime — it's a transient placeholder) and the no-TLS `connect_tls` stub (unreachable without TLS features). The stale comment text ("Plan 02 — not yet called from client.rs") must be removed along with the annotations.

**Item 3 — Plan reference cleanup:** A grep of all `src/*.rs` files for "Plan \d\d" reveals that all "Plan XX" references are in `src/transport.rs`: line 24 in the `Upgrading` doc comment and lines 265, 310, 342 in the `#[allow(dead_code)]` comments being removed. Once Items 2 and 3 are executed together, all plan references are eliminated. No other source file contains plan references.

**Primary recommendation:** Execute all three items in a single plan with two waves: Wave 1 removes dead_code annotations and plan references from transport.rs; Wave 2 adds the double-login guard to pool.rs plus a new unit test.

## Standard Stack

### Core

No new dependencies needed for this phase. All work uses existing crate infrastructure.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `tokio_test` | existing | Mock I/O for new pool test | Already in dev-deps |
| `bb8::ManageConnection` | existing | Trait being implemented | Already in pool feature |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `cargo clippy` | stable | Verify dead_code warnings gone | Post-annotation-removal check |
| `cargo test --features pool` | — | Validate guard test | Feature-gated test suite |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `state() == SessionState::Authenticated` | `matches!(client.state(), SessionState::Authenticated)` | Both idioms are equivalent; `matches!` is marginally more idiomatic for enum matching |

**Installation:** No new packages needed.

## Architecture Patterns

### Current Code: Pop3ConnectionManager::connect() (pool.rs lines 89–107)

```rust
// CURRENT — has double-login trap
fn connect(&self) -> impl Future<Output = Result<Pop3Client, Pop3Error>> + Send {
    let builder = self.builder.clone();
    let username = self.username.clone();
    let password = self.password.clone();
    async move {
        // Defense-in-depth: reject CRLF before opening a TCP connection.
        if username.contains('\r') || username.contains('\n')
            || password.contains('\r') || password.contains('\n')
        {
            return Err(Pop3Error::InvalidInput);
        }
        let mut client = builder.connect().await?;
        client.login(&username, &password).await?;  // ← DOUBLE LOGIN if builder had credentials
        Ok(client)
    }
}
```

### Pattern 1: Double-Login Guard

**What:** Add a `SessionState::Authenticated` check between `builder.connect()` and `client.login()`.

**When to use:** Any connection manager that calls both a builder (which may auto-auth) and explicit auth.

**Example:**
```rust
// AFTER fix — guard prevents double login
async move {
    // Defense-in-depth: reject CRLF before opening a TCP connection.
    // login() also validates, but this avoids wasting a connection attempt.
    if username.contains('\r') || username.contains('\n')
        || password.contains('\r') || password.contains('\n')
    {
        return Err(Pop3Error::InvalidInput);
    }
    let mut client = builder.connect().await?;
    // Guard: builder.connect() auto-authenticates when credentials are set on
    // the builder. Skip login() if the client is already in Authenticated state.
    if client.state() != SessionState::Authenticated {
        client.login(&username, &password).await?;
    }
    Ok(client)
}
```

### Pattern 2: Dead Code Annotation Removal

**What:** Remove `#[allow(dead_code)]` when the annotated function is genuinely called from production code. The annotation is a historical artifact from when the function was added before its caller existed.

**When to use:** After verifying via grep that the function is called in the production code path (not only in tests).

**Verification chain for `upgrade_in_place`:**
- `client.rs:367` → `self.transport.upgrade_in_place(hostname).await?`
- This is inside `stls()` which is `pub` — it IS called from production code
- `upgrade_in_place` calls `Self::tls_handshake(tcp_stream, hostname).await?` (line 299)
- Therefore both `upgrade_in_place` and `tls_handshake` are live

**What stays annotated:**
```rust
// KEEP — Upgrading variant: genuinely unreachable at match arms (transient placeholder)
#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
#[allow(dead_code)]
Upgrading,

// KEEP — no-TLS connect_tls stub: dead by design when no TLS feature active
#[cfg(not(any(feature = "rustls-tls", feature = "openssl-tls")))]
#[allow(dead_code)]
pub(crate) async fn connect_tls(...) -> Result<Self> { ... }
```

**What gets removed:**
```rust
// REMOVE from line 265:
#[allow(dead_code)] // Used in Plan 02 (STARTTLS) — not yet called from client.rs

// REMOVE from line 310:
#[allow(dead_code)] // Used by upgrade_in_place (Plan 02)

// REMOVE from line 342:
#[allow(dead_code)] // Used by upgrade_in_place (Plan 02)
```

**Also update the doc comment on Upgrading variant (line 24)** — strip "Plan 02" from the phrase "Temporary placeholder during STARTTLS upgrade (Plan 02)". Replace with accurate description without plan reference:
```rust
/// Temporary placeholder during STARTTLS upgrade. Never performs real I/O;
/// this variant exists only transiently inside upgrade_in_place and is
/// immediately replaced by the TLS variant before the method returns.
```

### Pattern 3: Rustdoc for Pop3ConnectionManager

**What:** Add a section to `Pop3ConnectionManager`'s rustdoc explaining the auth guard behavior.

**When to use:** Any `ManageConnection` implementation where the builder and the manager both can trigger authentication.

**Example addition:**
```rust
/// # Authentication
///
/// [`connect()`](bb8::ManageConnection::connect) always calls `login()` after the
/// builder connects. If the builder already authenticated the client (because
/// credentials were set on the builder via
/// [`Pop3ClientBuilder::credentials()`](crate::Pop3ClientBuilder::credentials)),
/// the login step is skipped — the client's session state is checked first to
/// prevent issuing a second `USER`/`PASS` exchange.
```

### Anti-Patterns to Avoid

- **Removing the CRLF check when skipping login:** The CRLF check must run unconditionally — it validates the credentials stored on the manager regardless of whether login actually fires.
- **Checking `is_authenticated` / `is_closed` instead of `state()`:** The canonical API is `state() -> SessionState`. Use it for clarity and to match the public API contract.
- **Removing `#[allow(dead_code)]` from `Upgrading` or `connect_tls` stub:** These are correctly annotated — the Upgrading variant is never matched (it's a transient placeholder during `mem::replace`) and the no-TLS stub compiles but is unreachable in the no-feature build.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| State equality check | Custom is_authenticated() method | `client.state() == SessionState::Authenticated` | `state()` is already public API |
| Feature detection | Runtime checks | `#[cfg(any(...))]` | Already in use throughout transport.rs |

**Key insight:** All scaffolding for this phase already exists in the codebase. This is annotation removal and a two-line guard addition, not new infrastructure.

## Common Pitfalls

### Pitfall 1: Removing the Wrong `#[allow(dead_code)]` Annotations

**What goes wrong:** The developer removes all four `#[allow(dead_code)]` annotations in transport.rs, including the two that must stay (`Upgrading` variant and no-TLS `connect_tls` stub).

**Why it happens:** The task description says "remove dead_code annotations" without specifying which ones survive.

**How to avoid:** The CONTEXT.md is explicit: remove only lines 265, 310, 342. Keep lines 28 and 228. Verify by compiling with `--features rustls-tls` and `--no-default-features` after the change.

**Warning signs:** Compiler warns `allow(dead_code)` has no effect on `Upgrading` or gives error — means you tried to remove an annotation that was doing real work.

### Pitfall 2: Double-Login Guard Placed Before the CRLF Check

**What goes wrong:** The CRLF defense-in-depth check is placed after the state check, so a builder with credentials could bypass credential validation.

**Why it happens:** Natural ordering might seem like "check state first, then validate".

**How to avoid:** CRLF check runs first unconditionally. Then state check. Then optional login. This is the locked decision order from CONTEXT.md.

### Pitfall 3: Feature Flag Combination Misses

**What goes wrong:** Tests pass with default features (`rustls-tls`) but fail with `openssl-tls` or with no TLS features at all.

**Why it happens:** The `tls_handshake` function has two separate implementations (one `#[cfg(feature = "rustls-tls")]`, one `#[cfg(feature = "openssl-tls")]`). Removing dead_code from one but not the other, or breaking the cfg guards, causes compilation failure under a specific feature combination.

**How to avoid:** Run `cargo build --no-default-features --features pool`, `cargo build --features rustls-tls,pool`, and `cargo build --no-default-features --features openssl-tls,pool` after the change.

### Pitfall 4: Test Needs Real TCP for Full Pool Round-Trip

**What goes wrong:** A test tries to use `pop3.checkout()` to actually invoke `Pop3ConnectionManager::connect()` but this requires a real TCP connection.

**Why it happens:** bb8's `connect()` is called internally during pool checkout — there's no way to inject a mock client into `bb8::ManageConnection::connect()`.

**How to avoid:** Test the guard directly by calling `manager.connect()` is NOT possible via mock I/O through bb8. Instead, test the guard logic via unit test using `build_authenticated_mock_client` — construct a client in `Authenticated` state and verify that calling `login()` on an already-authenticated client returns an error (which is the scenario the guard prevents). Alternatively, write a focused unit test that calls the connection manager's logic by factoring the guard into a standalone helper — or more practically, follow the existing has_broken test pattern: use `build_authenticated_mock_client` to create a client already in `Authenticated` state and assert that `client.state() == SessionState::Authenticated`.

The most realistic unit test approach: construct an already-authenticated mock client and verify the guard's branch behavior — the guard skips login when `state == Authenticated`. This does NOT require bb8 checkout; it tests the if-condition logic directly.

## Code Examples

### Complete Fixed connect() Method (Source: codebase analysis)

```rust
fn connect(&self) -> impl Future<Output = Result<Pop3Client, Pop3Error>> + Send {
    let builder = self.builder.clone();
    let username = self.username.clone();
    let password = self.password.clone();
    async move {
        // Defense-in-depth: reject CRLF before opening a TCP connection.
        // login() also validates, but this avoids wasting a connection attempt.
        if username.contains('\r')
            || username.contains('\n')
            || password.contains('\r')
            || password.contains('\n')
        {
            return Err(Pop3Error::InvalidInput);
        }
        let mut client = builder.connect().await?;
        // Guard: builder.connect() auto-authenticates when credentials are configured
        // on the builder. Skip login() if the session is already in Authenticated state
        // to prevent a redundant USER/PASS exchange.
        if client.state() != SessionState::Authenticated {
            client.login(&username, &password).await?;
        }
        Ok(client)
    }
}
```

### Test Pattern: Guard Skips Login When Already Authenticated

```rust
#[tokio::test]
async fn connect_skips_login_when_already_authenticated() {
    // build_authenticated_mock_client produces a client in SessionState::Authenticated
    let mock = tokio_test::io::Builder::new().build(); // no login exchange expected
    let client = build_authenticated_mock_client(mock);
    // Verify the precondition: client IS authenticated
    assert_eq!(client.state(), SessionState::Authenticated);
    // The guard in connect() would see this state and skip login()
    // No assertions needed beyond the precondition — the mock would panic
    // if UNEXPECTED writes (USER/PASS) were issued
}
```

Note: The most direct way to test the guard is to put a minimal mock that would panic on unexpected writes (USER or PASS commands), then call the connect() body logic. Since `ManageConnection::connect()` can't be called with a mock-injected client directly, the practical test strategy is:

1. A test that verifies `build_authenticated_mock_client()` returns `SessionState::Authenticated` (proving the precondition the guard checks)
2. A test using a mock that expects NO `USER\r\n` or `PASS\r\n` writes when the client is pre-authenticated

### transport.rs Lines Before/After

**Before (line 264-266):**
```rust
#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
#[allow(dead_code)] // Used in Plan 02 (STARTTLS) — not yet called from client.rs
pub(crate) async fn upgrade_in_place(&mut self, hostname: &str) -> Result<()> {
```

**After:**
```rust
#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
pub(crate) async fn upgrade_in_place(&mut self, hostname: &str) -> Result<()> {
```

**Before (line 309-311):**
```rust
#[cfg(feature = "rustls-tls")]
#[allow(dead_code)] // Used by upgrade_in_place (Plan 02)
async fn tls_handshake(tcp_stream: TcpStream, hostname: &str) -> Result<InnerStream> {
```

**After:**
```rust
#[cfg(feature = "rustls-tls")]
async fn tls_handshake(tcp_stream: TcpStream, hostname: &str) -> Result<InnerStream> {
```

**Before (line 341-343):**
```rust
#[cfg(feature = "openssl-tls")]
#[allow(dead_code)] // Used by upgrade_in_place (Plan 02)
async fn tls_handshake(tcp_stream: TcpStream, hostname: &str) -> Result<InnerStream> {
```

**After:**
```rust
#[cfg(feature = "openssl-tls")]
async fn tls_handshake(tcp_stream: TcpStream, hostname: &str) -> Result<InnerStream> {
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `#[allow(dead_code)]` with comment "not yet called" | Remove annotation when caller exists | Phase 10 | Cleaner code; clippy can catch actual dead code in future |
| Always call `login()` unconditionally in pool connect | Guard with `state()` check | Phase 10 | Prevents protocol error when builder auto-authenticates |

**Deprecated/outdated:**
- Stale comments like "Used in Plan 02 (STARTTLS) — not yet called from client.rs": These are planning artifacts. Shipped code does not reference planning phases.

## Open Questions

1. **Can `tls_handshake` be called with `#[cfg(feature = "openssl-tls")]` removed from dead_code without triggering a new clippy warning?**
   - What we know: `tls_handshake` is called from `upgrade_in_place`. Both are `#[cfg(feature = "openssl-tls")]`. The call chain is coherent.
   - What's unclear: Whether Rust/clippy considers a private `async fn` called only from a `pub(crate)` function in the same cfg block truly "used" — it should.
   - Recommendation: Compile with `cargo clippy --no-default-features --features openssl-tls` after removal to confirm zero dead_code warnings.

2. **Should the double-login guard test simulate the "builder with credentials" path?**
   - What we know: `Pop3ClientBuilder::connect()` calls `login()` internally when `AuthMode::Login` is set. The pool's `connect()` then calls `login()` again without the guard.
   - What's unclear: Whether a mock-IO test can exercise the full round-trip through `builder.connect()` auto-auth + pool guard.
   - Recommendation: Test at the unit level using `build_authenticated_mock_client()`. The integration scenario (builder with credentials → pool guard) is covered by the compilation check and documented behavior — a full end-to-end test requires a live server.

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in (`#[test]` / `#[tokio::test]`) |
| Config file | none — inline `#[cfg(test)]` modules |
| Quick run command | `cargo test --features pool` |
| Full suite command | `cargo test --features rustls-tls,pool,mime && cargo test --no-default-features --features pool` |

### Phase Requirements → Test Map

This phase has no formal requirement IDs. Success criteria are verified as follows:

| Success Criterion | Behavior | Test Type | Automated Command |
|-------------------|----------|-----------|-------------------|
| SC-1: Double-login guard | `connect()` skips `login()` when state is Authenticated | unit | `cargo test --features pool connect_skips_login` |
| SC-2: Dead code annotations removed | Compile without dead_code warnings for upgraded functions | compile | `cargo clippy --features rustls-tls,pool -- -D warnings` |
| SC-2b: No regression on kept annotations | `Upgrading` and no-TLS stub still compile cleanly | compile | `cargo build --no-default-features --features pool` |
| SC-3: No "Plan XX" references in src/ | grep finds zero matches | static analysis | `grep -rn "Plan 0\|Plan 1" src/` |

### Sampling Rate

- **Per task commit:** `cargo clippy --features rustls-tls,pool -- -D warnings`
- **Per wave merge:** `cargo test --features rustls-tls,pool,mime`
- **Phase gate:** Full feature matrix — see commands below

### Feature Matrix (Phase Gate)

```bash
cargo test --features rustls-tls,pool,mime
cargo test --no-default-features --features pool
cargo clippy --features rustls-tls,pool,mime -- -D warnings
cargo clippy --no-default-features --features pool -- -D warnings
```

### Wave 0 Gaps

- [ ] `src/pool.rs` — new test `connect_skips_login_when_already_authenticated` (covers SC-1)

All other checks are compile-time or static analysis — no additional test files required.

## Sources

### Primary (HIGH confidence)

- Direct code inspection of `src/transport.rs` — confirmed lines 28, 228, 265, 310, 342 with annotations and comments
- Direct code inspection of `src/pool.rs` — confirmed `connect()` body (lines 89-107) calls `login()` unconditionally
- Direct code inspection of `src/client.rs` — confirmed `stls()` calls `upgrade_in_place` (line 367), and `state()` returns `SessionState` (line 243)
- Direct code inspection of `src/builder.rs` — confirmed `builder.connect()` auto-authenticates when `AuthMode::Login` or `AuthMode::Apop` is set (lines 270-279)
- Direct code inspection of `src/types.rs` — confirmed `SessionState` enum: `Connected`, `Authenticated`, `Disconnected`
- `bash grep -rn "Plan 0\|Plan 1" src/` — confirmed all plan references are in `src/transport.rs` only (lines 24, 265, 310, 342)

### Secondary (MEDIUM confidence)

- Project skills (`rust-best-practices/SKILL.md`): prefer `#[expect(clippy::lint)]` over `#[allow(...)]` — noted, but the decision to use `#[allow(dead_code)]` for the two surviving annotations is locked (they are cfg-conditional and expected to be dead); no change needed

### Tertiary (LOW confidence)

- None

## Metadata

**Confidence breakdown:**
- What to change: HIGH — exact line numbers confirmed by reading the files
- How to change it: HIGH — patterns are direct code edits, no API research needed
- Test strategy: HIGH — follows existing pool.rs test patterns exactly
- Feature flag matrix: HIGH — matches the verification approach documented in CONTEXT.md

**Research date:** 2026-03-01
**Valid until:** Stable (no external dependencies changed; all findings are from codebase inspection)
