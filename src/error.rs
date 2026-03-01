use std::io;

/// Errors that can occur when interacting with a POP3 server.
#[derive(Debug, thiserror::Error)]
pub enum Pop3Error {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// A TLS error occurred.
    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),

    /// The hostname is not a valid DNS name for TLS.
    #[error("invalid DNS name: {0}")]
    InvalidDnsName(String),

    /// The server returned an `-ERR` response.
    #[error("server error: {0}")]
    ServerError(String),

    /// The server rejected authentication credentials.
    #[error("authentication failed: {0}")]
    AuthFailed(String),

    /// A response from the server could not be parsed.
    #[error("parse error: {0}")]
    Parse(String),

    /// The command requires authentication but the client is not logged in.
    #[error("not authenticated")]
    NotAuthenticated,

    /// The input contains invalid characters (e.g. CRLF injection).
    #[error("invalid input: CRLF injection detected")]
    InvalidInput,
}

/// A specialized `Result` type for POP3 operations.
pub type Result<T> = std::result::Result<T, Pop3Error>;
