//! Connection pool for managing multiple POP3 accounts concurrently.
//!
//! # RFC 1939 Exclusive-Lock Constraint
//!
//! **POP3 forbids concurrent access to the same mailbox.** Per RFC 1939 section 8,
//! a POP3 server acquires an exclusive lock when a session enters the TRANSACTION
//! state. A second connection to the same mailbox will be rejected with
//! `-ERR maildrop already locked`. This pool enforces the constraint by capping
//! each per-account pool at `max_size(1)` — at most one live connection exists
//! per mailbox at any time.
//!
//! This pool is a **registry of per-account pools**, not a traditional N-connection
//! pool to a single server. Each account (identified by host, port, and username)
//! gets its own [`bb8::Pool`] with a single-connection limit. Different accounts
//! can be checked out concurrently without blocking each other.
//!
//! # Usage
//!
//! ```no_run
//! use pop3::{Pop3ClientBuilder, pool::{Pop3Pool, Pop3PoolConfig}};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let pool = Pop3Pool::new(Pop3PoolConfig::default());
//!
//!     // Register an account
//!     let builder = Pop3ClientBuilder::new("pop.example.com")
//!         .credentials("alice", "secret");
//!     pool.add_account(builder).await?;
//!
//!     // Check out a connection — authenticated and health-checked
//!     let mut conn = pool.get("pop.example.com", 110, "alice").await?;
//!     let stat = conn.stat().await?;
//!     println!("{} messages", stat.message_count);
//!     // Connection returned to pool on drop
//!     Ok(())
//! }
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::builder::Pop3ClientBuilder;
use crate::client::Pop3Client;

/// Identifies a POP3 account by connection parameters.
///
/// The key is derived from the hostname, port, and username configured on
/// the [`Pop3ClientBuilder`] when `add_account` is called. Two builders with
/// the same host/port/username produce the same key and share a connection pool.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountKey {
    /// The POP3 server hostname.
    pub host: String,
    /// The TCP port.
    pub port: u16,
    /// The username used for authentication.
    pub username: String,
}

impl fmt::Display for AccountKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}:{}", self.username, self.host, self.port)
    }
}

/// Configuration for a [`Pop3Pool`].
///
/// Controls checkout timeout and health-check behavior. Use
/// [`Pop3PoolConfig::default()`] for sensible defaults.
#[derive(Debug, Clone)]
pub struct Pop3PoolConfig {
    /// Maximum time to wait for a connection to become available on checkout.
    ///
    /// Defaults to 30 seconds. If the connection is not available within this
    /// duration, `get()` returns a [`Pop3PoolError::Pool`] error wrapping
    /// [`bb8::RunError::TimedOut`].
    pub connection_timeout: Duration,

    /// Whether to send a NOOP health-check on every checkout.
    ///
    /// Defaults to `true`. When enabled, each checkout sends a `NOOP` command
    /// to verify the connection is live before returning it to the caller. A
    /// failed NOOP causes bb8 to discard the connection and create a fresh one.
    pub test_on_check_out: bool,
}

impl Default for Pop3PoolConfig {
    fn default() -> Self {
        Self {
            connection_timeout: Duration::from_secs(30),
            test_on_check_out: true,
        }
    }
}

