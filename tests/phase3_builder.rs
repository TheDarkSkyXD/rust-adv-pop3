//! Phase 3 — Builder: Pop3ClientBuilder over TCP.

mod common;

use common::spawn_mock_server;
use pop3::{Pop3ClientBuilder, SessionState};

/// Builder with plain TCP connects successfully.
#[tokio::test]
async fn builder_plain_connect() {
    let addr = spawn_mock_server(vec![("QUIT", "+OK bye\r\n")]).await;

    // Parse addr into host and port for the builder
    let parts: Vec<&str> = addr.split(':').collect();
    let host = parts[0];
    let port: u16 = parts[1].parse().unwrap();

    let client = Pop3ClientBuilder::new(host)
        .port(port)
        .connect()
        .await
        .unwrap();

    assert_eq!(client.state(), SessionState::Connected);
    client.quit().await.unwrap();
}

/// Builder with `.credentials()` auto-authenticates.
#[tokio::test]
async fn builder_with_credentials() {
    let addr = spawn_mock_server(vec![
        ("USER bob", "+OK\r\n"),
        ("PASS hunter2", "+OK logged in\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        ("QUIT", "+OK bye\r\n"),
    ])
    .await;

    let parts: Vec<&str> = addr.split(':').collect();
    let host = parts[0];
    let port: u16 = parts[1].parse().unwrap();

    let client = Pop3ClientBuilder::new(host)
        .port(port)
        .credentials("bob", "hunter2")
        .connect()
        .await
        .unwrap();

    assert_eq!(client.state(), SessionState::Authenticated);
    client.quit().await.unwrap();
}

/// Cloning a builder produces an equivalent builder that connects successfully.
#[tokio::test]
async fn builder_clone_produces_equivalent() {
    let addr = spawn_mock_server(vec![("QUIT", "+OK bye\r\n")]).await;

    let parts: Vec<&str> = addr.split(':').collect();
    let host = parts[0];
    let port: u16 = parts[1].parse().unwrap();

    let builder = Pop3ClientBuilder::new(host).port(port);
    let cloned = builder.clone();

    // Connect with the clone — the original is consumed by connect()
    let client = cloned.connect().await.unwrap();
    assert_eq!(client.state(), SessionState::Connected);
    client.quit().await.unwrap();
}
