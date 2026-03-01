//! Example: Connecting to a POP3 server over TLS (port 995).
//!
//! Requires the `rustls-tls` (default) or `openssl-tls` feature flag.
//!
//! Run with: `cargo run --example tls`

use pop3::Pop3Client;

#[tokio::main]
async fn main() -> pop3::Result<()> {
    let mut client = Pop3Client::connect_tls(
        ("pop.gmail.com", 995),
        "pop.gmail.com",
        std::time::Duration::from_secs(30),
    )
    .await?;

    println!("Greeting: {}", client.greeting());
    println!("Encrypted: {}", client.is_encrypted());

    client.login("user@gmail.com", "app-password").await?;

    let stat = client.stat().await?;
    println!("{} messages, {} bytes", stat.message_count, stat.mailbox_size);

    let entries = client.list(None).await?;
    for entry in &entries {
        println!("Message {}: {} bytes", entry.message_id, entry.size);
    }

    client.quit().await?;
    Ok(())
}
