//! Automatic reconnection wrapper for [`Pop3Client`] with exponential backoff and jitter.
//!
//! # Overview
//!
//! [`ReconnectingClient`] wraps a [`Pop3Client`] and transparently reconnects on I/O
//! errors using exponential backoff with jitter (via the `backon` crate). Every method
//! returns `Outcome<T>` so callers know whether pending DELE marks were discarded.
//!
//! # Session-State Loss
//!
//! POP3 DELE marks are committed only on `QUIT`. A reconnect starts a fresh session,
//! silently discarding all pending DELEs. To prevent data-loss bugs, every fallible
//! method on `ReconnectingClient` returns `Result<Outcome<T>>`:
//! - `Fresh(T)` — the existing connection was used; no state was lost
//! - `Reconnected(T)` — a reconnect occurred; all pending DELE marks are gone
//!
//! Callers must handle both variants. Use `outcome.into_inner()` to extract the value
//! when the reconnection signal is intentionally ignored, or `outcome.is_reconnected()`
//! to branch on it.
//!
//! # Error Classification
//!
//! Only transient connection errors trigger a reconnect: [`Pop3Error::Io`],
//! [`Pop3Error::ConnectionClosed`], [`Pop3Error::Timeout`], and [`Pop3Error::SysTemp`]
//! (explicitly transient per RFC 3206).
//!
//! [`Pop3Error::AuthFailed`] is **never** retried — retrying wrong credentials
//! risks account lockout on servers with brute-force protection.
//!
//! # Usage
//!
//! ```no_run
//! use pop3::{Pop3ClientBuilder, ReconnectingClientBuilder, Outcome};
//!
//! #[tokio::main]
//! async fn main() -> pop3::Result<()> {
//!     let mut client = ReconnectingClientBuilder::new(
//!         Pop3ClientBuilder::new("pop.example.com").port(110),
//!     )
//!     .max_retries(5)
//!     .connect("user@example.com", "app-password")
//!     .await?;
//!
//!     let outcome = client.stat().await?;
//!     if outcome.is_reconnected() {
//!         eprintln!("Session was reset — DELE marks discarded");
//!     }
//!     let stat = outcome.into_inner();
//!     client.quit().await?;
//!     Ok(())
//! }
//! ```

use std::collections::HashSet;
use std::time::Duration;

use backon::{ExponentialBuilder, Retryable};

use crate::types::{Capability, ListEntry, Message, SessionState, Stat, UidlEntry};
use crate::{Pop3Client, Pop3ClientBuilder, Pop3Error, Result};

/// Type alias for the optional reconnect callback to reduce type-complexity.
type ReconnectCallback = Option<Box<dyn FnMut(u32, &Pop3Error) + Send>>;

/// The outcome of a [`ReconnectingClient`] operation.
///
/// Every fallible method on `ReconnectingClient` returns `Result<Outcome<T>>`.
/// Callers must inspect the variant to know whether the connection was recycled
/// (which discards any pending DELE marks).
///
/// Use [`into_inner()`](Self::into_inner) to extract the value when the
/// reconnection signal is intentionally ignored, or [`is_reconnected()`](Self::is_reconnected)
/// to branch on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    /// The existing connection was used; no session state was lost.
    Fresh(T),
    /// A reconnect occurred; all pending DELE marks are gone.
    Reconnected(T),
}

impl<T> Outcome<T> {
    /// Extract the inner value, discarding whether a reconnect occurred.
    ///
    /// Use this when you intentionally do not care about session-state loss.
    pub fn into_inner(self) -> T {
        match self {
            Outcome::Fresh(v) | Outcome::Reconnected(v) => v,
        }
    }

    /// Returns `true` if this outcome represents a reconnect (session state lost).
    pub fn is_reconnected(&self) -> bool {
        matches!(self, Outcome::Reconnected(_))
    }
}

/// A fluent builder for creating [`ReconnectingClient`] connections.
///
/// # Example
///
/// ```no_run
/// use pop3::{Pop3ClientBuilder, ReconnectingClientBuilder};
///
/// #[tokio::main]
/// async fn main() -> pop3::Result<()> {
///     let mut client = ReconnectingClientBuilder::new(
///         Pop3ClientBuilder::new("pop.example.com").port(110),
///     )
///     .max_retries(5)
///     .connect("user@example.com", "app-password")
///     .await?;
///     client.quit().await?;
///     Ok(())
/// }
/// ```
pub struct ReconnectingClientBuilder {
    builder: Pop3ClientBuilder,
    max_retries: usize,
    initial_delay: Duration,
    max_delay: Duration,
    jitter: bool,
    on_reconnect: ReconnectCallback,
}

