//! Connection pool for managing multiple POP3 mailbox accounts concurrently.
//!
//! This module provides a bb8-backed connection pool that enforces RFC 1939's
//! exclusive mailbox access constraint. Each account gets its own pool capped
//! at one connection, preventing protocol violations while allowing concurrent
//! access across different accounts.
//!
//! # Feature Flag
//!
//! This module requires the `pool` feature flag:
//!
//! ```toml
//! [dependencies]
//! pop3 = { version = "2", features = ["pool"] }
//! ```

use std::future::Future;

use crate::builder::Pop3ClientBuilder;
use crate::client::Pop3Client;
use crate::error::Pop3Error;

/// Identifies a unique POP3 mailbox account for pool management.
///
/// Two accounts are considered the same if they share the same host, port,
/// and username. The pool uses this key to enforce one-connection-per-mailbox.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountKey {
    /// POP3 server hostname.
    pub host: String,
    /// POP3 server port.
    pub port: u16,
    /// Mailbox username.
    pub username: String,
}

impl AccountKey {
    /// Create a new account key.
    pub fn new(host: impl Into<String>, port: u16, username: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            username: username.into(),
        }
    }
}

/// Manages POP3 client connections for a single account via bb8.
///
/// Each manager stores a cloned [`Pop3ClientBuilder`] and credentials.
/// On [`connect()`](bb8::ManageConnection::connect), it clones the builder,
/// connects, and authenticates via USER/PASS.
pub struct Pop3ConnectionManager {
    builder: Pop3ClientBuilder,
    username: String,
    password: String,
}

impl Pop3ConnectionManager {
    /// Create a new connection manager for the given builder and credentials.
    pub fn new(
        builder: Pop3ClientBuilder,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            builder,
            username: username.into(),
            password: password.into(),
        }
    }
}

impl bb8::ManageConnection for Pop3ConnectionManager {
    type Connection = Pop3Client;
    type Error = Pop3Error;

    fn connect(&self) -> impl Future<Output = Result<Pop3Client, Pop3Error>> + Send {
        let builder = self.builder.clone();
        let username = self.username.clone();
        let password = self.password.clone();
        async move {
            let mut client = builder.connect().await?;
            client.login(&username, &password).await?;
            Ok(client)
        }
    }

    fn is_valid(
        &self,
        conn: &mut Pop3Client,
    ) -> impl Future<Output = Result<(), Pop3Error>> + Send {
        conn.noop()
    }

    fn has_broken(&self, conn: &mut Pop3Client) -> bool {
        conn.is_closed()
    }
}

/// Errors specific to pool operations.
///
/// Distinct from [`Pop3Error`] because pool-level errors (checkout timeout)
/// are conceptually different from POP3 protocol errors.
#[derive(Debug, thiserror::Error)]
pub enum Pop3PoolError {
    /// The pool checkout timed out waiting for an available connection.
    #[error("pool checkout timed out")]
    CheckoutTimeout,
    /// A POP3 connection-level error occurred.
    #[error("pool connection error: {0}")]
    Connection(#[source] Pop3Error),
}

impl From<bb8::RunError<Pop3Error>> for Pop3PoolError {
    fn from(e: bb8::RunError<Pop3Error>) -> Self {
        match e {
            bb8::RunError::TimedOut => Pop3PoolError::CheckoutTimeout,
            bb8::RunError::User(e) => Pop3PoolError::Connection(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use bb8::ManageConnection as _;

    use super::*;
    use crate::client::build_authenticated_mock_client;

    fn make_key(host: &str, port: u16, username: &str) -> AccountKey {
        AccountKey::new(host, port, username)
    }

    fn hash_of(key: &AccountKey) -> u64 {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    // --- AccountKey tests ---

    #[test]
    fn account_key_eq_same() {
        let k1 = make_key("mail.example.com", 110, "alice");
        let k2 = make_key("mail.example.com", 110, "alice");
        assert_eq!(k1, k2);
    }

    #[test]
    fn account_key_ne_different_host() {
        let k1 = make_key("mail.example.com", 110, "alice");
        let k2 = make_key("mail.other.com", 110, "alice");
        assert_ne!(k1, k2);
    }

    #[test]
    fn account_key_ne_different_port() {
        let k1 = make_key("mail.example.com", 110, "alice");
        let k2 = make_key("mail.example.com", 995, "alice");
        assert_ne!(k1, k2);
    }

    #[test]
    fn account_key_ne_different_username() {
        let k1 = make_key("mail.example.com", 110, "alice");
        let k2 = make_key("mail.example.com", 110, "bob");
        assert_ne!(k1, k2);
    }

    #[test]
    fn account_key_hash_consistent() {
        let k1 = make_key("mail.example.com", 110, "alice");
        let k2 = make_key("mail.example.com", 110, "alice");
        assert_eq!(hash_of(&k1), hash_of(&k2));
    }

    #[test]
    fn account_key_debug() {
        let key = make_key("mail.example.com", 110, "alice");
        let s = format!("{:?}", key);
        assert!(!s.is_empty());
    }

    #[test]
    fn account_key_clone() {
        let key = make_key("mail.example.com", 110, "alice");
        let cloned = key.clone();
        assert_eq!(key, cloned);
    }

    // --- Pop3PoolError tests ---

    #[test]
    fn pool_error_from_timed_out() {
        let run_err: bb8::RunError<Pop3Error> = bb8::RunError::TimedOut;
        let pool_err = Pop3PoolError::from(run_err);
        assert!(matches!(pool_err, Pop3PoolError::CheckoutTimeout));
    }

    #[test]
    fn pool_error_from_user() {
        let pop3_err = Pop3Error::AuthFailed("bad".into());
        let run_err: bb8::RunError<Pop3Error> = bb8::RunError::User(pop3_err);
        let pool_err = Pop3PoolError::from(run_err);
        assert!(matches!(
            pool_err,
            Pop3PoolError::Connection(Pop3Error::AuthFailed(_))
        ));
    }

    #[test]
    fn pool_error_display_timeout() {
        let err = Pop3PoolError::CheckoutTimeout;
        assert_eq!(err.to_string(), "pool checkout timed out");
    }

    #[test]
    fn pool_error_display_connection() {
        let err = Pop3PoolError::Connection(Pop3Error::ConnectionClosed);
        assert_eq!(err.to_string(), "pool connection error: connection closed");
    }

    // --- has_broken tests ---

    #[tokio::test]
    async fn has_broken_returns_false_for_live_client() {
        let mock = tokio_test::io::Builder::new().build();
        let mut client = build_authenticated_mock_client(mock);
        let manager =
            Pop3ConnectionManager::new(Pop3ClientBuilder::new("localhost"), "user", "pass");
        assert!(!manager.has_broken(&mut client));
    }

    #[tokio::test]
    async fn has_broken_returns_true_for_closed_client() {
        // Provide a mock that expects NOOP but returns no data (EOF).
        // When noop() is called, read_line() hits EOF and sets is_closed = true.
        let mock = tokio_test::io::Builder::new().write(b"NOOP\r\n").build();
        let mut client = build_authenticated_mock_client(mock);
        // Trigger EOF: noop() sends command but server closes (no response)
        let _ = client.noop().await;
        assert!(client.is_closed());
        let manager =
            Pop3ConnectionManager::new(Pop3ClientBuilder::new("localhost"), "user", "pass");
        assert!(manager.has_broken(&mut client));
    }
}
