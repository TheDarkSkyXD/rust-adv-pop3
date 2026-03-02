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

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};
use std::time::Duration;

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
    /// The requested account has not been registered with the pool.
    #[error("unknown account: {0:?}")]
    UnknownAccount(AccountKey),
}

impl From<bb8::RunError<Pop3Error>> for Pop3PoolError {
    fn from(e: bb8::RunError<Pop3Error>) -> Self {
        match e {
            bb8::RunError::TimedOut => Pop3PoolError::CheckoutTimeout,
            bb8::RunError::User(e) => Pop3PoolError::Connection(e),
        }
    }
}

/// Configuration for pool behavior applied to every per-account pool.
///
/// All durations have sensible defaults suitable for POP3 workloads.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum time to wait for a connection checkout (default: 30 seconds).
    pub connection_timeout: Duration,
    /// Idle connections are closed after this duration (default: 5 minutes).
    ///
    /// Set to `None` to disable idle timeout.
    pub idle_timeout: Option<Duration>,
    /// Connections older than this are closed (default: 30 minutes).
    ///
    /// Set to `None` to disable max lifetime.
    pub max_lifetime: Option<Duration>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            connection_timeout: Duration::from_secs(30),
            idle_timeout: Some(Duration::from_secs(300)),
            max_lifetime: Some(Duration::from_secs(1800)),
        }
    }
}

/// A checked-out POP3 connection that returns to the pool on drop.
///
/// This type implements `Deref<Target = Pop3Client>` and
/// `DerefMut<Target = Pop3Client>`, so all `Pop3Client` methods are
/// available directly on the guard.
///
/// The connection is automatically returned to the pool when this value
/// is dropped. The next caller blocked on [`Pop3Pool::checkout()`] for
/// the same account will then receive it.
pub type PooledConnection = bb8::PooledConnection<'static, Pop3ConnectionManager>;

/// A connection pool for managing multiple POP3 mailbox accounts concurrently.
///
/// # RFC 1939 Exclusive Mailbox Access
///
/// **POP3 forbids concurrent access to the same mailbox.** Per RFC 1939 section 8:
///
/// > "the POP3 server then acquires an exclusive-access lock on the maildrop,
/// > necessary to prevent messages from being overwritten by stranded
/// > retrievals, and stranded removes."
///
/// > "If the maildrop cannot be opened for some reason (for example, a lock
/// > can not be acquired, the user is denied access to the maildrop, or the
/// > maildrop cannot be parsed), the POP3 server responds with a negative
/// > status indicator."
///
/// This pool enforces that constraint at the library level: each mailbox
/// account is backed by an independent pool capped at **one connection**.
/// A caller attempting to check out a connection to an account that is already
/// in use will **wait asynchronously** until the previous caller drops their
/// [`PooledConnection`].
///
/// This is a **per-account** model, not a traditional N-connection pool.
/// Multiple accounts can be accessed concurrently; a single account cannot.
///
/// # Usage
///
/// ```no_run
/// use pop3::pool::{Pop3Pool, AccountKey};
/// use pop3::Pop3ClientBuilder;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let pool = Pop3Pool::new();
///
///     // Register an account
///     let key = AccountKey::new("pop.example.com", 110, "alice");
///     pool.add_account(
///         key.clone(),
///         Pop3ClientBuilder::new("pop.example.com").port(110),
///         "alice",
///         "secret",
///     );
///
///     // Check out a connection (blocks if already in use by another task)
///     let mut conn = pool.checkout(&key).await?;
///     let stat = conn.stat().await?;
///     println!("{} messages", stat.message_count);
///     // Connection returns to pool when `conn` drops
///
///     Ok(())
/// }
/// ```
pub struct Pop3Pool {
    pools: RwLock<HashMap<AccountKey, Arc<bb8::Pool<Pop3ConnectionManager>>>>,
    config: PoolConfig,
}

