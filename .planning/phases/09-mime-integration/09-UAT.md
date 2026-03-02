---
status: testing
phase: 09-mime-integration
source: 09-01-SUMMARY.md
started: 2026-03-01T12:00:00Z
updated: 2026-03-01T12:00:00Z
---

## Current Test

number: 1
name: Build with mime feature
expected: |
  `cargo build --features mime` compiles successfully with no errors.
awaiting: user response

## Tests

### 1. Build with mime feature
expected: `cargo build --features mime` compiles successfully with no errors.
result: [pending]

### 2. All tests pass with mime feature
expected: `cargo test --features mime` runs all tests and they pass (168+ tests, 0 failures).
result: [pending]

### 3. Build without mime feature (optional dep stays optional)
expected: `cargo build` (no features) compiles successfully — the mime dependency is truly optional and doesn't leak into the default build.
result: [pending]

### 4. ParsedMessage type re-exported from crate root
expected: In `src/lib.rs`, `ParsedMessage` is re-exported behind `#[cfg(feature = "mime")]`. When mime feature is active, `pop3::ParsedMessage` is accessible as `mail_parser::Message<'static>`.
result: [pending]

### 5. MimeParse error variant exists unconditionally
expected: `Pop3Error::MimeParse(String)` exists in `src/error.rs` without any `#[cfg(feature)]` gate. You can match on it even without the mime feature enabled.
result: [pending]

### 6. retr_parsed() and top_parsed() methods exist
expected: `Pop3Client` has `retr_parsed()` and `top_parsed()` methods in a `#[cfg(feature = "mime")]` impl block in `src/client.rs`. They return `Result<ParsedMessage>`.
result: [pending]

### 7. Example compiles
expected: `cargo build --example mime --features mime` builds the example successfully.
result: [pending]

### 8. Clippy clean with mime feature
expected: `cargo clippy --features mime -- -D warnings` passes with no warnings.
result: [pending]

## Summary

total: 8
passed: 0
issues: 0
pending: 8
skipped: 0

## Gaps

[none yet]
