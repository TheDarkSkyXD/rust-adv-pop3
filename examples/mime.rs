//! Example: Retrieve and parse a message as structured MIME.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example mime --features mime
//! ```
//!
//! Requires a running POP3 server on localhost:110.

use pop3::Pop3Client;

#[tokio::main]
async fn main() -> pop3::Result<()> {
    // Connect over plain TCP
    let mut client =
        Pop3Client::connect(("pop.example.com", 110), std::time::Duration::from_secs(30)).await?;

    // Authenticate
    client.login("user", "pass").await?;

    // Retrieve and parse message 1
    let parsed = client.retr_parsed(1).await?;

    // Access structured fields
    println!("Subject: {:?}", parsed.subject());
    println!("From:    {:?}", parsed.from());
    println!("Body:    {:?}", parsed.body_text(0));
    println!("HTML:    {:?}", parsed.body_html(0));
    println!("Attachments: {}", parsed.attachment_count());

    // Retrieve headers only (0 body lines)
    let headers = client.top_parsed(1, 0).await?;
    println!("Subject (from TOP): {:?}", headers.subject());

    client.quit().await?;
    Ok(())
}