/// Errors that can occur during pool operations.
///
/// Distinct variants allow callers to distinguish between connection-level
/// failures, checkout timeouts, missing credentials, and unknown accounts.
#[derive(Debug, thiserror::Error)]
pub enum Pop3PoolError {
    /// A POP3 client-level error (I/O, TLS, auth failure, etc.).
    #[error("POP3 client error: {0}")]
    Client(#[from] crate::Pop3Error),

    /// A bb8 pool-level error (checkout timed out or connection failed).
    ///
    /// Wraps [`bb8::RunError`]. Note: [`bb8::RunError::User`] containing an
    /// auth failure is mapped to [`Pop3PoolError::Client`] by the manual
    /// `From` impl for ergonomic error matching.
    #[error("pool error: {0}")]
    Pool(#[source] bb8::RunError<crate::Pop3Error>),

    /// The builder passed to `add_account` had no credentials configured.
    ///
    /// The pool requires credentials so connections can be authenticated
    /// automatically on creation. Call `.credentials()` or `.apop()` on the
    /// builder before passing it to `add_account`.
    #[error("no credentials configured on builder for account {0}")]
    NoCredentials(AccountKey),

    /// The account key was not found in the pool registry.
    ///
    /// Call `add_account` with a builder for this account before calling `get`.
    #[error("account not found: {0}")]
    AccountNotFound(AccountKey),
}

impl From<bb8::RunError<crate::Pop3Error>> for Pop3PoolError {
    fn from(err: bb8::RunError<crate::Pop3Error>) -> Self {
        match err {
            bb8::RunError::User(pop3_err) => Pop3PoolError::Client(pop3_err),
            bb8::RunError::TimedOut => Pop3PoolError::Pool(bb8::RunError::TimedOut),
        }
    }
}

/// bb8 connection manager that creates authenticated [`Pop3Client`] connections.
///
/// Each `Pop3ConnectionManager` holds a [`Pop3ClientBuilder`] that encapsulates
/// the server address, TLS mode, and credentials. When bb8 needs a new connection,
/// `connect()` calls `builder.clone().connect()` which establishes the TCP
/// connection, performs the TLS handshake if configured, and authenticates.
///
/// Health checks are performed via `is_valid()` (sends `NOOP`) and broken
/// detection via `has_broken()` (checks [`Pop3Client::is_closed()`]).
#[derive(Debug, Clone)]
pub struct Pop3ConnectionManager {
    builder: Pop3ClientBuilder,
}

impl Pop3ConnectionManager {
    /// Creates a new connection manager from a configured builder.
    ///
    /// The builder must have credentials set (`.credentials()` or `.apop()`).
    /// The manager clones the builder on each `connect()` call to produce an
    /// independent connection.
    pub fn new(builder: Pop3ClientBuilder) -> Self {
        Self { builder }
    }
}

impl bb8::ManageConnection for Pop3ConnectionManager {
    type Connection = Pop3Client;
    type Error = crate::Pop3Error;

    /// Creates a new authenticated [`Pop3Client`] connection.
    ///
    /// Calls `builder.clone().connect()`, which establishes the TCP/TLS
    /// connection and authenticates with the configured credentials before
    /// returning.
    async fn connect(&self) -> Result<Pop3Client, crate::Pop3Error> {
        self.builder.clone().connect().await
    }

    /// Validates a connection by sending a `NOOP` command.
    ///
    /// bb8 calls this before returning a connection to the caller when
    /// `test_on_check_out` is enabled. A failed `NOOP` causes bb8 to discard
    /// the connection and attempt to create a new one.
    async fn is_valid(&self, conn: &mut Pop3Client) -> Result<(), crate::Pop3Error> {
        conn.noop().await
    }

    /// Returns `true` if the connection is known to be closed.
    ///
    /// bb8 calls this when returning a connection to the pool. If `true`,
    /// the connection is discarded rather than returned to the idle pool.
    fn has_broken(&self, conn: &mut Pop3Client) -> bool {
        conn.is_closed()
    }
}

/// A connection pool for managing multiple POP3 accounts concurrently.
///
/// # RFC 1939 Exclusive-Lock Constraint
///
/// **POP3 forbids concurrent access to the same mailbox.** Each account in
/// this pool is backed by a [`bb8::Pool`] with `max_size(1)`. A second
/// checkout for the same account blocks until the first connection is
/// returned. Different accounts can be checked out concurrently.
///
/// See the [module-level documentation](self) for details.
///
/// # Thread Safety
///
/// `Pop3Pool` is `Send + Sync` and can be shared across tasks via `Arc`.
pub struct Pop3Pool {
    pools: tokio::sync::RwLock<HashMap<AccountKey, Arc<bb8::Pool<Pop3ConnectionManager>>>>,
    config: Pop3PoolConfig,
}

impl fmt::Debug for Pop3Pool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pop3Pool")
            .field("config", &self.config)
            .field("pools", &"<RwLock<HashMap<...>>>")
            .finish()
    }
}

