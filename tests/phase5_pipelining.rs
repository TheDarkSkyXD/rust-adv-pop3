//! Phase 5 — Pipelining: retr_many, dele_many, supports_pipelining.

mod common;

use common::spawn_mock_server;
use pop3::Pop3Client;

/// retr_many retrieves multiple messages (sequential fallback — server without PIPELINING).
#[tokio::test]
async fn retr_many_returns_all_messages() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        ("RETR 1", "+OK\r\nSubject: First\r\n\r\nBody one\r\n.\r\n"),
        ("RETR 2", "+OK\r\nSubject: Second\r\n\r\nBody two\r\n.\r\n"),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();
    client.login("user", "pass").await.unwrap();

    let results = client.retr_many(&[1, 2]).await.unwrap();
    assert_eq!(results.len(), 2);

    let msg1 = results[0].as_ref().unwrap();
    assert!(msg1.data.contains("Body one"));

    let msg2 = results[1].as_ref().unwrap();
    assert!(msg2.data.contains("Body two"));

    client.quit().await.unwrap();
}

/// dele_many marks multiple messages for deletion (sequential fallback).
#[tokio::test]
async fn dele_many_marks_all() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        ("DELE 1", "+OK deleted\r\n"),
        ("DELE 2", "+OK deleted\r\n"),
        ("DELE 3", "+OK deleted\r\n"),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();
    client.login("user", "pass").await.unwrap();

    let results = client.dele_many(&[1, 2, 3]).await.unwrap();
    assert_eq!(results.len(), 3);
    for result in &results {
        assert!(result.is_ok());
    }

    client.quit().await.unwrap();
}

/// Server advertising PIPELINING capability is detected by `supports_pipelining()`.
#[tokio::test]
async fn supports_pipelining_from_capa() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        // CAPA response includes PIPELINING
        ("CAPA", "+OK\r\nPIPELINING\r\nTOP\r\nUIDL\r\n.\r\n"),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();

    assert!(
        !client.supports_pipelining(),
        "should be false before login"
    );

    client.login("user", "pass").await.unwrap();
    assert!(
        client.supports_pipelining(),
        "should be true after login with PIPELINING capability"
    );

    client.quit().await.unwrap();
}
