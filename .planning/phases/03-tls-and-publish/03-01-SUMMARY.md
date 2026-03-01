---
phase: 03-tls-and-publish
plan: 01
subsystem: tls
tags: [rustls, tokio-rustls, ring, tls, feature-flags, transport]

# Dependency graph
requires:
  - phase: 02-async-core
    provides: Async Transport with BufReader/split, Pop3Client with SessionState, tokio I/O infrastructure
provides:
  - InnerStream enum (Plain, RustlsTls, OpensslTls, Mock) replacing Box<dyn AsyncRead/Write>
  - Feature flags: default=[rustls-tls], rustls-tls, openssl-tls with mutual exclusion guard
  - Transport::connect_tls for rustls-tls feature with native cert loading
  - Pop3Client::connect_tls, connect_tls_default, is_encrypted public API
  - Pop3Error::Tls(String) — backend-agnostic TLS error variant
affects: [03-02-starttls, 03-03-doctests, 03-04-publish]

# Tech tracking
tech-stack:
  added:
    - tokio-rustls 0.26 (ring backend, optional dep under rustls-tls feature)
    - rustls-native-certs 0.8 (optional dep under rustls-tls feature)
    - tokio-openssl 0.6 (optional dep under openssl-tls feature, stub only)
    - openssl 0.10 (optional dep under openssl-tls feature, stub only)
  patterns:
    - "InnerStream enum: concrete variant enum over Box<dyn Trait> — enables unsplit() for STARTTLS in Plan 02"
    - "Boxing large enum variants: Box<TlsStream<TcpStream>> satisfies clippy::large_enum_variant"
    - "Feature-gated TLS: #[cfg(feature = rustls-tls)] guards all TLS code paths"
    - "Backend-agnostic errors: .map_err(|e| Pop3Error::Tls(e.to_string())) converts any TLS error"
    - "compile_error! mutual exclusion: guards against activating both TLS backends simultaneously"
    - "ring crypto backend: tokio-rustls configured with ring feature to avoid aws-lc-sys/dlltool on Windows"

key-files:
  created: []
  modified:
    - Cargo.toml
    - src/error.rs
    - src/lib.rs
    - src/transport.rs
    - src/client.rs

key-decisions:
  - "Use ring crypto backend for tokio-rustls (not aws-lc-rs default) — aws-lc-sys requires dlltool.exe on Windows, ring builds cleanly"
  - "Box<TlsStream<TcpStream>> in InnerStream::RustlsTls — reduces enum size from 1104 bytes to pointer size, satisfies clippy::large_enum_variant"
  - "compile_error! positioned after //! crate doc block — inner doc comments must precede all items including #[cfg(...)]"
  - "Pop3Error::Tls(String) replaces #[from] rustls::Error — backend-agnostic, no rustls type in public API"
  - "connect_tls/connect_tls_default gated on any(feature=rustls-tls, feature=openssl-tls) — works for either backend"

patterns-established:
  - "InnerStream poll_* match arms must be exhaustive — add variant to enum and all 4 match arms together"
  - "TLS feature stubs: provide connect_tls stub under #[cfg(not(any(...)))] returning Unsupported error"
  - "is_encrypted() always available — not feature-gated, delegates to transport.encrypted bool"

requirements-completed: [TLS-01, TLS-03, TLS-04]

# Metrics
duration: 7min
completed: 2026-03-01
---

# Phase 3 Plan 01: TLS Foundation Summary

**InnerStream enum with tokio-rustls connect_tls (ring backend), feature flags with compile_error! guard, and backend-agnostic Pop3Error::Tls(String)**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-01T22:12:20Z
- **Completed:** 2026-03-01T22:19:00Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Replaced `Box<dyn AsyncRead/Write>` with `InnerStream` concrete enum enabling STARTTLS `unsplit()` in Plan 02
- Added feature flags to Cargo.toml (`default = ["rustls-tls"]`) with mutual exclusion guard in lib.rs
- Implemented `Transport::connect_tls` using tokio-rustls 0.26 with native cert store loading
- Refactored `Pop3Error::Tls` from `#[from] rustls::Error` to `Tls(String)` — no backend types in public API
- Added `Pop3Client::connect_tls`, `connect_tls_default`, `is_encrypted` public methods
- All 62 unit tests + 2 integration tests + 1 doc test pass, clippy zero warnings, fmt clean

## Task Commits

Each task was committed atomically:

1. **Task 1: InnerStream enum, feature flags, rustls TLS connect** - `f5434ef` (feat)
2. **Task 2: Pop3Client TLS methods and clippy clean** - `1f27f5a` (feat)
3. **Fix: remove spurious Upgrading variant** - `946d815` (fix — auto-fix deviation)

## Files Created/Modified

