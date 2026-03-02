use std::time::Duration;

use crate::client::Pop3Client;
use crate::error::Result;
use crate::transport::DEFAULT_TIMEOUT;

/// Internal TLS mode for the builder.
#[derive(Debug, Clone)]
enum TlsMode {
    /// Plain TCP connection (port 110 default).
    Plain,
    /// TLS-on-connect (port 995 default).
    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    Tls,
    /// Connect plain, then upgrade via STLS command.
    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    StartTls,
}

/// Internal auth mode for the builder.
#[derive(Debug, Clone)]
enum AuthMode {
    /// No automatic authentication.
    None,
    /// USER/PASS authentication.
    Login { username: String, password: String },
    /// APOP authentication.
    Apop { username: String, password: String },
}

/// A fluent builder for creating [`Pop3Client`] connections.
///
/// The builder provides a convenient way to configure connection parameters,
/// TLS mode, and optional automatic authentication before connecting.
///
/// # Smart Port Defaults
///
/// - Plain TCP and STARTTLS: port **110**
/// - TLS-on-connect: port **995**
///
/// Use [`.port()`](Self::port) to override the default.
///
/// # Auto-Authentication
///
/// If credentials are provided via [`.credentials()`](Self::credentials) or
/// [`.apop()`](Self::apop), the [`connect()`](Self::connect) method
/// authenticates automatically before returning the client.
///
/// # Example
///
/// ```no_run
/// use pop3::Pop3ClientBuilder;
///
/// #[tokio::main]
/// async fn main() -> pop3::Result<()> {
///     // Plain TCP with auto-login
///     let mut client = Pop3ClientBuilder::new("pop.example.com")
///         .credentials("alice", "secret")
///         .connect()
///         .await?;
///
///     let stat = client.stat().await?;
///     println!("{} messages", stat.message_count);
///     client.quit().await?;
///     Ok(())
/// }
/// ```
///
/// # TLS Example
///
/// ```ignore
/// use pop3::Pop3ClientBuilder;
///
/// #[tokio::main]
/// async fn main() -> pop3::Result<()> {
///     // TLS-on-connect (port 995 by default)
///     let mut client = Pop3ClientBuilder::new("pop.gmail.com")
///         .tls()
///         .credentials("user@gmail.com", "app-password")
///         .connect()
///         .await?;
///     client.quit().await?;
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Pop3ClientBuilder {
    hostname: String,
    port: Option<u16>,
    timeout: Duration,
    tls_mode: TlsMode,
    auth: AuthMode,
}

impl Pop3ClientBuilder {
    /// Create a new builder for the given hostname.
    ///
    /// The hostname is used for TCP connection and TLS SNI verification.
    /// Port defaults are applied automatically based on TLS mode:
    /// - Plain / STARTTLS: **110**
    /// - TLS-on-connect: **995**
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3ClientBuilder;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> pop3::Result<()> {
    /// let mut client = Pop3ClientBuilder::new("pop.example.com")
    ///     .connect()
    ///     .await?;
    /// client.quit().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(hostname: impl Into<String>) -> Self {
        Pop3ClientBuilder {
            hostname: hostname.into(),
            port: None,
            timeout: DEFAULT_TIMEOUT,
            tls_mode: TlsMode::Plain,
            auth: AuthMode::None,
        }
    }

