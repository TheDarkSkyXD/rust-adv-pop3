# Phase 10: Tech Debt Cleanup - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Close all advisory gaps and tech debt items identified by the v2.0+v3.0 milestone audit. Three items: pool double-login guard, stale dead_code annotation removal, and plan reference cleanup across source files. No new features — code hygiene only.

</domain>

<decisions>
## Implementation Decisions

### Double-login guard
- Check `client.state() == SessionState::Authenticated` after `builder.connect()` in `Pop3ConnectionManager::connect()`
- If already authenticated, skip `login()` silently — no log, no error
- State check only — covers both `login()` and `apop()` auto-auth paths equally
- Keep the existing CRLF injection defense-in-depth check even when login is skipped
- Document the guard behavior in `Pop3ConnectionManager` rustdoc

### Dead code annotation removal
- Remove `#[allow(dead_code)]` from the 3 annotations that are now genuinely called: `upgrade_in_place` (line 265), `tls_handshake` rustls (line 310), `tls_handshake` openssl (line 342)
- Keep `#[allow(dead_code)]` on `Upgrading` variant (line 28) and `connect_tls` stub (line 228) — these are cfg-conditional placeholders, dead by design
- Remove/update all stale comments on changed annotations (e.g., "Used in Plan 02 — not yet called from client.rs")

### Plan reference cleanup
- Grep all `src/*.rs` files for "Plan XX" / phase artifact references and remove them
- Not scoped to transport.rs only — clean up across the entire source tree
- Phase artifacts don't belong in shipped library code

### Verification approach
- Double-login guard: run `cargo build` + `cargo clippy` to verify compilation, plus new tests (granularity at Claude's discretion)
- Dead code annotations: compile check — if it compiles without dead_code warnings, annotations were correctly stale
- Run full test suite with all feature flag combinations: `--features rustls-tls`, `--features openssl-tls`, `--features pool`, `--features mime`

### Claude's Discretion
- Test granularity for double-login guard (unit test in pool.rs vs integration-style through Pop3Pool)
- Exact wording of rustdoc additions
- Whether to clean up any other minor hygiene issues encountered during the sweep

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches. The three items are well-defined from the audit report.

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Pop3Client::state()` returns `SessionState` — use for the auth check in pool connect()
- `build_authenticated_mock_client()` in pool.rs tests — reuse for double-login guard test
- Existing `Pop3ConnectionManager` unit tests (has_broken tests) — follow same pattern for new tests

### Established Patterns
- Pool tests use `tokio_test::io::Builder` mock I/O with canned server responses
- `#[cfg(feature = "pool")]` gates pool module — tests also behind this feature gate
- Transport methods use `#[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]` for TLS-conditional code

### Integration Points
- `pool.rs:89-107` — `Pop3ConnectionManager::connect()` is the single entry point for the double-login fix
- `transport.rs` lines 265, 310, 342 — three specific annotation removal sites
- `client.rs` `stls()` method already calls `upgrade_in_place` → `tls_handshake` — proves the functions are live

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 10-tech-debt-cleanup*
*Context gathered: 2026-03-01*
