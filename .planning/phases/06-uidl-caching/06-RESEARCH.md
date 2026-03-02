# Phase 6: UIDL Caching - Research

**Researched:** 2026-03-01
**Domain:** Rust std collections (HashSet), async sequential iteration, Rust library API design, RFC 1939 UIDL semantics
**Confidence:** HIGH

## Summary

Phase 6 adds three tightly-coupled methods to `Pop3Client` that give callers an incremental sync API. The core work is pure Rust — no new crate dependencies required. All three requirements (CACHE-01, CACHE-02, CACHE-03) can be implemented entirely with `std::collections::HashSet` and sequential `async` for-loops already present in the codebase.

The critical design insight is that **POP3 is a strictly sequential, request-response protocol over a single TCP connection.** The `retr()` method takes `&mut self`, which means it physically cannot be called concurrently on the same client. Any "concurrent fetch" pattern using `join_all` or streams is architecturally impossible here — the callee does not satisfy `Sync`. `fetch_new()` must be a sequential for-loop that calls `self.retr(id).await?` once per unseen message.

The UIDL reconciliation algorithm (CACHE-03) is a straightforward set intersection: fetch the server's current UIDL list, build a `HashSet` of server UIDs, then remove any entry from the caller's `seen` set that is not in the server set. The library hands this mutated `HashSet` back to the caller — it never persists anything to disk (that is explicitly out of scope).

**Primary recommendation:** Implement all three requirements as methods on `Pop3Client` in `src/client.rs`. Use `&HashSet<String>` as the parameter type for `seen` — it is the most honest and readable type for this API since set-containment O(1) lookup is the core operation. No new dependencies needed.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CACHE-01 | Client provides an API to filter the UIDL list against a set of previously-seen UIDs | `uidl()` already returns `Vec<UidlEntry>`; filter with `.iter().filter(|e| !seen.contains(&e.unique_id))` |
| CACHE-02 | Client provides a `fetch_new()` convenience method returning only unseen messages | Sequential for-loop calling `self.retr(entry.message_id).await?` for each new UID; returns `Vec<Message>` |
| CACHE-03 | UIDL cache reconciliation prunes ghost entries (UIDs no longer on server) on each connect | Build server UID set from `uidl()`, call `seen.retain(|uid| server_uids.contains(uid))` |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `std::collections::HashSet` | stdlib | O(1) UID membership testing, set difference, retain-based pruning | Standard Rust collection; no dependency needed; Hash+Eq on String |
| `std::collections::HashSet` (return) | stdlib | Caller owns seen-UID persistence; library operates on caller-supplied set | Explicit in REQUIREMENTS.md: "Built-in UIDL persistence to disk" is OUT OF SCOPE |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tokio_test::io::Builder` | 0.4 (already in dev-deps) | Mock async I/O for tests of new methods | Used throughout existing tests; same pattern applies |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `&HashSet<String>` param | `impl IntoIterator<Item = impl AsRef<str>>` | Generic IntoIterator maximizes caller flexibility but makes the rustdoc and type signatures opaque; for a set-subtraction API the caller must already have a HashSet to do membership-preserving operations — accepting `&HashSet<String>` is cleaner and more honest |
| `&HashSet<String>` param | `&BTreeSet<String>` | BTreeSet preserves order, costs more on insert; UID lookup does not need order; HashSet is correct |
| `Vec<Message>` return | `Stream<Item = Result<Message>>` | Stream would require adding `futures` or `tokio-stream` as a dependency; sequential Vec is simpler, matches existing `retr()` return type, fits the "small number of new messages" use case |

**Installation:** No new dependencies. All needed types are in `std` and already-imported Tokio features.

## Architecture Patterns

### Recommended Project Structure

No new source files. All three methods land in `src/client.rs`. No changes to `src/types.rs` unless a new return type is needed (it is not — `Vec<UidlEntry>` and `Vec<Message>` already exist).

```
src/
  client.rs   <- Three new async methods here (filter_new_uids, fetch_new, reconcile_seen)
  types.rs    <- No changes needed
  lib.rs      <- No changes to public re-exports (methods are on Pop3Client which is already exported)
