//! Phase 2 — Async Core: session state transitions, auth guards, accessor checks.

mod common;

use common::spawn_mock_server;
use pop3::{Pop3Client, Pop3Error, SessionState};

/// After connect, the session state should be `Connected`.
#[tokio::test]
async fn session_state_connected_on_connect() {
    let addr = spawn_mock_server(vec![("QUIT", "+OK bye\r\n")]).await;

    let client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();

    assert_eq!(client.state(), SessionState::Connected);
    client.quit().await.unwrap();
}

/// After login, the session state should transition to `Authenticated`.
#[tokio::test]
async fn session_state_authenticated_after_login() {
    let addr = spawn_mock_server(vec![
        ("USER alice", "+OK\r\n"),
        ("PASS secret", "+OK logged in\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        ("QUIT", "+OK bye\r\n"),
    ])
    .await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();

    assert_eq!(client.state(), SessionState::Connected);
    client.login("alice", "secret").await.unwrap();
    assert_eq!(client.state(), SessionState::Authenticated);

    client.quit().await.unwrap();
}

/// After quit, the session completes — quit() consumes the client so no further
/// calls are possible (compile-time guarantee). We verify the flow completes.
#[tokio::test]
async fn session_state_disconnected_after_quit() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        ("QUIT", "+OK goodbye\r\n"),
    ])
    .await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();

    client.login("user", "pass").await.unwrap();
    // quit() consumes the client — this is the compile-time use-after-disconnect prevention
    client.quit().await.unwrap();
    // If we tried to use `client` here, it would be a compile error.
}

/// Commands that require authentication should fail with `NotAuthenticated`
/// when called before login.
#[tokio::test]
async fn stat_before_login_fails_not_authenticated() {
    let addr = spawn_mock_server(vec![]).await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();

    let err = client.stat().await.unwrap_err();
    assert!(
        matches!(err, Pop3Error::NotAuthenticated),
        "expected NotAuthenticated, got: {err:?}"
    );
}

/// Verify `is_closed()` returns false on a fresh connection and
/// `is_encrypted()` returns false on a plain TCP connection.
#[tokio::test]
async fn accessors_on_fresh_connection() {
    let addr = spawn_mock_server(vec![("QUIT", "+OK bye\r\n")]).await;

    let client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();

    assert!(!client.is_closed());
    assert!(!client.is_encrypted());

    client.quit().await.unwrap();
}
