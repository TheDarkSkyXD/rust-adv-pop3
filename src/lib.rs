//! # pop3
//!
//! A modern, safe, async POP3 client library for Rust, powered by Tokio.
//!
//! ## Features
//!
//! - **Async/await** — all operations are `async fn` on the Tokio runtime
//! - **Dual TLS backends** — choose `rustls-tls` (default, pure Rust) or `openssl-tls`
//! - **STARTTLS** — upgrade plain connections to TLS via the [`Pop3Client::stls`] method
//! - **Full POP3 coverage** — STAT, LIST, UIDL, RETR, DELE, RSET, NOOP, TOP, CAPA, QUIT
//! - **Type-safe sessions** — [`quit()`](Pop3Client::quit) consumes the client, preventing use-after-disconnect
//! - **Proper error handling** — no panics; all errors returned as [`Result<T, Pop3Error>`]
//!
//! ## Quick Start
//!
//! ```no_run
//! use pop3::Pop3Client;
//!
//! #[tokio::main]
//! async fn main() -> pop3::Result<()> {
//!     // Connect over plain TCP (port 110)
//!     let mut client = Pop3Client::connect(
//!         ("pop.example.com", 110),
//!         std::time::Duration::from_secs(30),
//!     ).await?;
//!
//!     client.login("user", "pass").await?;
//!     let stat = client.stat().await?;
//!     println!("{} messages, {} bytes", stat.message_count, stat.mailbox_size);
//!     client.quit().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## TLS Connections
//!
//! Connect directly over TLS — port 995, requires `rustls-tls` (default) or `openssl-tls`:
//!
//! ```ignore
//! use pop3::Pop3Client;
//!
//! #[tokio::main]
//! async fn main() -> pop3::Result<()> {
//!     let mut client = Pop3Client::connect_tls_default(
//!         ("pop.gmail.com", 995),
//!         "pop.gmail.com",
//!     ).await?;
//!     client.login("user@gmail.com", "app-password").await?;
//!     client.quit().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## STARTTLS (Upgrade Plain to TLS)
//!
//! Connect over plain TCP then upgrade to TLS — requires `rustls-tls` (default) or `openssl-tls`:
//!
//! ```ignore
//! use pop3::Pop3Client;
//!
//! #[tokio::main]
//! async fn main() -> pop3::Result<()> {
//!     let mut client = Pop3Client::connect(
//!         ("pop.example.com", 110),
//!         std::time::Duration::from_secs(30),
//!     ).await?;
//!     client.stls("pop.example.com").await?;
//!     client.login("user", "pass").await?;
//!     client.quit().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Feature Flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `rustls-tls` | Yes | TLS via rustls (pure Rust, no system deps) |
//! | `openssl-tls` | No | TLS via OpenSSL (requires system libssl) |
//!
//! Enable one TLS backend at a time. Both cannot be active simultaneously.
//!
//! To use OpenSSL instead of rustls:
//!
//! ```toml
//! [dependencies]
//! pop3 = { version = "2", default-features = false, features = ["openssl-tls"] }
//! ```

#[cfg(all(feature = "rustls-tls", feature = "openssl-tls"))]
compile_error!(
    "Feature flags `rustls-tls` and `openssl-tls` are mutually exclusive. \
     Enable only one: `cargo build --features rustls-tls` or \
     `cargo build --no-default-features --features openssl-tls`."
);

mod builder;
mod client;
mod error;
#[cfg(feature = "pool")]
pub mod pool;
pub mod reconnect;
pub(crate) mod response;
mod transport;
mod types;

pub use builder::Pop3ClientBuilder;
pub use client::Pop3Client;
pub use error::{Pop3Error, Result};
pub use reconnect::{Outcome, ReconnectingClient, ReconnectingClientBuilder};
pub use types::{Capability, ListEntry, Message, SessionState, Stat, UidlEntry};

#[cfg(feature = "pool")]
pub use pool::{
    AccountKey, PoolConfig, PooledConnection, Pop3ConnectionManager, Pop3Pool, Pop3PoolError,
};