```

### Pattern 1: Set Difference via `contains` Filter

**What:** Filter `Vec<UidlEntry>` against a `&HashSet<String>` using iterator `.filter()`.
**When to use:** CACHE-01 — returning only unseen UIDL entries.

```rust
// Source: std::collections::HashSet docs - https://doc.rust-lang.org/std/collections/struct.HashSet.html
pub async fn filter_new_uids(
    &mut self,
    seen: &HashSet<String>,
) -> Result<Vec<UidlEntry>> {
    let all = self.uidl(None).await?;
    Ok(all
        .into_iter()
        .filter(|entry| !seen.contains(&entry.unique_id))
        .collect())
}
```

**Why not `.difference()`:** `HashSet::difference()` works on two HashSets. Here one operand is a `Vec<UidlEntry>` (the server list), not a `HashSet<String>`. The filter-with-contains approach avoids constructing an intermediate HashSet and is idiomatic for this shape of data.

### Pattern 2: Sequential Async Fetch Loop

**What:** For-loop over new UID entries, calling `self.retr()` sequentially.
**When to use:** CACHE-02 — `fetch_new()` must be sequential because `Pop3Client` is `!Sync` and `retr` takes `&mut self`.

```rust
// Source: RFC 1939 command-response sequencing; Rust ownership rules
pub async fn fetch_new(
    &mut self,
    seen: &HashSet<String>,
) -> Result<Vec<Message>> {
    let new_entries = self.filter_new_uids(seen).await?;
    let mut messages = Vec::with_capacity(new_entries.len());
    for entry in new_entries {
        let msg = self.retr(entry.message_id).await?;
        messages.push(msg);
    }
    Ok(messages)
}
```

**Why not `join_all` or streams:** `Pop3Client` takes `&mut self` — concurrent borrows are impossible. The compiler enforces sequential access. Any attempt to use concurrent future combinators would fail to compile.

### Pattern 3: Seen-Set Reconciliation via `retain`

**What:** Remove ghost UIDs from the caller's mutable `seen` set using `HashSet::retain()`.
**When to use:** CACHE-03 — prune UIDs no longer on the server after connecting/authenticating.

```rust
// Source: std::collections::HashSet::retain docs
pub async fn reconcile_seen(
    &mut self,
    seen: &mut HashSet<String>,
) -> Result<()> {
    let server_entries = self.uidl(None).await?;
    let server_uids: HashSet<&str> = server_entries
        .iter()
        .map(|e| e.unique_id.as_str())
        .collect();
    seen.retain(|uid| server_uids.contains(uid.as_str()));
    Ok(())
}
```

**Key detail:** Build `server_uids` as `HashSet<&str>` (borrowing from `server_entries`) to avoid cloning. Use `.as_str()` in `retain` closure so we can query `HashSet<&str>` from an owned `String` key — `HashSet<&str>::contains(&str)` works because `&str: Borrow<str>`.

### Anti-Patterns to Avoid

- **Concurrent RETR calls:** `Pop3Client` is `!Sync` and all methods take `&mut self`. Do not attempt `join_all`, `JoinSet`, or stream buffering on `retr()` — the compiler rejects it, and attempting to work around it with `Arc<Mutex<Pop3Client>>` would destroy the library's simplicity.
- **Storing seen UIDs inside `Pop3Client`:** The library must not own the persistence layer. Callers pass their set in; callers persist it after. Making `Pop3Client` hold an `Option<HashSet<String>>` leaks a persistence concern into the connection struct.
- **Returning `HashSet` instead of `Vec<UidlEntry>` from `filter_new_uids`:** Callers need `message_id: u32` from `UidlEntry` to call `retr()`. A `HashSet<String>` of just unique IDs would force callers to call `uidl()` again to get message numbers.
- **Ignoring the UIDL-not-supported case:** Some old servers do not implement UIDL. `uidl()` will return `Pop3Error::ServerError` in that case. The new methods should propagate this error unchanged — do not try to fall back to LIST-based approaches.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Set membership testing | Manual linear scan `Vec::contains()` | `HashSet::contains()` | O(n) vs O(1); with hundreds of UIDs, linear scan hurts |
| Ghost entry removal | Two-Vec diff loop | `HashSet::retain()` | `retain` is in-place, single-pass, idiomatic; loop approach needs temporary collection |
| Unique UID collection for reconciliation | `Vec` dedup | `HashSet::collect()` | Server may return duplicates in edge cases; HashSet deduplicates automatically |

**Key insight:** The entire phase is pure set arithmetic. The standard library's `HashSet` API handles every required operation — containment, retain, and collect. Nothing custom is needed.

## Common Pitfalls

### Pitfall 1: Calling `uidl()` Twice in `fetch_new`

**What goes wrong:** A naive implementation of `fetch_new()` calls `uidl()` directly and then calls `retr()` in a loop. If `filter_new_uids` is implemented as a separate method that also calls `uidl()`, composing them naively makes two round-trips to the server.

**Why it happens:** Not noticing that `filter_new_uids` already calls `uidl()` internally.

**How to avoid:** Implement `fetch_new()` by calling `self.filter_new_uids(seen).await?` — which calls `uidl()` once — then iterate the returned `Vec<UidlEntry>`.

**Warning signs:** Two `UIDL` commands appearing in mock I/O expectations for a single `fetch_new()` call.

### Pitfall 2: Borrowing Conflict in `reconcile_seen`

**What goes wrong:** Trying to build `server_uids: HashSet<String>` (owned) while also calling `seen.retain()` inside the same scope produces a borrow conflict if `server_entries` is bound to a lifetime that conflicts.

**Why it happens:** `HashSet<&str>` borrows from `server_entries`. If `server_entries` is dropped before `retain` completes, the borrow is invalid.

**How to avoid:** Keep `server_entries` alive for the duration of `retain`. The pattern above — binding `server_entries` in a `let`, then building `server_uids` that borrows it, then calling `retain` — is fine because all three statements are in the same scope. Alternatively, collect `server_uids` as `HashSet<String>` (clone each `unique_id`) to eliminate the lifetime dependency at the cost of one allocation.

**Warning signs:** Compiler error "does not live long enough" on `server_uids` inside `retain`.

### Pitfall 3: Off-by-One on `require_auth` State Check

**What goes wrong:** `filter_new_uids`, `fetch_new`, and `reconcile_seen` all call `uidl()` internally, which already calls `require_auth()`. If the new wrapper methods also call `require_auth()` at the top, no bug results — but if they accidentally call `uidl()` before their own auth check, the error message may be confusing.

**Why it happens:** Forgetting that `uidl()` already enforces authentication.

**How to avoid:** Do not add explicit `require_auth()` calls in the new wrapper methods — let the delegated `uidl()` and `retr()` calls handle auth enforcement.

**Warning signs:** Tests for unauthenticated access failing with the wrong error or error message.

### Pitfall 4: RFC 1939 UID Stability Edge Case — UIDL Not Guaranteed by All Servers

**What goes wrong:** The UIDL command is optional in RFC 1939 ("OPTIONAL" in the spec). Some ancient POP3 servers do not implement it. Calling `uidl()` returns `Pop3Error::ServerError("-ERR UIDL not supported")` on those servers.

**Why it happens:** Assuming UIDL is universally supported.

**How to avoid:** The library should not silently swallow this error. Document clearly in rustdoc that `filter_new_uids`, `fetch_new`, and `reconcile_seen` all require UIDL support. Consider recommending callers check `capa()` for the `UIDL` capability before calling these methods. The `Pop3Error::ServerError` propagates naturally via `?`.

**Warning signs:** Method returns error on real servers that lack UIDL; no test covers this path.

### Pitfall 5: Ghost UIDs That Are Actually Valid New Messages (UID Reuse Edge Case)

**What goes wrong:** RFC 1939 says servers SHOULD NOT reuse UIDs but does not MUST NOT. A pathological server could reuse a deleted message's UID for a new message. This would cause `reconcile_seen` to leave the old UID in `seen`, making the new message appear as already-seen.

**Why it happens:** Trusting server compliance with the SHOULD NOT reuse guideline.

**How to avoid:** The library cannot defend against this at the API level without violating its no-persistence policy. Document the limitation in rustdoc with a note that UIDL caching assumes RFC 1939 UID stability. This is the correct tradeoff — it mirrors how all real-world POP3 clients handle this edge case.

## Code Examples

Verified patterns from official sources:

### CACHE-01: filter_new_uids

```rust
// Source: std::collections::HashSet::contains - https://doc.rust-lang.org/std/collections/struct.HashSet.html
use std::collections::HashSet;