impl ReconnectingClientBuilder {
    /// Create a new builder wrapping the given [`Pop3ClientBuilder`].
    ///
    /// Default settings:
    /// - `max_retries`: 3
    /// - `initial_delay`: 1 second
    /// - `max_delay`: 30 seconds
    /// - `jitter`: enabled
    pub fn new(builder: Pop3ClientBuilder) -> Self {
        ReconnectingClientBuilder {
            builder,
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            jitter: true,
            on_reconnect: None,
        }
    }

    /// Set the maximum number of retry attempts (default: 3).
    pub fn max_retries(mut self, n: usize) -> Self {
        self.max_retries = n;
        self
    }

    /// Set the initial backoff delay (default: 1 second).
    pub fn initial_delay(mut self, d: Duration) -> Self {
        self.initial_delay = d;
        self
    }

    /// Set the maximum backoff delay (default: 30 seconds).
    pub fn max_delay(mut self, d: Duration) -> Self {
        self.max_delay = d;
        self
    }

    /// Enable or disable jitter on backoff delays (default: enabled).
    pub fn jitter(mut self, enabled: bool) -> Self {
        self.jitter = enabled;
        self
    }

    /// Register a callback invoked on each reconnect attempt.
    ///
    /// The callback receives `(attempt: u32, error: &Pop3Error)` where `attempt`
    /// starts at 1. It is informational only — it cannot cancel the retry.
    pub fn on_reconnect(mut self, f: impl FnMut(u32, &Pop3Error) + Send + 'static) -> Self {
        self.on_reconnect = Some(Box::new(f) as Box<dyn FnMut(u32, &Pop3Error) + Send>);
        self
    }

