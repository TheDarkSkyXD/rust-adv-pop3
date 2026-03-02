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
- **Builder pattern** — fluent `Pop3ClientBuilder` with smart port defaults and auto-auth
- **Auto-reconnect** — `ReconnectingClient` with exponential backoff and session-loss signaling
- **Connection pooling** — bb8-backed multi-account pool (optional `pool` feature)
- **MIME parsing** — `retr_parsed()` / `top_parsed()` via mail-parser (optional `mime` feature)

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

## Builder Pattern

`Pop3ClientBuilder` provides a fluent API with smart defaults (port 110 for plain/STARTTLS, port 995 for TLS) and optional auto-authentication:

```rust
use pop3::Pop3ClientBuilder;

#[tokio::main]
async fn main() -> pop3::Result<()> {
    let mut client = Pop3ClientBuilder::new("pop.example.com")
        .tls()                              // TLS-on-connect (port 995)
        .credentials("user", "pass")        // auto-login after connect
        .connect()
        .await?;

    let stat = client.stat().await?;
    println!("{} messages", stat.message_count);
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

## Auto-Reconnect

`ReconnectingClient` transparently reconnects on I/O errors with exponential backoff. Every method returns `Outcome<T>` so you know if pending DELE marks were lost:

```rust
use pop3::{Pop3ClientBuilder, ReconnectingClientBuilder, Outcome};

#[tokio::main]
async fn main() -> pop3::Result<()> {
    let mut client = ReconnectingClientBuilder::new(
        Pop3ClientBuilder::new("pop.example.com").port(110),
    )
    .max_retries(5)
    .connect("user@example.com", "app-password")
    .await?;

    let outcome = client.stat().await?;
    if outcome.is_reconnected() {
        eprintln!("Session was reset — pending DELEs discarded");
    }
    let stat = outcome.into_inner();
    println!("{} messages", stat.message_count);

    client.quit().await?;
    Ok(())
}
```

## MIME Parsing

With the `mime` feature, `retr_parsed()` and `top_parsed()` return structured `ParsedMessage` (powered by [mail-parser](https://crates.io/crates/mail-parser)):

```toml
[dependencies]
pop3 = { version = "2", features = ["mime"] }
```

```rust
use pop3::Pop3Client;

#[tokio::main]
async fn main() -> pop3::Result<()> {
    let mut client = Pop3Client::connect(
        ("pop.example.com", 110),
        std::time::Duration::from_secs(30),
    ).await?;
    client.login("user", "pass").await?;

    let parsed = client.retr_parsed(1).await?;
    println!("Subject: {:?}", parsed.subject());
    println!("From:    {:?}", parsed.from());
    println!("Body:    {:?}", parsed.body_text(0));
    println!("Attachments: {}", parsed.attachment_count());

    client.quit().await?;
    Ok(())
}
```

## Connection Pooling

With the `pool` feature, manage multiple POP3 accounts concurrently via a bb8-backed pool that enforces RFC 1939's one-connection-per-mailbox constraint:

```toml
[dependencies]
pop3 = { version = "2", features = ["pool"] }
```

```rust
use pop3::{Pop3Pool, Pop3ClientBuilder, AccountKey};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = Pop3Pool::new(Default::default());

    let key = AccountKey::new("pop.example.com", 995, "user@example.com");
    let builder = Pop3ClientBuilder::new("pop.example.com").tls();

    let mut conn = pool.get(&key, builder, "user@example.com", "pass").await?;
    let stat = conn.stat().await?;
    println!("{} messages", stat.message_count);

    Ok(())
}
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `rustls-tls` | Yes | TLS via rustls (pure Rust, no system deps) |
| `openssl-tls` | No | TLS via OpenSSL (requires system libssl) |
| `pool` | No | Connection pooling via bb8 for multi-account scenarios |
| `mime` | No | MIME parsing via mail-parser (`retr_parsed`, `top_parsed`) |

Enable exactly one TLS backend at a time.

## Supported Commands

| Method | POP3 Command | Description |
|--------|-------------|-------------|
| `connect` / `connect_tls` | — | Establish connection |
| `stls` | STLS | Upgrade to TLS (STARTTLS) |
| `login` | USER / PASS | Authenticate |
| `apop` | APOP | Authenticate with APOP digest |
| `stat` | STAT | Message count and total size |
| `list` | LIST | List messages with sizes |
| `uidl` | UIDL | List messages with unique IDs |
| `retr` | RETR | Download a full message |
| `retr_parsed` | RETR | Download + parse as MIME (requires `mime` feature) |
| `top` | TOP | Download message headers + N lines |
| `top_parsed` | TOP | Download headers + parse as MIME (requires `mime` feature) |
| `dele` | DELE | Mark message for deletion |
| `rset` | RSET | Cancel pending deletions |
| `noop` | NOOP | Keep connection alive |
| `capa` | CAPA | Query server capabilities |
| `quit` | QUIT | End session, commit deletions |

## Minimum Rust Version

Rust 1.75 or later (async fn in traits stabilized).

## License

MIT
