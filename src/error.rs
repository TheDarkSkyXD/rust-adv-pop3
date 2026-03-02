use std::io;

/// All errors that can occur when interacting with a POP3 server.
///
/// Most methods on [`Pop3Client`](crate::Pop3Client) return `Result<T, Pop3Error>`.
/// Use `pop3::Result<T>` as a convenient alias.
#[derive(Debug, thiserror::Error)]
pub enum Pop3Error {
    /// An underlying I/O error occurred (network read/write failure, EOF, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// A TLS error occurred during handshake or encrypted I/O.
    ///
    /// The inner `String` contains the backend-agnostic error message
    /// (works for both rustls and OpenSSL backends).
    #[error("TLS error: {0}")]
    Tls(String),

    /// The provided hostname is not a valid DNS name for TLS SNI verification.
    ///
    /// Check that the hostname does not contain invalid characters.
    #[error("invalid DNS name: {0}")]
    InvalidDnsName(String),

    /// Mailbox is locked by another POP3 session (RESP-CODE: `[IN-USE]`).
    /// Retry after the other session ends.
    #[error("mailbox in use: {0}")]
    MailboxInUse(String),

    /// Authentication attempted too soon after last login (RESP-CODE: `[LOGIN-DELAY]`).
    #[error("login delay: {0}")]
    LoginDelay(String),

    /// Temporary system error -- likely transient (RESP-CODE: `[SYS/TEMP]`).
    #[error("temporary system error: {0}")]
    SysTemp(String),

    /// Permanent system error -- requires manual intervention (RESP-CODE: `[SYS/PERM]`).
    #[error("permanent system error: {0}")]
    SysPerm(String),

    /// The server returned a `-ERR` response to a command.
    ///
    /// The inner `String` contains the server error message. This variant is
    /// distinct from [`AuthFailed`](Pop3Error::AuthFailed) — server errors
    /// during `USER`/`PASS` are promoted to `AuthFailed`.
    #[error("server error: {0}")]
    ServerError(String),

    /// The server rejected the authentication credentials.
    ///
    /// Returned by [`login()`](crate::Pop3Client::login) when the server
    /// sends `-ERR` in response to the `USER` or `PASS` command.
    #[error("authentication failed: {0}")]
    AuthFailed(String),

    /// A response from the server could not be parsed.
    ///
    /// Indicates a server that does not conform to RFC 1939.
    #[error("parse error: {0}")]
    Parse(String),

    /// A command was issued that requires authentication, but the client is
    /// not yet logged in (or was called after a failed login attempt).
    #[error("not authenticated")]
    NotAuthenticated,

    /// A user-supplied string contains CR (`\r`) or LF (`\n`) characters,
    /// which would enable CRLF injection attacks.
    #[error("invalid input: CRLF injection detected")]
    InvalidInput,

    /// The server did not respond within the configured timeout duration.
    #[error("timed out")]
    Timeout,
}

/// A specialized `Result` type for POP3 operations.
///
/// Equivalent to `std::result::Result<T, Pop3Error>`.
pub type Result<T> = std::result::Result<T, Pop3Error>;
