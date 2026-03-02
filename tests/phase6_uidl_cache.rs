//! Phase 6 — UIDL Cache: unseen_uids, fetch_unseen, prune_seen.

mod common;

use std::collections::HashSet;

use common::spawn_mock_server;
use pop3::Pop3Client;

/// unseen_uids returns only UIDs not in the seen set.
#[tokio::test]
async fn unseen_uids_filters_seen() {
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

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();
    client.login("user", "pass").await.unwrap();

    let mut seen = HashSet::new();
    seen.insert("uid-aaa".to_string());
    seen.insert("uid-bbb".to_string());

    let unseen = client.unseen_uids(&seen).await.unwrap();
    assert_eq!(unseen.len(), 1);
    assert_eq!(unseen[0].unique_id, "uid-ccc");
    assert_eq!(unseen[0].message_id, 3);

    client.quit().await.unwrap();
}

/// fetch_unseen retrieves messages for UIDs not in the seen set.
#[tokio::test]
async fn fetch_unseen_retrieves_new_only() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        (
            "UIDL",
            "+OK\r\n1 uid-aaa\r\n2 uid-bbb\r\n3 uid-ccc\r\n.\r\n",
        ),
        ("RETR 2", "+OK\r\nSubject: New msg B\r\n\r\nBody B\r\n.\r\n"),
        ("RETR 3", "+OK\r\nSubject: New msg C\r\n\r\nBody C\r\n.\r\n"),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();
    client.login("user", "pass").await.unwrap();

    let mut seen = HashSet::new();
    seen.insert("uid-aaa".to_string());

    let results = client.fetch_unseen(&seen).await.unwrap();
    assert_eq!(results.len(), 2);

    // Results are in order of unseen entries
    assert_eq!(results[0].0.unique_id, "uid-bbb");
    assert!(results[0].1.data.contains("Body B"));

    assert_eq!(results[1].0.unique_id, "uid-ccc");
    assert!(results[1].1.data.contains("Body C"));

    client.quit().await.unwrap();
}

/// prune_seen removes UIDs from the seen set that no longer exist on the server.
#[tokio::test]
async fn prune_seen_removes_ghosts() {
    let addr = spawn_mock_server(vec![
        ("USER user", "+OK\r\n"),
        ("PASS pass", "+OK\r\n"),
        ("CAPA", "+OK\r\n.\r\n"),
        // Server only has uid-aaa and uid-ccc; uid-bbb was deleted
        ("UIDL", "+OK\r\n1 uid-aaa\r\n2 uid-ccc\r\n.\r\n"),
        ("QUIT", "+OK\r\n"),
    ])
    .await;

    let mut client = Pop3Client::connect(addr.as_str(), std::time::Duration::from_secs(5))
        .await
        .unwrap();
    client.login("user", "pass").await.unwrap();

    let mut seen = HashSet::new();
    seen.insert("uid-aaa".to_string());
    seen.insert("uid-bbb".to_string()); // ghost — no longer on server
    seen.insert("uid-ccc".to_string());

    let pruned = client.prune_seen(&mut seen).await.unwrap();

    assert_eq!(pruned.len(), 1);
    assert_eq!(pruned[0], "uid-bbb");
    assert_eq!(seen.len(), 2);
    assert!(seen.contains("uid-aaa"));
    assert!(seen.contains("uid-ccc"));
    assert!(!seen.contains("uid-bbb"));

    client.quit().await.unwrap();
}