impl Pop3Pool {
    /// Creates a new, empty pool registry with the given configuration.
    ///
    /// No accounts are registered until `add_account` is called. No connections
    /// are established until `get` is called for a registered account.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::pool::{Pop3Pool, Pop3PoolConfig};
    ///
    /// let pool = Pop3Pool::new(Pop3PoolConfig::default());
    /// ```
    pub fn new(config: Pop3PoolConfig) -> Self {
        Self {
            pools: tokio::sync::RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Registers a POP3 account with the pool.
    ///
    /// The builder must have credentials configured via `.credentials()` or
    /// `.apop()`. The account is identified by the (host, port, username) tuple
    /// derived from the builder. If an account with the same key is already
    /// registered, the existing registration is kept (idempotent).
    ///
    /// No connection is established at registration time — connections are
    /// created lazily on the first `get()` call.
    ///
    /// # Errors
    ///
    /// Returns [`Pop3PoolError::NoCredentials`] if the builder has no auth
    /// configured (`AuthMode::None`).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::{Pop3ClientBuilder, pool::{Pop3Pool, Pop3PoolConfig}};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let pool = Pop3Pool::new(Pop3PoolConfig::default());
    /// let builder = Pop3ClientBuilder::new("pop.example.com")
    ///     .credentials("alice", "secret");
    /// let key = pool.add_account(builder).await?;
    /// println!("Registered: {}", key);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add_account(
        &self,
        builder: Pop3ClientBuilder,
    ) -> Result<AccountKey, Pop3PoolError> {
        let username = builder
            .username()
            .ok_or_else(|| {
                let key = AccountKey {
                    host: builder.hostname().to_string(),
                    port: builder.effective_port(),
                    username: String::new(),
                };
                Pop3PoolError::NoCredentials(key)
            })?
            .to_string();

        let key = AccountKey {
            host: builder.hostname().to_string(),
            port: builder.effective_port(),
            username,
        };

        let manager = Pop3ConnectionManager::new(builder);
        let pool = Arc::new(
            bb8::Pool::builder()
                .max_size(1)
                .min_idle(None)
                .retry_connection(false)
                .test_on_check_out(self.config.test_on_check_out)
                .connection_timeout(self.config.connection_timeout)
                .build_unchecked(manager),
        );

        let mut guard = self.pools.write().await;
        guard.entry(key.clone()).or_insert(pool);
        Ok(key)
    }

    /// Checks out a connection for the given account.
    ///
    /// Looks up the per-account pool by (host, port, username) key. If a
    /// connection is available in the pool, it is returned immediately (with
    /// an optional NOOP health-check if `test_on_check_out` is enabled). If
    /// no connection is available (the single connection is in use), this
    /// method blocks until one becomes available or `connection_timeout` elapses.
    ///
    /// The returned [`bb8::PooledConnection`] auto-returns the connection to
    /// the pool when dropped.
    ///
    /// # Errors
    ///
    /// - [`Pop3PoolError::AccountNotFound`] — `add_account` was not called for
    ///   this (host, port, username) combination.
    /// - [`Pop3PoolError::Client`] — connection or authentication failed.
    /// - [`Pop3PoolError::Pool`] — checkout timed out.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::pool::{Pop3Pool, Pop3PoolConfig};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let pool = Pop3Pool::new(Pop3PoolConfig::default());
    /// let mut conn = pool.get("pop.example.com", 110, "alice").await?;
    /// let stat = conn.stat().await?;
    /// println!("{} messages", stat.message_count);
    /// // conn returned to pool on drop
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get(
        &self,
        host: &str,
        port: u16,
        username: &str,
    ) -> Result<bb8::PooledConnection<'static, Pop3ConnectionManager>, Pop3PoolError> {
        let key = AccountKey {
            host: host.to_string(),
            port,
            username: username.to_string(),
        };

        let pool = {
            let guard = self.pools.read().await;
            guard.get(&key).map(Arc::clone)
        };

        match pool {
            Some(p) => {
                let conn = p.get_owned().await.map_err(Pop3PoolError::from)?;
                Ok(conn)
            }
            None => Err(Pop3PoolError::AccountNotFound(key)),
        }
    }

