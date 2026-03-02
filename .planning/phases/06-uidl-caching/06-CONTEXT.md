# Phase 6: UIDL Caching - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

UIDL-based incremental sync — three methods on `Pop3Client` that let callers retrieve only messages they haven't seen before, and automatically prune ghost UIDs from their seen set. No disk persistence (caller owns that). No new dependencies.

</domain>

<decisions>
## Implementation Decisions

### Method naming
- Method names: `unseen_uids`, `fetch_unseen`, `prune_seen`
- Parameter name: `seen` (not `seen_uids`)
- No "UIDL" in method names — the protocol command is an implementation detail
- Consistent theme: "unseen" for query/fetch methods, "seen" for the set callers manage
- Group all three methods under a `# Incremental Sync` rustdoc section heading in the impl block (first section header in the codebase)

### Return type shape
- `unseen_uids(&mut self, seen: &HashSet<String>) -> Result<Vec<UidlEntry>>` — full entries with message_id + unique_id
- `fetch_unseen(&mut self, seen: &HashSet<String>) -> Result<Vec<(UidlEntry, Message)>>` — tuple pairs UID info with message content so callers can update their seen set
- `prune_seen(&mut self, seen: &mut HashSet<String>) -> Result<Vec<String>>` — mutates set in-place AND returns the list of pruned ghost UIDs for logging/auditing
- `fetch_unseen` does NOT mutate the seen set — caller updates it themselves after processing messages (clean separation of fetching vs state management)

### Error behavior
- UIDL-not-supported: propagate the `Pop3Error::ServerError` from `uidl()` unchanged via `?`. Document the UIDL requirement in rustdoc. Callers can check `capa()` beforehand.
- `fetch_unseen` fails fast on first `retr()` error — no partial results. Simple and predictable.
- No redundant `require_auth()` calls in wrapper methods — underlying `uidl()` and `retr()` enforce auth state.
- Rustdoc for `fetch_unseen` includes a brief "Performance" note: for pipelined bulk fetching, callers can use `unseen_uids()` + `retr_many()` manually.

### Seen-set contract
- Parameter type: `&HashSet<String>` (explicit, honest about O(1) lookup requirement)
- `prune_seen` takes `&mut HashSet<String>` — mutate in place, caller keeps ownership
- Empty seen set accepted gracefully — first-time use naturally returns all messages as "unseen"
- Rustdoc includes a `serde_json` round-trip example showing how to persist/load the seen set (example only, not a dependency)

### Claude's Discretion
- Exact rustdoc wording and examples beyond the decisions above
- Test structure and coverage depth
- Whether to use `HashSet<&str>` internally for the server UID set in `prune_seen` (borrowing optimization vs simplicity)

</decisions>

<specifics>
## Specific Ideas

- "Unseen" as the consistent theme across the method family — intuitive for callers thinking about mailbox sync
- Performance doc note should explicitly mention the `unseen_uids()` + `retr_many()` escape hatch for advanced users who want pipelining
- The serde_json doc example should show a realistic flow: load seen set from file, prune, fetch unseen, update seen set, save back

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `uidl(None)` — returns `Vec<UidlEntry>` with all message UIDs; core building block for all three methods
- `retr(message_id)` — returns `Message`; used by `fetch_unseen` in sequential loop
- `require_auth()` — enforces `SessionState::Authenticated`; already called by `uidl()` and `retr()`
- `build_authenticated_test_client()` — test helper that creates a mock client in authenticated state
- `retr_many(&[u32])` — existing pipelined batch method; reference for the performance doc note

### Established Patterns
- All command methods take `&mut self` — enforces sequential access, no concurrent fetch possible
- Response parsing separated from I/O (response.rs) — new methods compose existing parsed types, no new parsing needed
- Error propagation via `?` — all methods return `Result<T>` with `Pop3Error`
- Inline `#[cfg(test)]` modules in client.rs — tests live next to the implementation

### Integration Points
- Methods added to `impl Pop3Client` in `src/client.rs` — no new files needed
- Return types `UidlEntry` and `Message` already exist in `src/types.rs` — no new types needed
- `lib.rs` re-exports `Pop3Client` — methods are automatically public

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 06-uidl-caching*
*Context gathered: 2026-03-01*
