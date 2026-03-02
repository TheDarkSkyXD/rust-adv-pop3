//! Integration tests exercising the public Pop3Client API over real TCP.
//!
//! These tests spawn a minimal mock POP3 server on a random loopback port using
//! `tokio::net::TcpListener`, then connect the real `Pop3Client` to it. This
//! verifies that the public API works end-to-end over genuine TCP sockets —
//! not just mock I/O injected at the transport layer.

mod common;

use common::spawn_mock_server;
use pop3::{Pop3Client, SessionState};

/// Verify the happy path: connect -> login -> stat -> quit over real TCP.
#[tokio::test]
async fn public_api_connect_login_stat_quit() {
    let addr = spawn_mock_server(vec![
        ("USER testuser", "+OK\r\n"),
        ("PASS testpass", "+OK logged in\r\n"),
        // CAPA probe sent automatically by login() after successful auth
        ("CAPA", "+OK\r\n.\r\n"),
        ("STAT", "+OK 3 15000\r\n"),
        ("QUIT", "+OK goodbye\r\n"),
    ])
    .await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();

    assert_eq!(client.state(), SessionState::Connected);
    assert!(!client.is_encrypted());

    client.login("testuser", "testpass").await.unwrap();
    assert_eq!(client.state(), SessionState::Authenticated);

    let stat = client.stat().await.unwrap();
    assert_eq!(stat.message_count, 3);
    assert_eq!(stat.mailbox_size, 15000);

    client.quit().await.unwrap();
}

/// Verify CAPA (pre-auth) and TOP command over real TCP.
#[tokio::test]
async fn public_api_capa_and_top() {
    let addr = spawn_mock_server(vec![
        ("CAPA", "+OK\r\nTOP\r\nUIDL\r\nSASL PLAIN\r\n.\r\n"),
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        // CAPA probe sent automatically by login() after successful auth
        ("CAPA", "+OK\r\nTOP\r\nUIDL\r\n.\r\n"),
        ("TOP 1 5", "+OK\r\nSubject: Test\r\n\r\nBody line\r\n.\r\n"),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();

    // CAPA is permitted before authentication (RFC 2449)
    let caps = client.capa().await.unwrap();
    assert_eq!(caps.len(), 3);
    assert!(caps.iter().any(|c| c.name == "TOP"));
    assert!(caps.iter().any(|c| c.name == "UIDL"));

    client.login("user", "pass").await.unwrap();
    assert_eq!(client.state(), SessionState::Authenticated);

    // TOP returns headers + up to 5 body lines
    let msg = client.top(1, 5).await.unwrap();
    assert!(msg.data.contains("Subject: Test"));
    assert!(msg.data.contains("Body line"));

    client.quit().await.unwrap();
}