impl Pop3Pool {
    /// Create a new pool with default configuration.
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            config: PoolConfig::default(),
        }
    }

    /// Create a new pool with custom configuration.
    pub fn with_config(config: PoolConfig) -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Register a POP3 account with the pool.
    ///
    /// The pool creates a per-account bb8 pool capped at one connection.
    /// Connections are created lazily on first [`checkout()`](Self::checkout).
    ///
    /// If the account is already registered, this is a no-op (idempotent).
    pub fn add_account(
        &self,
        key: AccountKey,
        builder: Pop3ClientBuilder,
        username: impl Into<String>,
        password: impl Into<String>,
    ) {
        let mut pools = self.pools.write().expect("pool registry lock poisoned");
        if pools.contains_key(&key) {
            return; // idempotent
        }
        let manager = Pop3ConnectionManager::new(builder, username, password);
        let pool = bb8::Pool::builder()
            .max_size(1)
            .min_idle(Some(0))
            .test_on_check_out(true)
            .retry_connection(false)
            .connection_timeout(self.config.connection_timeout)
            .idle_timeout(self.config.idle_timeout)
            .max_lifetime(self.config.max_lifetime)
            .build_unchecked(manager);
        pools.insert(key, Arc::new(pool));
    }

    /// Check out a connection for the given account.
    ///
    /// Returns a [`PooledConnection`] that automatically returns to the pool
    /// when dropped. If the account's connection is already checked out,
    /// this waits asynchronously until it becomes available (up to
    /// `connection_timeout` from [`PoolConfig`]).
    ///
    /// # Errors
    ///
    /// - [`Pop3PoolError::UnknownAccount`] — if the key has not been registered
    /// - [`Pop3PoolError::CheckoutTimeout`] — if the checkout times out
    /// - [`Pop3PoolError::Connection`] — if connecting/authenticating fails
    pub async fn checkout(&self, key: &AccountKey) -> Result<PooledConnection, Pop3PoolError> {
        let pool = {
            let pools = self.pools.read().expect("pool registry lock poisoned");
            pools
                .get(key)
                .cloned() // clones the Arc, not the pool
                .ok_or_else(|| Pop3PoolError::UnknownAccount(key.clone()))?
        };
        // RwLock guard is dropped here — safe to await
        pool.get_owned().await.map_err(Pop3PoolError::from)
    }

    /// Remove a registered account from the pool.
    ///
    /// Returns `true` if the account was present and removed, `false` if the
    /// account was not registered.
    ///
    /// Existing [`PooledConnection`] handles checked out from this account
    /// continue to work until dropped. They just won't return to any pool
    /// (they are discarded on drop instead).
    pub fn remove_account(&self, key: &AccountKey) -> bool {
        let mut pools = self.pools.write().expect("pool registry lock poisoned");
        pools.remove(key).is_some()
    }

    /// Return the list of currently registered account keys.
    pub fn accounts(&self) -> Vec<AccountKey> {
        let pools = self.pools.read().expect("pool registry lock poisoned");
        pools.keys().cloned().collect()
    }
}