    /// Connect to the POP3 server with automatic retry on transient errors.
    ///
    /// Credentials are passed here (not stored on the builder) to minimise the
    /// time they exist in memory. The initial connection attempt is itself
    /// subject to the configured retry loop.
    ///
    /// # Errors
    ///
    /// Returns the last error if all retry attempts are exhausted, or immediately
    /// for non-retryable errors (e.g., [`Pop3Error::AuthFailed`]).
    pub async fn connect(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<ReconnectingClient> {
        let username = username.into();
        let password = password.into();

        let exp = build_exp_backoff(
            self.max_retries,
            self.initial_delay,
            self.max_delay,
            self.jitter,
        );

        let builder_ref = &self.builder;
        let user = username.as_str();
        let pass = password.as_str();
        let make_conn = || async move { connect_and_auth(builder_ref, user, pass).await };

        let client = if let Some(ref mut cb) = self.on_reconnect {
            let mut attempt: u32 = 0;
            make_conn
                .retry(exp)
                .sleep(tokio::time::sleep)
                .when(is_retryable)
                .notify(|e, _dur| {
                    attempt += 1;
                    cb(attempt, e);
                })
                .await?
        } else {
            make_conn
                .retry(exp)
                .sleep(tokio::time::sleep)
                .when(is_retryable)
                .await?
        };

        Ok(ReconnectingClient {
            builder: self.builder,
            username,
            password,
            client,
            max_retries: self.max_retries,
            initial_delay: self.initial_delay,
            max_delay: self.max_delay,
            jitter: self.jitter,
            on_reconnect: self.on_reconnect,
        })
    }
}

/// A [`Pop3Client`] wrapper that automatically reconnects on transient errors.
///
/// Created via [`ReconnectingClientBuilder::connect()`]. Every fallible method
/// returns `Result<Outcome<T>>` so callers can detect session-state loss.
///
/// See the [module-level documentation](self) for details.
pub struct ReconnectingClient {
    builder: Pop3ClientBuilder,
    username: String,
    password: String,
    /// The active inner client.
    pub(crate) client: Pop3Client,
    max_retries: usize,
    initial_delay: Duration,
    max_delay: Duration,
    jitter: bool,
    on_reconnect: ReconnectCallback,
}

impl ReconnectingClient {
    /// Rebuild `self.client` by connecting and authenticating from scratch.
    ///
    /// Used internally by command wrappers when a transient error is detected
    /// on the active connection.
    pub(crate) async fn do_reconnect(&mut self) -> Result<()> {
        let exp = build_exp_backoff(
            self.max_retries,
            self.initial_delay,
            self.max_delay,
            self.jitter,
        );

        let builder_ref = &self.builder;
        let user = self.username.as_str();
        let pass = self.password.as_str();
        let make_conn = || async move { connect_and_auth(builder_ref, user, pass).await };

        let new_client = if let Some(ref mut cb) = self.on_reconnect {
            let mut attempt: u32 = 0;
            make_conn
                .retry(exp)
                .sleep(tokio::time::sleep)
                .when(is_retryable)
                .notify(|e, _dur| {
                    attempt += 1;
                    cb(attempt, e);
                })
                .await?
        } else {
            make_conn
                .retry(exp)
                .sleep(tokio::time::sleep)
                .when(is_retryable)
                .await?
        };

        self.client = new_client;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Mailbox commands — all return Result<Outcome<T>>
    // -------------------------------------------------------------------------

    /// Get mailbox statistics: total message count and total size in bytes.
    ///
    /// Returns `Outcome::Reconnected(stat)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn stat(&mut self) -> Result<Outcome<Stat>> {
        match self.client.stat().await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.stat().await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    /// List messages with their sizes.
    ///
    /// - `Some(id)` — returns size info for the single message with that number
    /// - `None` — returns a list of all messages with their sizes
    ///
    /// Returns `Outcome::Reconnected(list)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn list(&mut self, message_id: Option<u32>) -> Result<Outcome<Vec<ListEntry>>> {
        match self.client.list(message_id).await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.list(message_id).await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    /// Get unique IDs (UIDs) for messages.
    ///
    /// - `Some(id)` — returns the UID for the single message with that number
    /// - `None` — returns UIDs for all messages
    ///
    /// Returns `Outcome::Reconnected(uids)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn uidl(&mut self, message_id: Option<u32>) -> Result<Outcome<Vec<UidlEntry>>> {
        match self.client.uidl(message_id).await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.uidl(message_id).await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    /// Retrieve the full text of message `id`.
    ///
    /// Returns `Outcome::Reconnected(msg)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn retr(&mut self, id: u32) -> Result<Outcome<Message>> {
        match self.client.retr(id).await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.retr(id).await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    /// Mark message `id` for deletion.
    ///
    /// The deletion is committed only on [`quit()`](Self::quit).
    ///
    /// Returns `Outcome::Reconnected(())` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded — including this one.
    pub async fn dele(&mut self, id: u32) -> Result<Outcome<()>> {
        match self.client.dele(id).await {
            Ok(()) => Ok(Outcome::Fresh(())),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                self.client.dele(id).await?;
                Ok(Outcome::Reconnected(()))
            }
            Err(e) => Err(e),
        }
    }

