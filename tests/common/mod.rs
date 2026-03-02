use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// Spawn a minimal mock POP3 server on a random loopback port.
///
/// The server sends a greeting on connect, then replays `conversation` pairs
/// of (expected_command, response) in order. Each pair reads exactly one
/// CRLF-terminated line from the client, asserts it matches the expected
/// command, then sends the corresponding response bytes.
///
/// Returns the bound address as a `String` (e.g. `"127.0.0.1:54321"`).
pub async fn spawn_mock_server(conversation: Vec<(&'static str, &'static str)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut writer) = tokio::io::split(socket);
        let mut reader = BufReader::new(read_half);

        // Send the POP3 greeting
        writer
            .write_all(b"+OK Mock POP3 server ready\r\n")
            .await
            .unwrap();

        // Replay the scripted conversation one command at a time
        for (expected_cmd, response) in conversation {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let received = line.trim_end_matches(['\r', '\n']);
            assert_eq!(
                received, expected_cmd,
                "mock server: unexpected command received"
            );
            writer.write_all(response.as_bytes()).await.unwrap();
        }
    });

    addr
}
