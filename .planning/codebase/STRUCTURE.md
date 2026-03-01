# Codebase Structure

**Analysis Date:** 2026-03-01

## Directory Layout

```
rust-adv-pop3/
├── src/
│   └── pop3.rs          # Entire library implementation (538 lines)
├── .planning/
│   └── codebase/        # GSD analysis documents
├── .claude/             # Claude Code agent configuration
├── target/              # Cargo build output (gitignored)
├── Cargo.toml           # Package manifest and dependencies
├── Cargo.lock           # Dependency lockfile (gitignored)
├── example.rs           # Runnable example binary (project root)
├── .travis.yml          # CI pipeline configuration
├── .gitignore           # Ignores /target and /Cargo.lock
├── LICENSE              # MIT license
└── README.md            # Usage documentation
```

## Directory Purposes

**`src/`:**
- Purpose: Library source code
- Contains: A single Rust source file — the entire POP3 client implementation
- Key files: `src/pop3.rs`

**`target/`:**
- Purpose: Cargo compilation output
- Contains: Debug/release build artifacts, dependency caches
- Generated: Yes
- Committed: No (in `.gitignore`)

**`.planning/codebase/`:**
- Purpose: GSD codebase analysis documents used by planning and execution commands
- Contains: Markdown analysis files (ARCHITECTURE.md, STRUCTURE.md, etc.)
- Generated: Yes (by GSD tooling)
- Committed: Yes

**`.claude/`:**
- Purpose: Claude Code agent and command configuration
- Contains: Agent definitions, GSD command scripts, workflow templates
- Generated: No (configuration)
- Committed: Yes

## Key File Locations

**Entry Points:**
- `src/pop3.rs`: Library crate root — defines `#![crate_name = "pop3"]` and `#![crate_type = "lib"]`
- `example.rs`: Binary entry point (`fn main()`) demonstrating end-to-end usage

**Configuration:**
- `Cargo.toml`: Package name (`pop3`), version (`1.0.6`), lib path, binary path, and three dependencies
- `.travis.yml`: CI build configuration for Travis CI

**Core Logic:**
- `src/pop3.rs` lines 40–351: `POP3Stream` — all public command methods
- `src/pop3.rs` lines 384–537: `POP3Response` — response accumulation and parsing state machine
- `src/pop3.rs` lines 21–29: `lazy_static!` regex constants

**Testing:**
- No test files present. No `tests/` directory. No `#[cfg(test)]` modules detected in `src/pop3.rs`.

## Naming Conventions

**Files:**
- Snake case: `pop3.rs` — matches the crate name

**Types (structs and enums):**
- `POP3` prefix + PascalCase descriptor: `POP3Stream`, `POP3Result`, `POP3Command`, `POP3Response`, `POP3EmailMetadata`, `POP3EmailUidldata`

**Enum Variants:**
- `POP3` prefix + PascalCase: `POP3Ok`, `POP3Err`, `POP3Stat`, `POP3List`, `POP3Message`, `POP3Uidl`
- Command variants are plain PascalCase (no prefix): `Greet`, `User`, `Pass`, `Stat`, `ListAll`, `ListOne`, etc.

**Methods:**
- Snake case matching POP3 command names: `login()`, `stat()`, `list()`, `retr()`, `dele()`, `noop()`, `rset()`, `quit()`, `uidl()`
- Internal helpers: `write_str()`, `read()`, `read_response()`, `add_line()`, `parse_stat()`, `parse_list_all()`, `parse_list_one()`, `parse_uidl_all()`, `parse_uidl_one()`, `parse_message()`

**Fields:**
- Snake case: `is_authenticated`, `message_id`, `message_size`, `message_uid`, `emails_metadata`

**Regex constants:**
- SCREAMING_SNAKE_CASE with `_REGEX` suffix: `OK_REGEX`, `ERR_REGEX`, `STAT_REGEX`, `ENDING_REGEX`, `MESSAGE_DATA_LIST_ALL_REGEX`

## Where to Add New Code

**New POP3 Command (e.g., TOP, APOP):**
- Add variant to `POP3Command` enum: `src/pop3.rs` lines 47–61
- Add `POP3Result` variant if a new return shape is needed: `src/pop3.rs` lines 366–382
- Add public method on `impl POP3Stream`: `src/pop3.rs` lines 63–351
- Handle the new command variant in `POP3Response::add_line()`: `src/pop3.rs` lines 400–460
- Add a `parse_*` method on `impl POP3Response` if the response format is multi-line: `src/pop3.rs` lines 462–536
- Add new regex to `lazy_static!` block if parsing requires it: `src/pop3.rs` lines 21–29

**New Data Type (returned from a command):**
- Add a new public struct in `src/pop3.rs` following the `POP3Email*` naming pattern
- Add a new variant to `POP3Result` carrying the struct

**New Integration Test or Example:**
- Add a new binary entry in `Cargo.toml` under `[[bin]]` pointing to a `.rs` file at the project root (following the `example.rs` pattern), or create a `tests/` directory with integration test files

**Utilities / Shared Helpers:**
- The codebase has no utility module. Add helpers as private functions or private impl blocks within `src/pop3.rs`, or introduce a new file under `src/` and declare it as `mod` in `src/pop3.rs`

## Special Directories

**`target/`:**
- Purpose: Cargo-managed build cache and output binaries
- Generated: Yes — by `cargo build` / `cargo test`
- Committed: No

**`.git/`:**
- Purpose: Git repository metadata
- Generated: Yes
- Committed: No (excluded implicitly)

---

*Structure analysis: 2026-03-01*