    /// Removes a previously registered account from the pool.
    ///
    /// Returns `true` if the account was present and removed, `false` if it
    /// was not registered. After removal, `get()` calls for the same key will
    /// return [`Pop3PoolError::AccountNotFound`].
    ///
    /// Any connections currently checked out from the removed pool continue to
    /// function until dropped. No new connections can be checked out after
    /// removal.
    pub async fn remove_account(&self, host: &str, port: u16, username: &str) -> bool {
        let key = AccountKey {
            host: host.to_string(),
            port,
            username: username.to_string(),
        };
        let mut guard = self.pools.write().await;
        guard.remove(&key).is_some()
    }

    /// Returns a snapshot of all currently registered account keys.
    ///
    /// The returned `Vec` reflects the state at the moment of the call. Keys
    /// registered or removed after this call are not reflected.
    pub async fn accounts(&self) -> Vec<AccountKey> {
        let guard = self.pools.read().await;
        guard.keys().cloned().collect()
    }

    /// Returns the number of registered accounts.
    pub async fn pool_count(&self) -> usize {
        let guard = self.pools.read().await;
        guard.len()
    }

    /// Returns `true` if the given account is registered in the pool.
    pub async fn contains_account(&self, host: &str, port: u16, username: &str) -> bool {
        let key = AccountKey {
            host: host.to_string(),
            port,
            username: username.to_string(),
        };
        let guard = self.pools.read().await;
        guard.contains_key(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;

    // =========================================================================
    // Mock infrastructure for bb8 pool behavior tests (no POP3 involved)
    // =========================================================================

    #[derive(Debug, Default)]
    struct FakeConn;

    #[derive(Debug)]
    struct FakeError;

    impl std::fmt::Display for FakeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "fake error")
        }
    }

    impl std::error::Error for FakeError {}

    struct AlwaysOkManager;

    impl bb8::ManageConnection for AlwaysOkManager {
        type Connection = FakeConn;
        type Error = FakeError;

        fn connect(&self) -> impl Future<Output = Result<Self::Connection, Self::Error>> + Send {
            async { Ok(FakeConn) }
        }

        fn is_valid(
            &self,
            _conn: &mut Self::Connection,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send {
            async { Ok(()) }
        }

        fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
            false
        }
    }

    struct AlwaysBrokenManager;

    impl bb8::ManageConnection for AlwaysBrokenManager {
        type Connection = FakeConn;
        type Error = FakeError;

        fn connect(&self) -> impl Future<Output = Result<Self::Connection, Self::Error>> + Send {
            async { Ok(FakeConn) }
        }

        fn is_valid(
            &self,
            _conn: &mut Self::Connection,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send {
            async { Ok(()) }
        }

        fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
            true
        }
    }

    // =========================================================================
    // Group 1: Pop3ConnectionManager unit tests
    // =========================================================================

    mod manager_tests {
        use super::*;
        use tokio_test::io::Builder;

        fn make_manager() -> Pop3ConnectionManager {
            let builder = crate::builder::Pop3ClientBuilder::new("pop.example.com")
                .credentials("alice", "secret");
            Pop3ConnectionManager::new(builder)
        }

        #[tokio::test]
        async fn manager_is_valid_sends_noop_and_succeeds() {
            let mock = Builder::new().write(b"NOOP\r\n").read(b"+OK\r\n").build();
            let mut client = crate::client::build_authenticated_mock_client(mock);
            let manager = make_manager();
            manager.is_valid(&mut client).await.unwrap();
        }

        #[tokio::test]
        async fn manager_is_valid_fails_on_server_error() {
            let mock = Builder::new()
                .write(b"NOOP\r\n")
                .read(b"-ERR server error\r\n")
                .build();
            let mut client = crate::client::build_authenticated_mock_client(mock);
            let manager = make_manager();
            let result = manager.is_valid(&mut client).await;
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn manager_is_valid_fails_on_eof() {
            // Mock that writes NOOP but gets no response (simulates EOF)
            let mock = Builder::new().write(b"NOOP\r\n").build();
            let mut client = crate::client::build_authenticated_mock_client(mock);
            let manager = make_manager();
            let result = manager.is_valid(&mut client).await;
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn manager_has_broken_returns_false_for_live_client() {
            // A fresh authenticated mock client is not closed
            let mock = Builder::new().build();
            let mut client = crate::client::build_authenticated_mock_client(mock);
            let manager = make_manager();
            assert!(!manager.has_broken(&mut client));
        }

        #[tokio::test]
        async fn manager_has_broken_returns_true_for_closed_client() {
            // Write NOOP but provide no response — reading will fail (EOF)
            // which marks the client as closed.
            let mock = Builder::new().write(b"NOOP\r\n").build();
            let mut client = crate::client::build_authenticated_mock_client(mock);
            // Trigger a NOOP which will fail on EOF, marking the connection closed
            let _ = client.noop().await;
            let manager = make_manager();
            assert!(manager.has_broken(&mut client));
        }
    }

    // =========================================================================
    // Group 2: bb8 pool behavior tests (FakeConn / AlwaysOkManager)
    // =========================================================================

    mod pool_behavior_tests {
        use super::*;

        #[tokio::test]
        async fn pool_checkout_succeeds() {
            let pool = bb8::Pool::builder()
                .max_size(1)
                .build(AlwaysOkManager)
                .await
                .unwrap();
            let _conn = pool.get().await.unwrap();
        }

        #[tokio::test]
        async fn pool_connection_returned_on_drop() {
            let pool = bb8::Pool::builder()
                .max_size(1)
                .build(AlwaysOkManager)
                .await
                .unwrap();

            {
                let _conn = pool.get().await.unwrap();
                assert_eq!(pool.state().idle_connections, 0);
                // _conn drops here, returning to pool
            }

            assert_eq!(pool.state().idle_connections, 1);
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn pool_checkout_times_out_when_exhausted() {
            use tokio::sync::oneshot;

            let pool = bb8::Pool::builder()
                .max_size(1)
                .connection_timeout(Duration::from_millis(100))
                .retry_connection(false)
                .build(AlwaysOkManager)
                .await
                .unwrap();

            // Hold the single connection in a spawned task
            let (hold_tx, hold_rx) = oneshot::channel::<()>();
            let pool_clone = pool.clone();
            tokio::spawn(async move {
                let _conn = pool_clone.get().await.unwrap();
                hold_tx.send(()).unwrap();
                // Hold forever
                tokio::time::sleep(Duration::from_secs(10)).await;
            });

            // Wait until the task has the connection
            hold_rx.await.unwrap();

            // Second checkout must time out
            let result = pool.get().await;
            assert!(matches!(result, Err(bb8::RunError::TimedOut)));
        }

        #[tokio::test]
        async fn pool_statistics_track_connections_created() {
            let pool = bb8::Pool::builder()
                .max_size(1)
                .build(AlwaysOkManager)
                .await
                .unwrap();

            let _conn = pool.get().await.unwrap();
            assert_eq!(pool.state().statistics.connections_created, 1);
        }

        #[tokio::test]
        async fn broken_connection_is_discarded_not_returned() {
            let pool = bb8::Pool::builder()
                .max_size(1)
                .build(AlwaysBrokenManager)
                .await
                .unwrap();

            let _conn = pool.get().await.unwrap();
            drop(_conn);

            // bb8 detects has_broken() == true on return, closes the connection
            assert_eq!(pool.state().statistics.connections_closed_broken, 1);
            assert_eq!(pool.state().idle_connections, 0);
        }
    }

    // =========================================================================
    // Group 3: Pop3Pool registry tests
    // =========================================================================

    mod registry_tests {
        use super::*;

        fn make_pool() -> Pop3Pool {
            Pop3Pool::new(Pop3PoolConfig::default())
        }

        fn builder_with_credentials() -> crate::builder::Pop3ClientBuilder {
            crate::builder::Pop3ClientBuilder::new("pop.example.com")
                .port(110)
                .credentials("alice", "secret")
        }

        fn builder_no_credentials() -> crate::builder::Pop3ClientBuilder {
            crate::builder::Pop3ClientBuilder::new("pop.example.com").port(110)
        }

        #[tokio::test]
        async fn add_account_creates_pool() {
            let pool = make_pool();
            let key = pool.add_account(builder_with_credentials()).await.unwrap();
            assert_eq!(key.host, "pop.example.com");
            assert_eq!(key.port, 110);
            assert_eq!(key.username, "alice");
        }

        #[tokio::test]
        async fn add_account_rejects_no_credentials() {
            let pool = make_pool();
            let result = pool.add_account(builder_no_credentials()).await;
            assert!(matches!(result, Err(Pop3PoolError::NoCredentials(_))));
        }

        #[tokio::test]
        async fn get_returns_account_not_found() {
            let pool = make_pool();
            let result = pool.get("unknown.example.com", 110, "nobody").await;
            assert!(matches!(result, Err(Pop3PoolError::AccountNotFound(_))));
        }

        #[tokio::test]
        async fn remove_account_returns_true_if_present() {
            let pool = make_pool();
            pool.add_account(builder_with_credentials()).await.unwrap();
            let removed = pool.remove_account("pop.example.com", 110, "alice").await;
            assert!(removed);
        }

        #[tokio::test]
        async fn remove_account_returns_false_if_absent() {
            let pool = make_pool();
            let removed = pool
                .remove_account("nonexistent.example.com", 110, "nobody")
                .await;
            assert!(!removed);
        }

        #[tokio::test]
        async fn pool_count_tracks_accounts() {
            let pool = make_pool();
            let b1 = crate::builder::Pop3ClientBuilder::new("pop1.example.com")
                .port(110)
                .credentials("alice", "secret");
            let b2 = crate::builder::Pop3ClientBuilder::new("pop2.example.com")
                .port(110)
                .credentials("bob", "secret");
            pool.add_account(b1).await.unwrap();
            pool.add_account(b2).await.unwrap();
            assert_eq!(pool.pool_count().await, 2);
        }

        #[tokio::test]
        async fn contains_account_true_after_add() {
            let pool = make_pool();
            pool.add_account(builder_with_credentials()).await.unwrap();
            assert!(pool.contains_account("pop.example.com", 110, "alice").await);
        }

        #[tokio::test]
        async fn contains_account_false_before_add() {
            let pool = make_pool();
            assert!(!pool.contains_account("pop.example.com", 110, "alice").await);
        }

        #[tokio::test]
        async fn accounts_returns_all_keys() {
            let pool = make_pool();
            let b1 = crate::builder::Pop3ClientBuilder::new("pop1.example.com")
                .port(110)
                .credentials("alice", "secret");
            let b2 = crate::builder::Pop3ClientBuilder::new("pop2.example.com")
                .port(110)
                .credentials("bob", "secret");
            pool.add_account(b1).await.unwrap();
            pool.add_account(b2).await.unwrap();

            let mut keys = pool.accounts().await;
            keys.sort_by(|a, b| a.host.cmp(&b.host));
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0].host, "pop1.example.com");
            assert_eq!(keys[1].host, "pop2.example.com");
        }

        #[tokio::test]
        async fn add_account_is_idempotent_same_key() {
            let pool = make_pool();
            pool.add_account(builder_with_credentials()).await.unwrap();
            pool.add_account(builder_with_credentials()).await.unwrap();
            // Should still be 1 pool, not 2
            assert_eq!(pool.pool_count().await, 1);
        }

        #[tokio::test]
        async fn remove_account_and_contains_returns_false() {
            let pool = make_pool();
            pool.add_account(builder_with_credentials()).await.unwrap();
            pool.remove_account("pop.example.com", 110, "alice").await;
            assert!(!pool.contains_account("pop.example.com", 110, "alice").await);
        }
    }

    // =========================================================================
    // Group 4: AccountKey and error type tests
    // =========================================================================

    mod type_tests {
        use super::*;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn make_key() -> AccountKey {
            AccountKey {
                host: "pop.example.com".to_string(),
                port: 110,
                username: "alice".to_string(),
            }
        }

        #[test]
        fn account_key_display() {
            let key = make_key();
            assert_eq!(format!("{key}"), "alice@pop.example.com:110");
        }

        #[test]
        fn account_key_equality() {
            let k1 = make_key();
            let k2 = make_key();
            assert_eq!(k1, k2);
        }

        #[test]
        fn account_key_inequality_different_username() {
            let k1 = make_key();
            let k2 = AccountKey {
                host: "pop.example.com".to_string(),
                port: 110,
                username: "bob".to_string(),
            };
            assert_ne!(k1, k2);
        }

        #[test]
        fn account_key_hash_consistency() {
            let k1 = make_key();
            let k2 = make_key();

            let mut h1 = DefaultHasher::new();
            k1.hash(&mut h1);
            let hash1 = h1.finish();

            let mut h2 = DefaultHasher::new();
            k2.hash(&mut h2);
            let hash2 = h2.finish();

            assert_eq!(hash1, hash2);
        }

        #[test]
        fn pool_error_from_pop3_error() {
            let pop3_err = crate::Pop3Error::Timeout;
            let pool_err: Pop3PoolError = pop3_err.into();
            assert!(matches!(pool_err, Pop3PoolError::Client(_)));
        }

        #[test]
        fn pool_error_from_run_error_timed_out() {
            let run_err: bb8::RunError<crate::Pop3Error> = bb8::RunError::TimedOut;
            let pool_err: Pop3PoolError = run_err.into();
            assert!(matches!(pool_err, Pop3PoolError::Pool(_)));
        }

        #[test]
        fn pool_error_display_client() {
            let err = Pop3PoolError::Client(crate::Pop3Error::Timeout);
            let s = format!("{err}");
            assert!(!s.is_empty());
            assert!(s.contains("POP3"));
        }

        #[test]
        fn pool_error_display_account_not_found() {
            let key = make_key();
            let err = Pop3PoolError::AccountNotFound(key);
            let s = format!("{err}");
            assert!(s.contains("account not found") || s.contains("not found"));
        }

        #[test]
        fn pool_error_display_no_credentials() {
            let key = make_key();
            let err = Pop3PoolError::NoCredentials(key);
            let s = format!("{err}");
            assert!(s.contains("credentials") || s.contains("no credentials"));
        }

        #[test]
        fn pool_error_display_pool_timed_out() {
            let err = Pop3PoolError::Pool(bb8::RunError::TimedOut);
            let s = format!("{err}");
            assert!(!s.is_empty());
        }

        #[test]
        fn account_key_clone() {
            let k = make_key();
            let k2 = k.clone();
            assert_eq!(k, k2);
        }

        #[test]
        fn pop3_pool_config_default() {
            let cfg = Pop3PoolConfig::default();
            assert_eq!(cfg.connection_timeout, Duration::from_secs(30));
            assert!(cfg.test_on_check_out);
        }
    }
}
