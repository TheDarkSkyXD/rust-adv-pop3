rust-pop3
================
POP3 Client for Rust

A modern, safe POP3 client library using [rustls](https://github.com/rustls/rustls) for TLS (pure Rust, no system dependencies). All operations return `Result` — no panics or unwraps.

[![Number of Crate Downloads](https://img.shields.io/crates/d/pop3.svg)](https://crates.io/crates/pop3)
[![Crate Version](https://img.shields.io/crates/v/pop3.svg)](https://crates.io/crates/pop3)
[![Crate License](https://img.shields.io/crates/l/pop3.svg)](https://crates.io/crates/pop3)

[Documentation](https://docs.rs/pop3/)

### Usage

Add to your `Cargo.toml`:
```toml
[dependencies]
pop3 = "2"
```

```rust
use pop3::{Pop3Client, TlsMode};

fn main() -> pop3::Result<()> {
    let mut client = Pop3Client::connect(
        ("pop.gmail.com", 995),
        TlsMode::Tls("pop.gmail.com".into()),
    )?;
    println!("Greeting: {}", client.greeting());

    client.login("username", "password")?;

    let stat = client.stat()?;
    println!("{} messages, {} bytes", stat.message_count, stat.mailbox_size);

    let entries = client.list(None)?;
    for entry in &entries {
        println!("Message {}: {} bytes", entry.message_id, entry.size);
    }

    if let Some(first) = entries.first() {
        let msg = client.retr(first.message_id)?;
        println!("{}", msg.data);
    }

    client.quit()?;
    Ok(())
}
```

### License

MIT