    /// Set the port number, overriding the smart default.
    ///
    /// If not called, port is determined by TLS mode:
    /// - Plain / STARTTLS: **110**
    /// - TLS-on-connect: **995**
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Set the read timeout for all operations.
    ///
    /// Defaults to 30 seconds. Applied to every read on the connection.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = duration;
        self
    }

    /// Enable TLS-on-connect mode (typically port 995).
    ///
    /// The connection is encrypted from the start. The hostname provided to
    /// [`new()`](Self::new) is used for TLS SNI verification.
    ///
    /// If both `.tls()` and `.starttls()` are called, the last one wins.
    ///
    /// Requires the `rustls-tls` (default) or `openssl-tls` feature flag.
    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    pub fn tls(mut self) -> Self {
        self.tls_mode = TlsMode::Tls;
        self
    }

    /// Enable STARTTLS mode: connect over plain TCP, then upgrade to TLS.
    ///
    /// The builder connects on the plain port (default 110), sends the STLS
    /// command, and upgrades the connection to TLS before returning. The
    /// hostname from [`new()`](Self::new) is used for SNI verification.
    ///
    /// If both `.tls()` and `.starttls()` are called, the last one wins.
    ///
    /// Requires the `rustls-tls` (default) or `openssl-tls` feature flag.
    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    pub fn starttls(mut self) -> Self {
        self.tls_mode = TlsMode::StartTls;
        self
    }

    /// Set USER/PASS credentials for automatic authentication.
    ///
    /// When credentials are set, [`connect()`](Self::connect) authenticates
    /// via [`Pop3Client::login()`] before returning. The returned client is
    /// already in [`SessionState::Authenticated`](crate::SessionState::Authenticated).
    ///
    /// Mutually exclusive with [`.apop()`](Self::apop) -- the last one called wins.
    pub fn credentials(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.auth = AuthMode::Login {
            username: username.into(),
            password: password.into(),
        };
        self
    }

    /// Set APOP credentials for automatic authentication.
    ///
    /// When APOP credentials are set, [`connect()`](Self::connect) authenticates
    /// via [`Pop3Client::apop()`] before returning. The server greeting must
    /// contain an APOP timestamp (`<...>`); an error is returned if it does not.
    ///
    /// # Security Warning
    ///
    /// APOP uses MD5, which is cryptographically broken. Prefer
    /// [`.credentials()`](Self::credentials) over a TLS connection for
    /// modern servers.
    ///
    /// Mutually exclusive with [`.credentials()`](Self::credentials) -- the last one called wins.
    pub fn apop(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.auth = AuthMode::Apop {
            username: username.into(),
            password: password.into(),
        };
        self
    }

    /// Resolve the effective port based on TLS mode and explicit override.
    fn effective_port(&self) -> u16 {
        if let Some(port) = self.port {
            return port;
        }
        match self.tls_mode {
            #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
            TlsMode::Tls => 995,
            _ => 110,
        }
    }

    /// Connect to the POP3 server, applying the configured TLS mode and
    /// optional authentication.
    ///
    /// This is the terminal method that consumes the builder and produces a
    /// connected [`Pop3Client`]. If credentials were set via
    /// [`.credentials()`](Self::credentials) or [`.apop()`](Self::apop),
    /// the client is authenticated before being returned.
    ///
    /// # Errors
    ///
    /// - [`Pop3Error::Io`](crate::Pop3Error::Io) -- TCP connection failed
    /// - [`Pop3Error::Tls`](crate::Pop3Error::Tls) -- TLS handshake failed
    /// - [`Pop3Error::AuthFailed`](crate::Pop3Error::AuthFailed) -- authentication rejected
    /// - [`Pop3Error::ServerError`](crate::Pop3Error::ServerError) -- APOP requested but no timestamp in greeting
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3ClientBuilder;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3ClientBuilder::new("pop.example.com")
    ///         .port(110)
    ///         .credentials("user", "pass")
    ///         .connect()
    ///         .await?;
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn connect(self) -> Result<Pop3Client> {
        let port = self.effective_port();
        let addr = (self.hostname.as_str(), port);

        let mut client = match self.tls_mode {
            TlsMode::Plain => Pop3Client::connect(addr, self.timeout).await?,
            #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
            TlsMode::Tls => Pop3Client::connect_tls(addr, &self.hostname, self.timeout).await?,
            #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
            TlsMode::StartTls => {
                let mut client = Pop3Client::connect(addr, self.timeout).await?;
                client.stls(&self.hostname).await?;
                client
            }
        };

        // Auto-authenticate if credentials were provided
        match self.auth {
            AuthMode::None => {}
            AuthMode::Login { username, password } => {
                client.login(&username, &password).await?;
            }
            AuthMode::Apop { username, password } => {
                #[allow(deprecated)]
                client.apop(&username, &password).await?;
            }
        }

        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_port_plain() {
        let builder = Pop3ClientBuilder::new("host.example.com");
        assert_eq!(builder.effective_port(), 110);
    }

    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    #[test]
    fn default_port_tls() {
        let builder = Pop3ClientBuilder::new("host.example.com").tls();
        assert_eq!(builder.effective_port(), 995);
    }

    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    #[test]
    fn default_port_starttls() {
        let builder = Pop3ClientBuilder::new("host.example.com").starttls();
        assert_eq!(builder.effective_port(), 110);
    }

    #[test]
    fn explicit_port_overrides_default() {
        let builder = Pop3ClientBuilder::new("host.example.com").port(2525);
        assert_eq!(builder.effective_port(), 2525);
    }

    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    #[test]
    fn explicit_port_overrides_tls_default() {
        let builder = Pop3ClientBuilder::new("host.example.com").tls().port(9950);
        assert_eq!(builder.effective_port(), 9950);
    }

    #[test]
    fn timeout_is_configurable() {
        let builder = Pop3ClientBuilder::new("host.example.com").timeout(Duration::from_secs(60));
        assert_eq!(builder.timeout, Duration::from_secs(60));
    }

    #[test]
    fn default_timeout_is_30_seconds() {
        let builder = Pop3ClientBuilder::new("host.example.com");
        assert_eq!(builder.timeout, Duration::from_secs(30));
    }

    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    #[test]
    fn tls_mode_last_wins() {
        // .tls() then .starttls() -> StartTls wins
        let builder = Pop3ClientBuilder::new("host.example.com").tls().starttls();
        assert!(matches!(builder.tls_mode, TlsMode::StartTls));

        // .starttls() then .tls() -> Tls wins
        let builder = Pop3ClientBuilder::new("host.example.com").starttls().tls();
        assert!(matches!(builder.tls_mode, TlsMode::Tls));
    }

    #[test]
    fn credentials_sets_login_auth() {
        let builder = Pop3ClientBuilder::new("host.example.com").credentials("user", "pass");
        assert!(matches!(builder.auth, AuthMode::Login { .. }));
    }

    #[test]
    fn apop_sets_apop_auth() {
        let builder = Pop3ClientBuilder::new("host.example.com").apop("user", "pass");
        assert!(matches!(builder.auth, AuthMode::Apop { .. }));
    }

    #[test]
    fn auth_last_wins() {
        // credentials then apop -> Apop wins
        let builder = Pop3ClientBuilder::new("host.example.com")
            .credentials("user", "pass")
            .apop("user", "pass");
        assert!(matches!(builder.auth, AuthMode::Apop { .. }));

        // apop then credentials -> Login wins
        let builder = Pop3ClientBuilder::new("host.example.com")
            .apop("user", "pass")
            .credentials("user", "pass");
        assert!(matches!(builder.auth, AuthMode::Login { .. }));
    }

    #[test]
    fn builder_derives_debug_and_clone() {
        let builder = Pop3ClientBuilder::new("host.example.com")
            .port(110)
            .timeout(Duration::from_secs(60));
        let _ = format!("{:?}", builder);
        let clone = builder.clone();
        assert_eq!(clone.hostname, "host.example.com");
        assert_eq!(clone.effective_port(), 110);
    }

    #[test]
    fn consuming_chain_compiles() {
        // Verify the consuming chain style works in one expression.
        // This is a compile-time check -- we cannot actually connect in tests.
        let _builder = Pop3ClientBuilder::new("host.example.com")
            .port(110)
            .timeout(Duration::from_secs(60))
            .credentials("user", "pass");
        // If this compiles, the consuming chain style works.
    }

    #[test]
    fn hostname_stored_correctly() {
        let builder = Pop3ClientBuilder::new("pop.gmail.com");
        assert_eq!(builder.hostname, "pop.gmail.com");
    }

    #[test]
    fn hostname_from_string() {
        let host = String::from("pop.example.com");
        let builder = Pop3ClientBuilder::new(host);
        assert_eq!(builder.hostname, "pop.example.com");
    }
}
