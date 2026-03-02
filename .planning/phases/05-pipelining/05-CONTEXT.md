# Phase 5: Pipelining - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

RFC 2449 command pipelining for batch POP3 operations. Callers can send batches of commands without waiting for individual responses, with automatic CAPA-based detection and silent sequential fallback. Also includes infrastructure changes needed by downstream phases (is_closed, ConnectionClosed error variant).

</domain>

<decisions>
## Implementation Decisions

### Batch error handling
- Per-item results: `retr_many` returns `Vec<Result<Message>>`, `dele_many` returns `Vec<Result<()>>` — each item independently Ok or Err
- DELE processing continues through individual -ERR responses — all commands are sent and all responses collected regardless of per-item failures
- I/O errors mid-pipeline: return successfully-received results so far, plus the I/O error for remaining items (caller doesn't lose already-received messages)
- Validate all message IDs upfront before sending any commands — if any ID is invalid (e.g., 0), return `Err(InvalidInput)` immediately without touching the wire

### Batch API surface
- Only `retr_many(&[u32])` and `dele_many(&[u32])` — just what PIPE-05 requires
- No generic pipeline builder or custom command sequencing
- Fixed methods only — no `top_many`, `list_many`, etc. (can be added in future phases if needed)
- Accept `&[u32]` slices — simple, explicit, matches existing single-item methods
- Results are guaranteed to be in the same order as input IDs (natural since POP3 processes commands in order)

### Pipelining visibility
- Add `pub fn supports_pipelining(&self) -> bool` read-only accessor — useful for logging, diagnostics, and testing
- No opt-out mechanism — if server supports pipelining, use it; the windowed strategy is safe
- CAPA probe for PIPELINING happens automatically during `login()`, right after successful auth (matches PIPE-02)
- Builder auto-login also auto-probes CAPA — caller gets a fully-ready client from `connect()`, consistent behavior regardless of login path

### Claude's Discretion
- Window size constant (W = 4 recommended by research, but exact value is implementation detail)
- BufWriter buffer size (default 8 KB should be fine)
- Internal method decomposition (how to split pipelined vs sequential paths)
- Error variant wording for ConnectionClosed
- Whether to add timeout handling per-window or per-batch

</decisions>

<specifics>
## Specific Ideas

- Per-item `Vec<Result<T>>` return type is important — callers should never lose successfully-received messages due to a single failure mid-batch
- I/O errors are fundamentally different from -ERR responses: -ERR means the server processed the command and rejected it (continue), I/O error means the connection may be dead (return what we have)
- The API should feel natural to anyone who has used iterator-based batch APIs — zip input IDs with results for easy processing

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `capa()` method (client.rs:733): Returns `Vec<Capability>` with `.name` field — pipelining detection is `caps.iter().any(|c| c.name == "PIPELINING")`
- `retr()` / `dele()` (client.rs:562, 590): Existing single-item methods — sequential fallback calls these in a loop
- `validate_message_id()` (client.rs:42): Already validates ID >= 1, reuse for batch validation
- `check_no_crlf()` (client.rs:33): Not needed for batch methods (IDs are u32, not strings)
- `response::parse_status_line()`: Reuse for parsing individual pipelined responses

### Established Patterns
- Transport has split reader (`BufReader<ReadHalf<InnerStream>>`) and writer (`WriteHalf<InnerStream>`) — already split via `io::split`
- Writer is bare `WriteHalf` — needs `BufWriter` wrapping for batch flushes
- All commands follow send_command + read_line/read_multiline pattern
- `Pop3ClientBuilder` already derives `Clone` (builder.rs:86)
- `SessionState` enum tracks auth state — batch methods call `require_auth()`

### Integration Points
- `Transport::send_command()`: Current single-command send — batch methods bypass this for windowed writes
- `Transport::read_line()` / `read_multiline()`: Used per-response in the drain loop
- `Pop3Client::login()`: Where CAPA probe is inserted after successful auth
- `Pop3ClientBuilder::connect()`: Where CAPA auto-probe happens if builder does auto-login
- `transport.rs upgrade_in_place()`: Must adapt for BufWriter wrapping (call `into_inner()` before `unsplit()`)

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 05-pipelining*
*Context gathered: 2026-03-01*
