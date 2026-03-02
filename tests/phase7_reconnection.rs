//! Phase 7 — Reconnection: ReconnectingClient basic flow and accessors.

mod common;

use common::spawn_mock_server;
use pop3::{Pop3ClientBuilder, ReconnectingClientBuilder, SessionState};

/// ReconnectingClient connects, runs stat, and quits successfully.
#[tokio::test]
async fn reconnecting_client_basic_flow() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        ("STAT", "+OK 5 25000\r\n"),
        ("QUIT", "+OK bye\r\n"),
    ])
    .await;

    let parts: Vec<&str> = addr.split(':').collect();
    let host = parts[0];
    let port: u16 = parts[1].parse().unwrap();

    let mut client = ReconnectingClientBuilder::new(Pop3ClientBuilder::new(host).port(port))
        .max_retries(1)
        .connect("user", "pass")
        .await
        .unwrap();

    let outcome = client.stat().await.unwrap();
    assert!(!outcome.is_reconnected());
    let stat = outcome.into_inner();
    assert_eq!(stat.message_count, 5);
    assert_eq!(stat.mailbox_size, 25000);

    client.quit().await.unwrap();
}

/// ReconnectingClient accessors delegate to the inner Pop3Client.
#[tokio::test]
async fn reconnecting_client_accessors() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\nTOP\r\nUIDL\r\n.\r\n"),
        ("QUIT", "+OK bye\r\n"),
    ])
    .await;

    let parts: Vec<&str> = addr.split(':').collect();
    let host = parts[0];
    let port: u16 = parts[1].parse().unwrap();

    let client = ReconnectingClientBuilder::new(Pop3ClientBuilder::new(host).port(port))
        .max_retries(1)
        .connect("user", "pass")
        .await
        .unwrap();

    assert_eq!(client.greeting(), "Mock POP3 server ready");
    assert_eq!(client.state(), SessionState::Authenticated);
    assert!(!client.is_encrypted());
    assert!(!client.is_closed());
    assert!(!client.supports_pipelining());

    client.quit().await.unwrap();
}
