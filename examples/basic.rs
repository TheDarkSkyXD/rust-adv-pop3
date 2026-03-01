use pop3::{Pop3Client, TlsMode};

fn main() -> pop3::Result<()> {
    // Connect to a POP3 server over TLS
    let mut client =
        Pop3Client::connect(("pop.gmail.com", 995), TlsMode::Tls("pop.gmail.com".into()))?;
    println!("Greeting: {}", client.greeting());

    // Authenticate
    client.login("username", "password")?;

    // Get mailbox statistics
    let stat = client.stat()?;
    println!(
        "{} messages, {} bytes total",
        stat.message_count, stat.mailbox_size
    );

    // List all messages
    let entries = client.list(None)?;
    for entry in &entries {
        println!("Message {}: {} bytes", entry.message_id, entry.size);
    }

    // Retrieve the first message (if any)
    if let Some(first) = entries.first() {
        let msg = client.retr(first.message_id)?;
        println!("--- Message {} ---", first.message_id);
        println!("{}", msg.data);
    }

    // Disconnect
    client.quit()?;
    Ok(())
}
