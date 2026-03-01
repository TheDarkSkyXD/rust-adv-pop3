//! A modern, safe POP3 client library for Rust.
//!
//! This crate provides a [`Pop3Client`] for communicating with POP3 mail servers
//! over plain TCP or TLS (via [`rustls`]).
//!
//! # Example
//!
//! ```no_run
//! use pop3::{Pop3Client, TlsMode};
//!
//! fn main() -> pop3::Result<()> {
//!     let mut client = Pop3Client::connect(
//!         ("pop.example.com", 995),
//!         TlsMode::Tls("pop.example.com".into()),
//!     )?;
//!
//!     client.login("user", "pass")?;
//!     let stat = client.stat()?;
//!     println!("{} messages, {} bytes", stat.message_count, stat.mailbox_size);
//!     client.quit()?;
//!     Ok(())
//! }
//! ```

mod client;
mod error;
pub(crate) mod response;
mod transport;
mod types;

pub use client::{Pop3Client, TlsMode};
pub use error::{Pop3Error, Result};
pub use types::{Capability, ListEntry, Message, Stat, UidlEntry};
