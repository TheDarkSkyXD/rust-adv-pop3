//! A modern, safe async POP3 client library for Rust.
//!
//! This crate provides a [`Pop3Client`] for communicating with POP3 mail servers
//! over plain TCP (async, powered by Tokio). TLS support is added in Phase 3.
//!
//! # Example
//!
//! ```no_run
//! use pop3::Pop3Client;
//!
//! #[tokio::main]
//! async fn main() -> pop3::Result<()> {
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

mod client;
mod error;
pub(crate) mod response;
mod transport;
mod types;

pub use client::Pop3Client;
pub use error::{Pop3Error, Result};
pub use types::{Capability, ListEntry, Message, SessionState, Stat, UidlEntry};
