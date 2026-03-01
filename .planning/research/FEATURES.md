# Feature Research

**Domain:** Async POP3 client library — v3.0 advanced features
**Researched:** 2026-03-01
**Confidence:** HIGH (RFC 2449 authoritative; tokio patterns from official docs; crate versions from docs.rs; POP3 locking from RFC 1939 + real server reports)

---

## Context: v3.0 is a Subsequent Milestone

v2.0 (phases 1–4) delivers the async rewrite with full RFC 1939 coverage, dual TLS backends, CAPA, STARTTLS, RESP-CODES, APOP, and builder API. v3.0 layers advanced features on top of that stable async foundation. The question is: which of the five requested features are table stakes, which are genuine differentiators, and which are anti-features masquerading as reasonable requests?

**Existing v2.0 foundation assumed available:**
- `async fn` methods returning `Result<T, Pop3Error>`
- `tokio::io::BufReader` split into reader/writer halves
- `AsyncStream` enum for TLS backend abstraction
- `Pop3Error` typed enum with `thiserror`
- `SessionState` enum (Authorization / Transaction / Update)
- CAPA command returning server capability set
- Full RFC 1939 command set

---

## Feature Landscape

### Table Stakes (Users Expect These)

These are features a user filing GitHub issue #2 ("advanced features") explicitly expects — they represent the _minimum_ that makes v3.0 a credible upgrade over v2.0 for production high-volume use.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| POP3 command pipelining (RFC 2449) | High-volume mail processors need batch throughput; RFC 2449 defines PIPELINING as a standard capability; MailKit (the most widely used cross-platform mail library) uses it as its primary performance optimization for bulk operations; missing it leaves throughput on the table for the only real production use case for POP3 in 2026 | HIGH | Server must advertise `PIPELINING` in CAPA response. Client sends multiple commands (e.g., all RETR + DELE for a batch) without waiting for each response, then reads responses in order. Requires an in-order pending-command queue (`VecDeque<CommandType>`). Must fall back to sequential mode if server does not advertise PIPELINING. Cannot be used without the CAPA check already implemented in v2.0. |
| UIDL caching for incremental sync | Anyone using POP3 programmatically to process new mail needs to avoid re-processing messages seen in previous sessions; UIDL is already in v2.0; caching the seen set is the obvious and expected next step; without it, every session re-downloads everything | MEDIUM | Caller-provided or library-managed `HashSet<String>` of seen UIDLs. Callers compare remote UIDL list against seen set to identify new messages. The library should provide the mechanism (API to filter/track), not the persistence (where to store it — that's the caller's concern). Persistence strategy (file, serde_json, sled, SQLite) belongs in the caller's application layer, not in a transport library. |
| Automatic reconnection with exponential backoff | Network connections drop; POP3 sessions over cellular or VPN connections commonly experience mid-session failures; a production mail processor needs resilience without writing a custom retry loop | MEDIUM | Library provides a `reconnect_with_backoff(config)` method or wraps the client in a `ReconnectingClient` type. Uses `backon` (v1.6.0, actively maintained, tokio-native) or manual `tokio::time::sleep`. Configuration: initial delay, multiplier, max delay, max attempts, jitter flag. CRITICAL constraint: automatic reconnection MUST NOT silently re-apply pending DELE marks — session state is reset on reconnect; caller must be notified via error or callback so they can decide whether to re-issue deletes. |

### Differentiators (Competitive Advantage)