    /// Reset the session, unmarking all messages that were marked for deletion.
    ///
    /// Returns `Outcome::Reconnected(())` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn rset(&mut self) -> Result<Outcome<()>> {
        match self.client.rset().await {
            Ok(()) => Ok(Outcome::Fresh(())),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                self.client.rset().await?;
                Ok(Outcome::Reconnected(()))
            }
            Err(e) => Err(e),
        }
    }

    /// Send a no-op command to keep the connection alive.
    ///
    /// Returns `Outcome::Reconnected(())` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn noop(&mut self) -> Result<Outcome<()>> {
        match self.client.noop().await {
            Ok(()) => Ok(Outcome::Fresh(())),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                self.client.noop().await?;
                Ok(Outcome::Reconnected(()))
            }
            Err(e) => Err(e),
        }
    }

    /// Retrieve the headers and the first `lines` lines of message `id`.
    ///
    /// Returns `Outcome::Reconnected(msg)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn top(&mut self, id: u32, lines: u32) -> Result<Outcome<Message>> {
        match self.client.top(id, lines).await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.top(id, lines).await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    /// Query the server for its supported capabilities (RFC 2449).
    ///
    /// Returns `Outcome::Reconnected(caps)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn capa(&mut self) -> Result<Outcome<Vec<Capability>>> {
        match self.client.capa().await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.capa().await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    /// Retrieve multiple messages by their message numbers.
    ///
    /// Returns a `Vec` with one `Result<Message>` per input ID. Each result
    /// is independently `Ok` or `Err`.
    ///
    /// Returns `Outcome::Reconnected(results)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn retr_many(&mut self, ids: &[u32]) -> Result<Outcome<Vec<Result<Message>>>> {
        match self.client.retr_many(ids).await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.retr_many(ids).await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    /// Mark multiple messages for deletion.
    ///
    /// Returns a `Vec` with one `Result<()>` per input ID. Each result is
    /// independently `Ok` or `Err`.
    ///
    /// Returns `Outcome::Reconnected(results)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded — including any that were just applied.
    pub async fn dele_many(&mut self, ids: &[u32]) -> Result<Outcome<Vec<Result<()>>>> {
        match self.client.dele_many(ids).await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.dele_many(ids).await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    /// Return UIDL entries for messages not in `seen`.
    ///
    /// Returns `Outcome::Reconnected(entries)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn unseen_uids(
        &mut self,
        seen: &HashSet<String>,
    ) -> Result<Outcome<Vec<UidlEntry>>> {
        match self.client.unseen_uids(seen).await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.unseen_uids(seen).await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    /// Fetch full message content for messages not in `seen`.
    ///
    /// Returns `Outcome::Reconnected(results)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn fetch_unseen(
        &mut self,
        seen: &HashSet<String>,
    ) -> Result<Outcome<Vec<(UidlEntry, Message)>>> {
        match self.client.fetch_unseen(seen).await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.fetch_unseen(seen).await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    /// Remove UIDs from `seen` that no longer exist on the server.
    ///
    /// Returns `Outcome::Reconnected(pruned)` if the connection was dropped and
    /// re-established before this call returned. In that case, all pending
    /// DELE marks have been discarded.
    pub async fn prune_seen(
        &mut self,
        seen: &mut HashSet<String>,
    ) -> Result<Outcome<Vec<String>>> {
        match self.client.prune_seen(seen).await {
            Ok(v) => Ok(Outcome::Fresh(v)),
            Err(e) if is_retryable(&e) => {
                self.do_reconnect().await?;
                let v = self.client.prune_seen(seen).await?;
                Ok(Outcome::Reconnected(v))
            }
            Err(e) => Err(e),
        }
    }

    // -------------------------------------------------------------------------
    // Session management
    // -------------------------------------------------------------------------

    /// End the session, committing any pending deletions.
    ///
    /// This method consumes `self`, providing a compile-time guarantee that no
    /// further commands can be issued after the session ends.
    ///
    /// If the connection is already dead (transient I/O error on QUIT), the
    /// error is silently ignored — a best-effort disconnect. Non-transient
    /// errors are propagated.
    pub async fn quit(self) -> Result<()> {
        match self.client.quit().await {
            Ok(()) => Ok(()),
            Err(e) if is_retryable(&e) => Ok(()), // best-effort QUIT; connection already dead
            Err(e) => Err(e),
        }
    }

    // -------------------------------------------------------------------------
    // Non-async read-only accessors
    // -------------------------------------------------------------------------

    /// Returns the server greeting message received on connection.
    pub fn greeting(&self) -> &str {
        self.client.greeting()
    }

    /// Returns the current session state.
    pub fn state(&self) -> SessionState {
        self.client.state()
    }

    /// Returns `true` if the connection is currently encrypted via TLS.
    pub fn is_encrypted(&self) -> bool {
        self.client.is_encrypted()
    }

    /// Returns `true` if the connection is known to be closed.
    pub fn is_closed(&self) -> bool {
        self.client.is_closed()
    }

    /// Returns `true` if the server advertised the `PIPELINING` capability.
    pub fn supports_pipelining(&self) -> bool {
        self.client.supports_pipelining()
    }

    // -------------------------------------------------------------------------
    // Test-only constructor
    // -------------------------------------------------------------------------

    /// Construct a `ReconnectingClient` from an already-connected `Pop3Client`.
    ///
    /// This is used in tests to inject a mock `Pop3Client` directly without
    /// going through the real connection and authentication flow.
    #[cfg(test)]
    pub(crate) fn new_for_test(
        client: Pop3Client,
        builder: Pop3ClientBuilder,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            client,
            builder,
            username: username.into(),
            password: password.into(),
            max_retries: 3,
            initial_delay: Duration::from_millis(0), // zero delay for tests
            max_delay: Duration::from_millis(0),
            jitter: false, // deterministic for tests
            on_reconnect: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a configured [`ExponentialBuilder`] from the given parameters.
fn build_exp_backoff(
    max_retries: usize,
    initial_delay: Duration,
    max_delay: Duration,
    with_jitter: bool,
) -> ExponentialBuilder {
    let exp = ExponentialBuilder::default()
        .with_max_times(max_retries)
        .with_min_delay(initial_delay)
        .with_max_delay(max_delay);
    if with_jitter {
        exp.with_jitter()
    } else {
        exp
    }
}

/// Connect to the POP3 server and authenticate using USER/PASS.
///
/// This is the unit of work retried by the backon retry loops.
async fn connect_and_auth(
    builder: &Pop3ClientBuilder,
    username: &str,
    password: &str,
) -> Result<Pop3Client> {
    let mut client = builder.clone().connect().await?;
    client.login(username, password).await?;
    Ok(client)
}

/// Returns `true` for transient errors that warrant a reconnect attempt.
///
/// [`Pop3Error::AuthFailed`] is explicitly excluded — retrying wrong credentials
/// risks triggering brute-force lockout on the server.
fn is_retryable(e: &Pop3Error) -> bool {
    matches!(
        e,
        Pop3Error::Io(_) | Pop3Error::ConnectionClosed | Pop3Error::Timeout | Pop3Error::SysTemp(_)
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    // --- is_retryable ---

    #[test]
    fn is_retryable_io_error() {
        let e = Pop3Error::Io(io::Error::new(io::ErrorKind::ConnectionReset, "reset"));
        assert!(is_retryable(&e));
    }

    #[test]
    fn is_retryable_connection_closed() {
        assert!(is_retryable(&Pop3Error::ConnectionClosed));
    }

    #[test]
    fn is_retryable_timeout() {
        assert!(is_retryable(&Pop3Error::Timeout));
    }

    #[test]
    fn is_retryable_sys_temp() {
        assert!(is_retryable(&Pop3Error::SysTemp("transient".into())));
    }

    #[test]
    fn is_retryable_auth_failed() {
        assert!(!is_retryable(&Pop3Error::AuthFailed("bad pass".into())));
    }

    #[test]
    fn is_retryable_server_error() {
        assert!(!is_retryable(&Pop3Error::ServerError("no msg".into())));
    }

    #[test]
    fn is_retryable_parse_error() {
        assert!(!is_retryable(&Pop3Error::Parse("bad resp".into())));
    }

    #[test]
    fn is_retryable_invalid_input() {
        assert!(!is_retryable(&Pop3Error::InvalidInput));
    }

    #[test]
    fn is_retryable_tls() {
        assert!(!is_retryable(&Pop3Error::Tls("cert error".into())));
    }

    #[test]
    fn is_retryable_mailbox_in_use() {
        assert!(!is_retryable(&Pop3Error::MailboxInUse("in use".into())));
    }

    // --- Outcome<T> ---

    #[test]
    fn outcome_fresh_into_inner() {
        assert_eq!(Outcome::Fresh(42u32).into_inner(), 42);
    }

    #[test]
    fn outcome_reconnected_into_inner() {
        assert_eq!(Outcome::Reconnected(42u32).into_inner(), 42);
    }

    #[test]
    fn outcome_fresh_is_reconnected() {
        assert!(!Outcome::<u32>::Fresh(0).is_reconnected());
    }

    #[test]
    fn outcome_reconnected_is_reconnected() {
        assert!(Outcome::<u32>::Reconnected(0).is_reconnected());
    }

    // --- ReconnectingClientBuilder defaults ---

    #[test]
    fn reconnecting_builder_defaults() {
        let builder = ReconnectingClientBuilder::new(Pop3ClientBuilder::new("localhost"));
        assert_eq!(builder.max_retries, 3);
        assert_eq!(builder.initial_delay, Duration::from_secs(1));
        assert_eq!(builder.max_delay, Duration::from_secs(30));
        assert!(builder.jitter);
    }
}