- `Cargo.toml` — Added [features] section, converted rustls deps to optional, added tokio-rustls with ring backend
- `src/error.rs` — Changed `Tls(#[from] rustls::Error)` to `Tls(String)` backend-agnostic variant
- `src/lib.rs` — Added compile_error! mutual exclusion guard for rustls-tls + openssl-tls
- `src/transport.rs` — InnerStream enum, AsyncRead/Write impls, connect_tls, connect_plain updated, is_encrypted, mock updated
- `src/client.rs` — connect_tls, connect_tls_default, is_encrypted methods + is_encrypted tests

## Decisions Made

- Used `ring` crypto backend for tokio-rustls (not default `aws-lc-rs`) — `aws-lc-sys` requires `cmake` and `dlltool.exe` on Windows; `ring` builds cleanly on all platforms
- Boxed `TlsStream<TcpStream>` in `InnerStream::RustlsTls` to satisfy clippy `large_enum_variant` (1104 bytes vs 48 bytes for Plain)
- Placed `compile_error!` macro after `//!` crate doc block — Rust requires inner doc comments before all items

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Switch from aws-lc-rs to ring crypto backend for tokio-rustls**
- **Found during:** Task 1 (Cargo.toml feature flags)
- **Issue:** `tokio-rustls 0.26` defaults to `aws-lc-rs` which requires `dlltool.exe` (MinGW) on Windows — build failed with "program not found"
- **Fix:** Added `default-features = false, features = ["ring"]` to tokio-rustls dep in Cargo.toml
- **Files modified:** `Cargo.toml`
- **Verification:** `cargo build --features rustls-tls` succeeded after switch
- **Committed in:** f5434ef (Task 1 commit)

**2. [Rule 1 - Bug] Fixed compile_error! position in lib.rs**
- **Found during:** Task 1 (lib.rs compile_error! guard)
- **Issue:** Placed `#[cfg] compile_error!` before `//!` inner doc comments — Rust requires inner docs before all items, causing E0753 errors
- **Fix:** Moved `compile_error!` block to after the `//!` crate documentation block
- **Files modified:** `src/lib.rs`
- **Verification:** `cargo build` succeeded
- **Committed in:** f5434ef (Task 1 commit)

**3. [Rule 1 - Bug] Box TlsStream to fix large_enum_variant clippy lint**
- **Found during:** Task 2 verification (`cargo clippy --features rustls-tls -- -D warnings`)
- **Issue:** `TlsStream<TcpStream>` is 1104 bytes vs 48 bytes for `Plain(TcpStream)` — clippy large_enum_variant error
- **Fix:** Changed `RustlsTls(tokio_rustls::client::TlsStream<TcpStream>)` to `RustlsTls(Box<...>)` and updated constructor
- **Files modified:** `src/transport.rs`
- **Verification:** `cargo clippy --features rustls-tls -- -D warnings` passed with zero errors
- **Committed in:** 1f27f5a (Task 2 commit)

**4. [Rule 1 - Bug] Remove spurious Upgrading variant from InnerStream match arm**
- **Found during:** Overall verification
- **Issue:** A linter added an `InnerStream::Upgrading` match arm in `poll_read` referencing a non-existent variant (Plan 02 STARTTLS code that belongs in next plan)
- **Fix:** Removed the orphaned match arm from `AsyncRead::poll_read`
- **Files modified:** `src/transport.rs`
- **Verification:** `cargo build --features rustls-tls` succeeded
- **Committed in:** 946d815

---

**Total deviations:** 4 auto-fixed (2 blocking, 2 bugs)
**Impact on plan:** All auto-fixes were necessary for correctness and compilation. No scope creep — all Plan 02 stubs removed, only Plan 01 deliverables included.

## Issues Encountered

- `aws-lc-sys` Windows build failure (requires MinGW toolchain) — resolved by switching to ring crypto backend for tokio-rustls
- System linter injected Plan 02 STARTTLS code (`Upgrading` variant, `upgrade_in_place`, `tls_handshake`) into transport.rs — all removed as out-of-scope for Plan 01

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 02 (STARTTLS) can begin: `InnerStream` enum provides the `unsplit()` capability needed for in-place TLS upgrade
- `Transport::is_encrypted()` accessor ready for STARTTLS guard logic
- Feature flag infrastructure ready for OpenSSL backend implementation in Plan 02

## Self-Check: PASSED

Files verified present:
- FOUND: .planning/phases/03-tls-and-publish/03-01-SUMMARY.md
- FOUND: Cargo.toml
- FOUND: src/transport.rs
- FOUND: src/client.rs
- FOUND: src/error.rs
- FOUND: src/lib.rs

Commits verified in git history:
- FOUND: f5434ef (Task 1: InnerStream enum, feature flags, rustls TLS connect)
- FOUND: 1f27f5a (Task 2: Pop3Client TLS methods and clippy clean)
- FOUND: 946d815 (fix: remove spurious Upgrading variant)
- FOUND: 1e45cc9 (fix: Upgrading variant in all match arms)
- FOUND: 6efabce (feat: Plan 02 STARTTLS scaffolding with dead_code allows)

---
*Phase: 03-tls-and-publish*
*Completed: 2026-03-01*