Features that distinguish this library from `async-pop` and competing crates, and make it the first POP3 library to deliver production-quality advanced features in the Rust ecosystem.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Optional MIME parsing integration via feature flag | After RETR, users almost always need to parse the raw RFC 5322 message into structured headers, body parts, and attachments; without this, every user writes the same `mailparse::parse_mail(&raw_bytes)` boilerplate; providing it behind a feature flag adds ergonomic value without forcing the dependency | LOW | Feature flag: `mime` (or `mailparse`). When active, adds convenience methods `client.retr_parsed(n) -> Result<ParsedMail>` and `client.retr_parsed_mail_parser(n) -> Result<Message>` that call RETR then parse. Both `mailparse` (v0.16.1, synchronous, 0BSD) and `mail-parser` (v0.11.2, synchronous, zero-copy `Cow<str>`, no external dependencies, full RFC 5322 + MIME) are candidate back-ends. **Recommendation: `mail-parser` behind a `mime` feature flag** — it has no external dependencies, is zero-copy, conforms to more RFCs, and is actively maintained by Stalwart Labs. `mailparse` is simpler but has more dependencies (charset, data-encoding, quoted_printable). Neither crate is async; call parse after RETR without spawning a blocking task — POP3 message parsing is not CPU-bound enough to warrant `spawn_blocking`. |
| Pipelining with automatic PIPELINING capability detection | A naïve pipelining implementation requires callers to check CAPA themselves; automatically detecting and enabling pipelining (using the already-implemented CAPA command from v2.0) makes the optimization transparent | LOW | `connect()` already reads the server greeting. After authentication, auto-call CAPA and cache the capability set including PIPELINING. Expose `client.supports_pipelining() -> bool`. Batch methods like `client.retr_many(&[u32]) -> Vec<Result<String>>` automatically pipeline when supported, fall back to sequential otherwise. This is a differentiator because no other Rust POP3 crate does this. |
| Incremental sync helper returning only new messages | Combine UIDL + caller-supplied seen set to return only truly new messages in one method call, eliminating the boilerplate every caller writes | LOW | `client.fetch_new(seen: &HashSet<String>) -> Result<Vec<(String, String)>>` — returns `(uid, message_body)` for messages not in `seen`. Caller updates their seen set and persists it. Complexity is LOW because UIDL is already implemented; this is a composition method. |

### Anti-Features (Commonly Requested, Often Problematic)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Connection pooling | High-throughput scenarios; re-using connections across parallel workers seems efficient | RFC 1939 mandates an exclusive-access lock on the mailbox for each POP3 session. Servers return `-ERR [IN-USE]` when a second connection attempts to open the same mailbox. This is not a server implementation detail — it is protocol-mandated behavior. A pool of connections to the same mailbox is therefore impossible; connections to different mailboxes (one connection per mailbox) are trivially managed by the caller creating N `Client` instances. Building a pool in the library would create a false abstraction that breaks at runtime. | Caller creates one `Client` per mailbox. If they have 10 mailboxes, they create 10 clients and use `tokio::task::spawn` to handle them concurrently. Document this pattern explicitly. |
| Transparent auto-reconnect that re-issues pending deletes | Users want "fire and forget" resilience — if the connection drops mid-session, they want the library to silently reconnect and pick up where it left off | POP3 DELE marks are not committed until QUIT triggers the UPDATE state. A reconnected session starts in a fresh AUTHORIZATION state — no DELE marks survive. If the library silently re-connects and re-issues deletes, it must re-identify which messages to delete (requiring message re-download or external state). If it does NOT re-issue deletes, messages the user thought were deleted remain. Either way, hidden reconnection creates invisible data inconsistency. The caller must own this decision. | Expose reconnection as explicit API (`reconnect_with_backoff`). Return an error when the connection drops. Provide a `ReconnectResult` that informs callers of the new session state. Callers re-issue their own deletes with full awareness. |
| Built-in UIDL persistence to disk | Users want the library to automatically save the seen UIDL set to a file between sessions | Adding file I/O, serialization format choices (JSON vs bincode vs custom), error handling for disk failures, and platform path conventions to a transport library violates single-responsibility. Different applications need different persistence strategies (file, database, cloud KV store). Bundling one forces every user to accept it or work around it. | Provide the `HashSet<String>` API; document a one-paragraph example showing `serde_json` to persist it. The caller owns persistence; the library provides the data. |
| Async MIME parsing | Users expect parsing to be async since the rest of the library is async | Email message parsing is CPU-bound parsing of an in-memory `&[u8]` or `String`. There is no I/O. Running it on the async thread is fine — it completes in microseconds even for large messages. Wrapping in `spawn_blocking` adds overhead and complexity for zero real-world benefit. Both `mailparse` and `mail-parser` are synchronous by design and this is correct for the use case. | Call parse synchronously inside the async method after RETR completes. No `spawn_blocking`. |
| SASL authentication in v3.0 | Users want SASL PLAIN or XOAUTH2 for modern provider compatibility | SASL PLAIN is semantically identical to USER/PASS over TLS — already implemented. XOAUTH2 requires external OAuth2 token flows (HTTP client, token refresh, provider-specific endpoints) that are completely out of scope for a transport library. Gmail ended POP3 support for third-party OAuth in 2026; Outlook's POP3 + OAuth flow is not part of the RFC. | SASL PLAIN can be a thin v4.0 addition if user demand emerges. Document that XOAUTH2 is deliberately excluded. |

---

## Feature Dependencies

