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

use std::time::Duration;

use backon::{ExponentialBuilder, Retryable};

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
#[allow(dead_code)] // fields are used in Plan 02 command wrappers
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
    /// Used internally by command wrappers (Plan 02) when a transient error
    /// is detected on the active connection.
    #[allow(dead_code)] // used in Plan 02 command wrappers
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

    /// Quit the underlying POP3 session, committing DELE marks and disconnecting.
    ///
    /// This consumes `self` so the client cannot be used after quitting.
    pub async fn quit(self) -> Result<()> {
        self.client.quit().await
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