// On Pop3Client:
pub async fn filter_new_uids(
    &mut self,
    seen: &HashSet<String>,
) -> Result<Vec<UidlEntry>> {
    let all = self.uidl(None).await?;
    Ok(all
        .into_iter()
        .filter(|entry| !seen.contains(&entry.unique_id))
        .collect())
}
```

### CACHE-02: fetch_new

```rust
// Source: RFC 1939 sequential command pattern; Rust &mut self ownership
pub async fn fetch_new(
    &mut self,
    seen: &HashSet<String>,
) -> Result<Vec<Message>> {
    let new_entries = self.filter_new_uids(seen).await?;
    let mut messages = Vec::with_capacity(new_entries.len());
    for entry in new_entries {
        let msg = self.retr(entry.message_id).await?;
        messages.push(msg);
    }
    Ok(messages)
}
```

### CACHE-03: reconcile_seen

```rust
// Source: std::collections::HashSet::retain - https://doc.rust-lang.org/std/collections/struct.HashSet.html
pub async fn reconcile_seen(
    &mut self,
    seen: &mut HashSet<String>,
) -> Result<()> {
    let server_entries = self.uidl(None).await?;
    let server_uids: HashSet<&str> = server_entries
        .iter()
        .map(|e| e.unique_id.as_str())
        .collect();
    seen.retain(|uid| server_uids.contains(uid.as_str()));
    Ok(())
}
```

### Test Helper Pattern (existing pattern extended)

```rust
// Source: existing build_authenticated_test_client in src/client.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[tokio::test]
    async fn filter_new_uids_returns_only_unseen() {
        // Canned UIDL response
        let mock_data = b"+OK\r\n+OK 2 messages\r\n1 abc123\r\n2 def456\r\n.\r\n";
        let mut client = build_authenticated_test_client(mock_data);

        let seen: HashSet<String> = ["abc123".to_string()].into();
        let new = client.filter_new_uids(&seen).await.unwrap();

        assert_eq!(new.len(), 1);
        assert_eq!(new[0].unique_id, "def456");
    }

    #[tokio::test]
    async fn reconcile_seen_prunes_ghost_uid() {
        // Server only has message 2; message 1's UID is a ghost
        let mock_data = b"+OK\r\n+OK 1 messages\r\n2 def456\r\n.\r\n";
        let mut client = build_authenticated_test_client(mock_data);

        let mut seen: HashSet<String> = ["abc123".to_string(), "def456".to_string()].into();
        client.reconcile_seen(&mut seen).await.unwrap();

        assert!(!seen.contains("abc123"), "ghost uid should be pruned");
        assert!(seen.contains("def456"), "live uid should be retained");
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Caller manually calls `uidl()` + iterates + filters | Library provides `filter_new_uids(&seen)` | Phase 6 | Eliminates boilerplate; set arithmetic is hidden |
| Caller manually calls `retr()` in a loop | Library provides `fetch_new(&seen)` | Phase 6 | One-call incremental download |
| Caller manually diffs old UIDs vs new UIDL list | Library provides `reconcile_seen(&mut seen)` | Phase 6 | Ghost entries never accumulate |

**Deprecated/outdated:**
- No patterns being deprecated in this phase. These are purely additive methods.

## Open Questions

1. **Method naming: `filter_new_uids` vs `new_uids` vs `unseen_uids`**
   - What we know: CACHE-01 describes the operation conceptually; the requirement does not specify a name
   - What's unclear: The most caller-intuitive name is not obvious from the requirement text
   - Recommendation: Use `filter_new_uids` — mirrors the operation (`filter` + `new`) and reads naturally; alternatively `unseen_uids` is concise. Planner should choose one consistently for all three methods.

2. **Should `reconcile_seen` require `SessionState::Authenticated` or `SessionState::Connected`?**
   - What we know: `uidl()` requires `SessionState::Authenticated` (calls `require_auth()` internally)
   - What's unclear: The CACHE-03 requirement says "after connecting" — but UIDL is only available in TRANSACTION state (post-auth)
   - Recommendation: `reconcile_seen` is naturally constrained to Authenticated state by the delegated `uidl()` call. Document it as "call after login()". No special state check needed.

3. **Return type for `fetch_new`: `Vec<Message>` vs `Vec<(UidlEntry, Message)>`**
   - What we know: CACHE-02 says "returns full message content for only unseen messages". The requirement does not say whether the UID should be in the return value.
   - What's unclear: Callers may want to know which UID corresponds to which message to add it to their `seen` set.
   - Recommendation: Return `Vec<(UidlEntry, Message)>` — this is strictly more useful than `Vec<Message>` since callers need the `unique_id` to update their `seen` set. Returning `Message` alone forces callers to call `uidl()` again.

## Sources

### Primary (HIGH confidence)
- [std::collections::HashSet](https://doc.rust-lang.org/std/collections/struct.HashSet.html) — `contains`, `retain`, `difference` API
- [std::collections::hash_set::Difference](https://doc.rust-lang.org/std/collections/hash_set/struct.Difference.html) — set difference iterator
- [RFC 1939](https://datatracker.ietf.org/doc/html/rfc1939) — UIDL command, UID stability guarantees, SHOULD NOT reuse
- Existing `src/client.rs` — `uidl()`, `retr()`, `require_auth()`, `build_authenticated_test_client` patterns (read directly)

### Secondary (MEDIUM confidence)
- [Rust API Guidelines — Flexibility](https://rust-lang.github.io/api-guidelines/flexibility.html) — IntoIterator for generic params; verified that `&HashSet<String>` is a reasonable direct type for set-containment APIs
- [Use borrowed types for arguments — Rust Patterns](https://rust-unofficial.github.io/patterns/idioms/coercion-arguments.html) — prefer borrowed types

### Tertiary (LOW confidence)
- [imap crate Session docs](https://docs.rs/imap/latest/imap/struct.Session.html) — IMAP uses server-side flags rather than client-side UID sets; confirms POP3's design is fundamentally different and UID-set approach is correct for stateless servers

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — stdlib only; no external library choices to make
- Architecture: HIGH — `Pop3Client` ownership model (`&mut self`) forces sequential implementation; no ambiguity
- Pitfalls: HIGH — most are Rust ownership/borrow pitfalls, verifiable from first principles; UID reuse edge case is from RFC 1939 text
- API design (return types): MEDIUM — the `Vec<(UidlEntry, Message)>` vs `Vec<Message>` question is a judgment call; both are technically valid

**Research date:** 2026-03-01
**Valid until:** 2027-03-01 (stdlib APIs are stable; RFC 1939 does not change)
