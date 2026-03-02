//! Phase 4 — Commands: LIST, UIDL, RETR, DELE, RSET, NOOP, auth failure.

mod common;

use common::spawn_mock_server;
use pop3::{Pop3Client, Pop3Error};

/// Helper: connect and authenticate a client against the mock server.
async fn connect_and_login(addr: &str) -> Pop3Client {
    let mut client = Pop3Client::connect(addr, std::time::Duration::from_secs(5))
        .await
        .unwrap();
    client.login("user", "pass").await.unwrap();
    client
}

/// LIST with no argument returns all message entries.
#[tokio::test]
async fn list_all_messages() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        ("LIST", "+OK\r\n1 1200\r\n2 3400\r\n3 5600\r\n.\r\n"),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = connect_and_login(&addr).await;
    let entries = client.list(None).await.unwrap();

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].message_id, 1);
    assert_eq!(entries[0].size, 1200);
    assert_eq!(entries[1].message_id, 2);
    assert_eq!(entries[1].size, 3400);
    assert_eq!(entries[2].message_id, 3);
    assert_eq!(entries[2].size, 5600);

    client.quit().await.unwrap();
}

/// LIST with a specific message ID returns a single entry.
#[tokio::test]
async fn list_single_message() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        ("LIST 2", "+OK 2 3400\r\n"),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = connect_and_login(&addr).await;
    let entries = client.list(Some(2)).await.unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].message_id, 2);
    assert_eq!(entries[0].size, 3400);

    client.quit().await.unwrap();
}

/// UIDL returns unique IDs for all messages.
#[tokio::test]
async fn uidl_all_messages() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        (
            "UIDL",
            "+OK\r\n1 uid-aaa\r\n2 uid-bbb\r\n3 uid-ccc\r\n.\r\n",
        ),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = connect_and_login(&addr).await;
    let entries = client.uidl(None).await.unwrap();

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].unique_id, "uid-aaa");
    assert_eq!(entries[1].unique_id, "uid-bbb");
    assert_eq!(entries[2].unique_id, "uid-ccc");

    client.quit().await.unwrap();
}

/// RETR retrieves full message content with dot-unstuffing applied.
#[tokio::test]
async fn retr_message() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        (
            "RETR 1",
            "+OK\r\nFrom: sender@example.com\r\nSubject: Test\r\n\r\nHello, world!\r\n..leading dot\r\n.\r\n",
        ),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = connect_and_login(&addr).await;
    let msg = client.retr(1).await.unwrap();

    assert!(msg.data.contains("Subject: Test"));
    assert!(msg.data.contains("Hello, world!"));
    // Dot-unstuffing: ".." at start of line becomes "."
    assert!(msg.data.contains(".leading dot"));
    assert!(!msg.data.contains("..leading dot"));

    client.quit().await.unwrap();
}

/// DELE marks a message, RSET unmarks all.
#[tokio::test]
async fn dele_and_rset() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        ("DELE 1", "+OK message 1 deleted\r\n"),
        ("RSET", "+OK\r\n"),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = connect_and_login(&addr).await;
    client.dele(1).await.unwrap();
    client.rset().await.unwrap();
    client.quit().await.unwrap();
}

/// NOOP succeeds as a keepalive.
#[tokio::test]
async fn noop_keepalive() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        ("NOOP", "+OK\r\n"),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = connect_and_login(&addr).await;
    client.noop().await.unwrap();
    client.quit().await.unwrap();
}

/// Login with bad credentials returns `Pop3Error::AuthFailed`.
#[tokio::test]
async fn login_failure_returns_auth_failed() {
    let addr = spawn_mock_server(vec![("USER baduser", "-ERR unknown user\r\n")]).await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();

    let err = client.login("baduser", "badpass").await.unwrap_err();
    assert!(
        matches!(err, Pop3Error::AuthFailed(_)),
        "expected AuthFailed, got: {err:?}"
    );
}