```
[v2.0 CAPA command]
    └──required by──> [Pipelining — capability detection]
    └──required by──> [Pipelining — auto-detect and enable]

[v2.0 UIDL command]
    └──required by──> [UIDL caching for incremental sync]
    └──required by──> [Incremental sync helper (fetch_new)]

[v2.0 RETR command]
    └──required by──> [MIME integration (retr_parsed)]
    └──required by──> [Pipelining — RETR batching]

[v2.0 typed Pop3Error + async fn skeleton]
    └──required by──> [Automatic reconnection — error propagation]
    └──required by──> [Pipelining — error handling per-command in queue]

[Pipelining — VecDeque pending queue]
    └──enables──> [Bulk RETR batching]
    └──enables──> [Bulk DELE batching]
    └──conflicts with──> [Connection pooling] (pooling is impossible; pipelining is the correct throughput mechanism)

[UIDL caching mechanism]
    └──enhances──> [Incremental sync helper]
    └──independent of──> [Pipelining] (orthogonal features; can be combined)

[Automatic reconnection]
    └──independent of──> [Pipelining] (reconnection re-establishes the session; pipelining operates within a session)
    └──requires──> [backon or manual backoff] (new dependency if using backon crate)
    └──conflicts with──> [Transparent auto-reconnect with silent DELE re-issue] (do not implement the anti-feature)

[MIME integration (feature flag)]
    └──requires──> [mail-parser OR mailparse as optional dependency]
    └──requires──> [RETR returning raw message bytes/String]
    └──independent of──> [Pipelining, UIDL caching, reconnection]
```

### Dependency Notes

- **Pipelining requires CAPA (from v2.0):** The client must check that the server advertises `PIPELINING` before sending unbatched commands. Sending pipelined commands to a non-pipelining server results in the server reading the second command as message content, corrupting the session. CAPA must be called after authentication and the capability set cached.
- **UIDL caching requires UIDL (from v2.0):** The UIDL command returns the per-message unique identifier stable across sessions. The caching mechanism is a pure composition on top of this — no new protocol work.
- **Automatic reconnection must NOT be transparent:** The v2.0 DELE semantics (marks cleared on disconnect, committed only at QUIT) make transparent reconnection dangerous. The library must surface the disconnect error to the caller and let them decide whether and how to reconnect.
- **MIME integration is purely optional:** It adds a convenience method and a feature-gated dependency. It does not interact with the protocol layer. It can be implemented in a single function `fn retr_and_parse(...)`.
- **Connection pooling is architecturally incompatible with POP3:** RFC 1939 Section 8 states the server MUST acquire an exclusive-access lock on the maildrop and return `-ERR` if the lock cannot be acquired. This is protocol-mandated, not optional behavior. No implementation choice by this library can work around it.

---

## MVP Definition

v3.0 is a subsequent milestone, not a greenfield product. The "MVP" here is the minimum scope that justifies a v3.0.0 semver-major release with meaningful improvement over v2.x.

### Launch With (v3.0)

- [x] **POP3 command pipelining** — the core throughput feature; the primary reason for v3.0; most complex to implement correctly; must be guarded by PIPELINING CAPA check; provides VecDeque-based pending command queue; falls back to sequential mode automatically. This is the anchor feature of v3.0.
- [x] **UIDL caching / incremental sync helper** — directly requested in issue #2; LOW additional complexity on top of v2.0 UIDL; high practical value for any mail-processing application; can be a two-method addition.
- [x] **Automatic reconnection with exponential backoff** — directly requested in issue #2; MEDIUM complexity; use `backon` 1.6.0 behind optional `reconnect` feature flag or implement manually with `tokio::time::sleep`; must clearly document the DELE-reset behavior.

### Add After Core Is Stable (v3.x)

- [ ] **Optional MIME integration** — LOW complexity; genuinely useful; but it is additive and has no protocol interaction. Can be a v3.1 or v3.0 patch if time allows. Recommend `mail-parser` behind `mime` feature flag.
- [ ] **Incremental sync helper (`fetch_new`)** — LOW complexity; composition method; add in same PR as UIDL caching or in a follow-up patch.

### Future Consideration (v4+)