impl Default for Pop3Pool {
    fn default() -> Self {
        Self::new()
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

    // --- PoolConfig tests ---

    #[test]
    fn pool_config_default() {
        let cfg = PoolConfig::default();
        assert_eq!(cfg.connection_timeout, Duration::from_secs(30));
        assert_eq!(cfg.idle_timeout, Some(Duration::from_secs(300)));
        assert_eq!(cfg.max_lifetime, Some(Duration::from_secs(1800)));
    }

    // --- Pop3Pool tests ---

    #[test]
    fn pool_new_has_no_accounts() {
        let pool = Pop3Pool::new();
        assert!(pool.accounts().is_empty());
    }

    // NOTE: add_account calls bb8::Pool::build_unchecked() which starts a Tokio
    // interval timer internally, requiring a Tokio runtime context. All tests
    // that call add_account must therefore use #[tokio::test].

    #[tokio::test]
    async fn pool_add_account_registers_key() {
        let pool = Pop3Pool::new();
        let key = AccountKey::new("host", 110, "user");
        pool.add_account(
            key.clone(),
            Pop3ClientBuilder::new("host").port(110),
            "user",
            "pass",
        );
        let accounts = pool.accounts();
        assert_eq!(accounts.len(), 1);
        assert!(accounts.contains(&key));
    }

    #[tokio::test]
    async fn pool_add_account_is_idempotent() {
        let pool = Pop3Pool::new();
        let key = AccountKey::new("host", 110, "user");
        pool.add_account(
            key.clone(),
            Pop3ClientBuilder::new("host").port(110),
            "user",
            "pass",
        );
        // Adding again should not panic and accounts should still have one entry
        pool.add_account(
            key.clone(),
            Pop3ClientBuilder::new("host").port(110),
            "user",
            "pass",
        );
        assert_eq!(pool.accounts().len(), 1);
    }

    #[tokio::test]
    async fn pool_add_multiple_accounts() {
        let pool = Pop3Pool::new();
        let k1 = AccountKey::new("host1", 110, "alice");
        let k2 = AccountKey::new("host2", 110, "bob");
        let k3 = AccountKey::new("host3", 995, "carol");
        pool.add_account(k1.clone(), Pop3ClientBuilder::new("host1"), "alice", "pw");
        pool.add_account(k2.clone(), Pop3ClientBuilder::new("host2"), "bob", "pw");
        pool.add_account(k3.clone(), Pop3ClientBuilder::new("host3"), "carol", "pw");
        let accounts = pool.accounts();
        assert_eq!(accounts.len(), 3);
        assert!(accounts.contains(&k1));
        assert!(accounts.contains(&k2));
        assert!(accounts.contains(&k3));
    }

    #[tokio::test]
    async fn pool_remove_account_returns_true() {
        let pool = Pop3Pool::new();
        let key = AccountKey::new("host", 110, "user");
        pool.add_account(key.clone(), Pop3ClientBuilder::new("host"), "user", "pass");
        assert!(pool.remove_account(&key));
    }

    #[test]
    fn pool_remove_account_returns_false() {
        let pool = Pop3Pool::new();
        let key = AccountKey::new("host", 110, "user");
        // Never added — no Tokio runtime needed since we skip add_account
        assert!(!pool.remove_account(&key));
    }

    #[tokio::test]
    async fn pool_remove_account_deregisters() {
        let pool = Pop3Pool::new();
        let key = AccountKey::new("host", 110, "user");
        pool.add_account(key.clone(), Pop3ClientBuilder::new("host"), "user", "pass");
        pool.remove_account(&key);
        let accounts = pool.accounts();
        assert!(!accounts.contains(&key));
        assert!(accounts.is_empty());
    }

    #[tokio::test]
    async fn pool_checkout_unknown_account() {
        let pool = Pop3Pool::new();
        let key = AccountKey::new("host", 110, "user");
        let result = pool.checkout(&key).await;
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, Pop3PoolError::UnknownAccount(_)));
        }
    }

    #[test]
    fn pool_default_impl() {
        let pool = Pop3Pool::default();
        assert!(pool.accounts().is_empty());
    }

    #[test]
    fn pool_with_config() {
        let cfg = PoolConfig {
            connection_timeout: Duration::from_secs(10),
            idle_timeout: None,
            max_lifetime: None,
        };
        let pool = Pop3Pool::with_config(cfg.clone());
        // No public accessor for config, but we can verify accounts is empty
        // and it doesn't panic on basic operations
        assert!(pool.accounts().is_empty());
    }

    #[test]
    fn pool_error_unknown_account_display() {
        let key = AccountKey::new("mail.example.com", 110, "alice");
        let err = Pop3PoolError::UnknownAccount(key);
        let s = err.to_string();
        assert!(s.contains("unknown account"));
        assert!(s.contains("mail.example.com"));
        assert!(s.contains("alice"));
    }
}
