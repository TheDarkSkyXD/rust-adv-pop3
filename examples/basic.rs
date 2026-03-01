use pop3::Pop3Client;

#[tokio::main]
async fn main() -> pop3::Result<()> {
    // Connect to a POP3 server (plain TCP, port 110)
    let mut client =
        Pop3Client::connect(("pop.example.com", 110), std::time::Duration::from_secs(30)).await?;
    println!("Greeting: {}", client.greeting());

    // Authenticate
    client.login("username", "password").await?;

    // Get mailbox statistics
    let stat = client.stat().await?;
    println!(
        "{} messages, {} bytes total",
        stat.message_count, stat.mailbox_size
    );

    // List all messages
    let entries = client.list(None).await?;
    for entry in &entries {
        println!("Message {}: {} bytes", entry.message_id, entry.size);
    }

    // Retrieve the first message (if any)
    if let Some(first) = entries.first() {
        let msg = client.retr(first.message_id).await?;
        println!("--- Message {} ---", first.message_id);
        println!("{}", msg.data);
    }

    // Disconnect (consumes the client)
    client.quit().await?;
    Ok(())
}