- [ ] **SASL PLAIN** — thin wrapper; low demand; defer unless user requests it specifically.
- [ ] **OAUTH2/XOAUTH2** — explicitly out of scope; requires HTTP client dependency; do not add.
- [ ] **Connection pooling** — protocol-incompatible; do not implement.

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| POP3 command pipelining (RFC 2449) | HIGH | HIGH | P1 |
| UIDL caching + incremental sync helper | HIGH | LOW | P1 |
| Automatic reconnection with exponential backoff | HIGH | MEDIUM | P1 |
| Optional MIME parsing integration (mail-parser feature) | MEDIUM | LOW | P2 |
| Pipelining auto-detection via CAPA | MEDIUM | LOW | P2 (bundled with pipelining P1) |
| Connection pooling | LOW | N/A | Anti-feature — do not implement |
| Transparent auto-reconnect with DELE re-issue | LOW | HIGH | Anti-feature — do not implement |

**Priority key:**
- P1: Must have for v3.0.0 release
- P2: Should have; add in v3.0.0 if possible, else v3.1
- Anti-feature: Explicitly excluded, documented in PITFALLS.md

---

## Technical Implementation Notes

### Pipelining: How It Works

RFC 2449 Section 3 (PIPELINING) defines the mechanism:
1. Client calls CAPA after authentication; server returns capability list.
2. If `PIPELINING` is in the list, client may send multiple commands before reading responses.
3. Server processes commands in order received; responses arrive in the same order.
4. Client maintains an in-order queue of expected responses (a `VecDeque<PendingCommand>`).
5. After sending all commands, client reads responses one by one from the queue, matching each response to the head of the pending queue.

**Key practical use case (from RFC 2449):** Sending USER + PASS as a batch (saves one round-trip on login). Sending all RETR N commands for a session before reading any responses. Sending DELE after each RETR response arrives while the next RETR response is still in transit.

**Concrete Rust pattern:**
```rust
// Phase 1: send all commands
let mut pending: VecDeque<CommandKind> = VecDeque::new();
for msg_num in &to_fetch {
    self.writer.write_all(format!("RETR {}\r\n", msg_num).as_bytes()).await?;
    pending.push_back(CommandKind::Retr);
}
self.writer.flush().await?;

// Phase 2: read all responses in order
let mut results = Vec::new();
while let Some(kind) = pending.pop_front() {
    let response = self.read_multiline_response().await?;
    results.push(response);
}
```

**Window size consideration:** RFC 2449 notes that clients using blocking writes must not exceed the underlying transport window size (typically 64KB on TCP). For async Rust with `tokio::io::AsyncWriteExt`, writes complete without blocking, but buffering all commands for a 10,000-message mailbox before flushing would use significant memory. A practical limit of 50–100 commands per pipeline batch is sufficient and safe.

### UIDL Caching: What the Library Provides vs. What the Caller Provides

The library provides:
- `client.uidl_all() -> Result<HashMap<u32, String>>` (already in v2.0 — returns `message_number -> uid`)
- `client.fetch_new(seen_uids: &HashSet<String>) -> Result<Vec<NewMessage>>` — issues UIDL, filters against `seen_uids`, returns `(uid, message_number, body)` for truly new messages.

The caller provides:
- Persistence of the seen set between sessions (a one-liner with `serde_json::to_string`)
- The seen `HashSet<String>` on each call

The library does NOT provide:
- File I/O for the seen set
- Database integration
- Automatic persistence

### Automatic Reconnection: Correct Semantics

The library provides a `ReconnectingClient` wrapper or a `reconnect_with_backoff(config)` method with these semantics:
1. If a command returns `Pop3Error::Io(_)` or `Pop3Error::ConnectionClosed`, the wrapper attempts reconnection.
2. Reconnection uses exponential backoff with jitter (initial: 100ms, multiplier: 2.0, max: 30s, jitter: ±20%).
3. After reconnection, the session is in `Authorization` state — caller must re-authenticate.
4. DELE marks from the previous session are LOST. The wrapper must notify the caller via a `ReconnectEvent` callback or a distinct error variant before retrying.
5. Use `backon` 1.6.0 (`ExponentialBuilder`) for the backoff logic — it has native tokio integration, active maintenance, and 100% documentation coverage.

**Alternative (no new dependency):** Manual implementation using `tokio::time::sleep` and a loop. This avoids adding `backon` as a dependency. For a transport library where dependencies are kept minimal, this may be preferred. The implementation is ~30 lines.

### MIME Integration: Which Crate

**Recommendation: `mail-parser` 0.11.2** behind a `mime` feature flag.

