# pop3

A modern, safe, async POP3 client library for Rust, powered by Tokio.

[![CI](https://github.com/TheDarkSkyXD/rust-adv-pop3/actions/workflows/ci.yml/badge.svg)](https://github.com/TheDarkSkyXD/rust-adv-pop3/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/pop3.svg)](https://crates.io/crates/pop3)
[![Docs.rs](https://docs.rs/pop3/badge.svg)](https://docs.rs/pop3)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

## Features

- **Async/await** — all operations are `async fn` on the Tokio runtime
- **Dual TLS backends** — choose `rustls-tls` (default, pure Rust) or `openssl-tls`
- **STARTTLS** — upgrade plain connections to TLS via `stls()`
- **Full POP3 coverage** — STAT, LIST, UIDL, RETR, DELE, RSET, NOOP, TOP, CAPA, QUIT
- **Type-safe sessions** — `quit()` consumes the client, preventing use-after-disconnect
- **Proper error handling** — no panics; all errors returned as `Result<T, Pop3Error>`

## Installation

```toml
[dependencies]
pop3 = "2"
```

For OpenSSL instead of rustls:

```toml
[dependencies]
pop3 = { version = "2", default-features = false, features = ["openssl-tls"] }
```

## Quick Start

```rust
use pop3::Pop3Client;

#[tokio::main]
async fn main() -> pop3::Result<()> {
    // Connect over plain TCP (port 110)
    let mut client = Pop3Client::connect(
        ("pop.example.com", 110),
        std::time::Duration::from_secs(30),
    ).await?;

    client.login("user", "pass").await?;

    let stat = client.stat().await?;
    println!("{} messages, {} bytes", stat.message_count, stat.mailbox_size);

    client.quit().await?;
    Ok(())
}
```

## TLS Connections (port 995)

```rust
use pop3::Pop3Client;

#[tokio::main]
async fn main() -> pop3::Result<()> {
    let mut client = Pop3Client::connect_tls_default(
        ("pop.gmail.com", 995),
        "pop.gmail.com",
    ).await?;

    client.login("user@gmail.com", "app-password").await?;

    let stat = client.stat().await?;
    println!("{} messages, {} bytes", stat.message_count, stat.mailbox_size);

    client.quit().await?;
    Ok(())
}
```

## STARTTLS (Upgrade Plain to TLS)

```rust
use pop3::Pop3Client;

#[tokio::main]
async fn main() -> pop3::Result<()> {
    // Connect over plain TCP
    let mut client = Pop3Client::connect(
        ("pop.example.com", 110),
        std::time::Duration::from_secs(30),
    ).await?;

    // Upgrade to TLS before authenticating
    client.stls("pop.example.com").await?;

    client.login("user", "pass").await?;
    client.quit().await?;
    Ok(())
}
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `rustls-tls` | Yes | TLS via rustls (pure Rust, no system deps) |
| `openssl-tls` | No | TLS via OpenSSL (requires system libssl) |

Enable exactly one TLS backend at a time.

## Supported Commands

| Method | POP3 Command | Description |
|--------|-------------|-------------|
| `connect` / `connect_tls` | — | Establish connection |
| `stls` | STLS | Upgrade to TLS (STARTTLS) |
| `login` | USER / PASS | Authenticate |
| `stat` | STAT | Message count and total size |
| `list` | LIST | List messages with sizes |
| `uidl` | UIDL | List messages with unique IDs |
| `retr` | RETR | Download a full message |
| `top` | TOP | Download message headers + N lines |
| `dele` | DELE | Mark message for deletion |
| `rset` | RSET | Cancel pending deletions |
| `noop` | NOOP | Keep connection alive |
| `capa` | CAPA | Query server capabilities |
| `quit` | QUIT | End session, commit deletions |

## Minimum Rust Version

Rust 1.75 or later (async fn in traits stabilized).

## License

MIT
