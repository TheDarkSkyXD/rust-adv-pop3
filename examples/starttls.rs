//! Example: Upgrading a plain TCP connection to TLS via STARTTLS (STLS command).
//!
//! Requires the `rustls-tls` (default) or `openssl-tls` feature flag.
//!
//! Run with: `cargo run --example starttls`

use pop3::Pop3Client;

#[tokio::main]
async fn main() -> pop3::Result<()> {
    // Connect over plain TCP first
    let mut client =
        Pop3Client::connect(("pop.example.com", 110), std::time::Duration::from_secs(30)).await?;

    println!("Connected (encrypted: {})", client.is_encrypted());

    // Upgrade to TLS via STARTTLS
    client.stls("pop.example.com").await?;
    println!("Upgraded  (encrypted: {})", client.is_encrypted());

    // Now authenticate over the encrypted connection
    client.login("user", "password").await?;

    let stat = client.stat().await?;
    println!(
        "{} messages, {} bytes",
        stat.message_count, stat.mailbox_size
    );

    client.quit().await?;
    Ok(())
}