Rationale:
- Zero external dependencies (pure Rust).
- Zero-copy design (`Cow<str>` for headers) — no unnecessary allocation.
- Full RFC 5322 + RFC 2045–2049 (MIME) + RFC 8621 (JMAP) conformance.
- 41 character set decodings including legacy (BIG5, ISO-2022-JP, UTF-7).
- Active maintenance by Stalwart Labs (the same team maintaining the Stalwart mail server).
- `mailparse` (v0.16.1) is a viable alternative but has 3 external dependencies vs. zero for `mail-parser`.

Feature flag name: `mime` (not `mail-parser` — keep the public API name stable even if the back-end changes).

```toml
# Cargo.toml addition for v3.0
mail-parser = { version = "0.11", optional = true }
backon       = { version = "1.6", features = ["tokio-sleep"], optional = true }

[features]
mime      = ["dep:mail-parser"]
reconnect = ["dep:backon"]   # OR remove if implementing manually
```

---

## Competitor Feature Analysis

| Feature | async-pop (crates.io) | mailme (privaterookie) | v2.0 (this crate) | v3.0 Target |
|---------|----------------------|----------------------|--------------------|-------------|
| Async/await (tokio) | Yes | Yes | Yes | Yes (foundation) |
| POP3 pipelining | No | Unknown | No | Yes (RFC 2449) |
| UIDL caching / incremental sync | No | Unknown | No | Yes |
| Automatic reconnection | No | Unknown | No | Yes (with backoff) |
| Connection pooling | No | No | No (anti-feature) | No (protocol impossible) |
| MIME parsing integration | No | No | No | Yes (optional, mail-parser) |
| Dual TLS backends | No (1 backend) | No | Yes | Yes (inherited) |
| Typed errors | Yes | Unknown | Yes | Yes (enhanced) |
| Documentation | 31% | Unknown | Full | Full |
| Actively maintained | Unclear | Unclear | Yes | Yes |

No existing Rust POP3 crate implements pipelining, UIDL caching, or automatic reconnection. v3.0 would be the first production-ready Rust POP3 library with these features.

---

## Sources

- RFC 2449 (POP3 Extension Mechanism, PIPELINING definition): https://www.rfc-editor.org/rfc/rfc2449.html — HIGH confidence (authoritative spec)
- RFC 1939 (POP3 core, exclusive mailbox lock requirement): https://www.rfc-editor.org/rfc/rfc1939.html — HIGH confidence (authoritative spec)
- IANA POP3 Extension Registry (PIPELINING listed): https://www.iana.org/assignments/pop3-extension-mechanism — HIGH confidence (authoritative)
- mailparse 0.16.1 docs: https://docs.rs/mailparse/ — HIGH confidence (official docs)
- mail-parser 0.11.2 docs: https://docs.rs/mail-parser/ — HIGH confidence (official docs)
- backon 1.6.0 docs: https://docs.rs/backon/ — HIGH confidence (official docs)
- backoff 0.4.0 docs (alternative): https://docs.rs/backoff — HIGH confidence (official docs)
- tokio channels tutorial (mpsc + oneshot for pipelining pattern): https://tokio.rs/tokio/tutorial/channels — HIGH confidence (official tokio docs)
- MailKit POP3 pipelining implementation (reference for batch command pattern): https://github.com/jstedfast/MailKit/blob/master/MailKit/Net/Pop3/Pop3Client.cs — MEDIUM confidence (well-maintained open source reference)
- Dovecot POP3 locking documentation (mailbox exclusive lock confirmed): https://dovecot.dovecot.narkive.com/JggtijXU/pop3-locking — MEDIUM confidence (server implementation confirms RFC requirement)
- POP3 UIDL for incremental sync (client implementation pattern): https://owl.billpg.com/pop3-uidl/ — MEDIUM confidence (community; verified against RFC 1939)
- Stalwart mail-parser MIME parsing in Rust (zero-dependency, RFC conformance): https://stalwartlabs.medium.com/parsing-mime-e-mail-messages-in-rust-8095d4b1ee5c — MEDIUM confidence (official blog post from library authors)
- Pools and Pipeline with Tokio (VecDeque queue pattern for pipelining): https://terencezl.github.io/blog/2023/12/27/pools-and-pipeline-with-tokio-part-i/ — MEDIUM confidence (community; consistent with tokio official patterns)
- Gmail ending POP3 third-party import (context for POP3 relevance): https://support.google.com/mail/answer/16604719 — HIGH confidence (official Google support doc)

---

*Feature research for: rust-adv-pop3 v3.0 advanced features milestone*
*Researched: 2026-03-01*
