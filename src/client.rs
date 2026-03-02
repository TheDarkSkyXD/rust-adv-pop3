use std::collections::HashSet;
use std::time::Duration;

use crate::error::{Pop3Error, Result};
use crate::response;
use crate::transport::Transport;
use crate::types::{Capability, ListEntry, Message, SessionState, Stat, UidlEntry};

/// An async POP3 client connection.
///
/// Create a `Pop3Client` using one of the `connect*` constructors. After
/// creation, authenticate with [`login`](Self::login) before issuing
/// mailbox commands.
///
/// # Session Lifecycle
///
/// ```text
/// connect() / connect_tls()
///     -> SessionState::Connected
///         -> login() -> SessionState::Authenticated
///             -> stat() / list() / retr() / dele() / ...
///             -> quit() [consumes self] -> connection closed
/// ```
///
/// Dropping a `Pop3Client` without calling [`quit`](Self::quit) closes the
/// TCP connection silently. Any pending `DELE` marks are **not** committed.
pub struct Pop3Client {
    transport: Transport,
    greeting: String,
    state: SessionState,
    is_pipelining: bool,
}

/// Check that a string does not contain CR or LF (CRLF injection protection).
fn check_no_crlf(s: &str) -> Result<()> {
    if s.contains('\r') || s.contains('\n') {
        Err(Pop3Error::InvalidInput)
    } else {
        Ok(())
    }
}

/// Validate that a message ID is >= 1 (POP3 message numbering starts at 1).
fn validate_message_id(id: u32) -> Result<()> {
    if id == 0 {
        Err(Pop3Error::InvalidInput)
    } else {
        Ok(())
    }
}

/// Extract the APOP timestamp from a server greeting.
///
/// The timestamp can appear anywhere in the greeting as `<...>`.
/// Returns `None` if no angle-bracket pair is found.
fn extract_apop_timestamp(greeting: &str) -> Option<&str> {
    let start = greeting.find('<')?;
    let end = greeting[start..].find('>')? + start;
    Some(&greeting[start..=end])
}

/// Compute the APOP MD5 digest: hex(md5(timestamp + password)).
fn compute_apop_digest(timestamp: &str, password: &str) -> String {
    let input = format!("{timestamp}{password}");
    let digest = md5::compute(input.as_bytes());
    format!("{:x}", digest)
}

/// Maximum number of POP3 commands sent before draining responses in pipelined mode.
/// Prevents TCP send-buffer deadlock when the server sends large RETR responses.
/// See RFC 2449 section 6.6 and the Phase 5 research document.
const PIPELINE_WINDOW: usize = 4;

impl Pop3Client {
    /// Connect to a POP3 server over plain TCP.
    ///
    /// The `timeout` duration is applied to every read operation for the lifetime
    /// of this connection. Pass `Duration::from_secs(30)` for a sensible default.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect(
    ///         ("pop.example.com", 110),
    ///         std::time::Duration::from_secs(30),
    ///     ).await?;
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn connect(addr: impl tokio::net::ToSocketAddrs, timeout: Duration) -> Result<Self> {
        let mut transport = Transport::connect_plain(addr, timeout).await?;
        let greeting_line = transport.read_line().await?;
        let greeting_text = response::parse_status_line(&greeting_line)?;
        Ok(Pop3Client {
            transport,
            greeting: greeting_text.to_string(),
            state: SessionState::Connected,
            is_pipelining: false,
        })
    }

    /// Connect to a POP3 server over plain TCP with the default timeout (30 seconds).
    ///
    /// This is a convenience wrapper around [`connect`](Self::connect) that applies
    /// a 30-second read timeout.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn connect_default(addr: impl tokio::net::ToSocketAddrs) -> Result<Self> {
        Self::connect(addr, crate::transport::DEFAULT_TIMEOUT).await
    }

    /// Connect to a POP3 server over TLS (typically port 995).
    ///
    /// The `hostname` is used for TLS server name verification (SNI) and must
    /// match the server certificate. The `timeout` duration is applied to every
    /// read operation.
    ///
    /// Requires the `rustls-tls` (default) or `openssl-tls` feature flag.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_tls(
    ///         ("pop.gmail.com", 995),
    ///         "pop.gmail.com",
    ///         std::time::Duration::from_secs(30),
    ///     ).await?;
    ///     client.login("user@gmail.com", "app-password").await?;
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Pop3Error::InvalidDnsName`] if `hostname` is not a valid DNS name,
    /// or [`Pop3Error::Tls`] if the TLS handshake fails.
    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    pub async fn connect_tls(
        addr: impl tokio::net::ToSocketAddrs,
        hostname: &str,
        timeout: Duration,
    ) -> Result<Self> {
        let mut transport = Transport::connect_tls(addr, hostname, timeout).await?;
        let greeting_line = transport.read_line().await?;
        let greeting_text = response::parse_status_line(&greeting_line)?;
        Ok(Pop3Client {
            transport,
            greeting: greeting_text.to_string(),
            state: SessionState::Connected,
            is_pipelining: false,
        })
    }

    /// Connect to a POP3 server over TLS with the default timeout (30 seconds).
    ///
    /// Convenience wrapper around [`connect_tls`](Self::connect_tls) with a 30-second timeout.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_tls_default(
    ///         ("pop.gmail.com", 995),
    ///         "pop.gmail.com",
    ///     ).await?;
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    pub async fn connect_tls_default(
        addr: impl tokio::net::ToSocketAddrs,
        hostname: &str,
    ) -> Result<Self> {
        Self::connect_tls(addr, hostname, crate::transport::DEFAULT_TIMEOUT).await
    }

    /// Returns the server greeting message received on connection.
    ///
    /// This is the text following `+OK` on the initial line sent by the server.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     println!("Server greeting: {}", client.greeting());
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub fn greeting(&self) -> &str {
        &self.greeting
    }

    /// Returns the current session state.
    ///
    /// Use this to check whether the client is connected, authenticated, or
    /// disconnected without attempting a command.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::{Pop3Client, SessionState};
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     assert_eq!(client.state(), SessionState::Connected);
    ///     client.login("user", "pass").await?;
    ///     assert_eq!(client.state(), SessionState::Authenticated);
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub fn state(&self) -> SessionState {
        self.state.clone()
    }

    /// Returns `true` if the connection is currently encrypted via TLS.
    ///
    /// Returns `false` for plain TCP connections and after a failed STARTTLS attempt.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Requires rustls-tls (default) or openssl-tls feature.
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let client = Pop3Client::connect_tls_default(
    ///         ("pop.gmail.com", 995),
    ///         "pop.gmail.com",
    ///     ).await?;
    ///     assert!(client.is_encrypted());
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub fn is_encrypted(&self) -> bool {
        self.transport.is_encrypted()
    }

    /// Returns `true` if the connection is known to be closed.
    ///
    /// This flag is set when the server closes the connection (EOF) or
    /// after [`quit()`](Self::quit) completes. It is not a live probe --
    /// a connection that has been silently dropped by the server without
    /// sending EOF will still return `false`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     assert!(!client.is_closed());
    ///     client.quit().await?;
    ///     // client is consumed by quit() -- cannot check is_closed() after
    ///     Ok(())
    /// }
    /// ```
    pub fn is_closed(&self) -> bool {
        self.transport.is_closed()
    }

    /// Returns `true` if the server advertised the `PIPELINING` capability.
    ///
    /// Pipelining is detected automatically by probing the server's CAPA
    /// response after successful authentication. When pipelining is
    /// supported, batch methods like [`retr_many()`](Self::retr_many) and
    /// [`dele_many()`](Self::dele_many) send multiple commands before
    /// reading responses, significantly improving throughput.
    ///
    /// When pipelining is not supported, the batch methods transparently
    /// fall back to sequential execution (one command at a time).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///     if client.supports_pipelining() {
    ///         println!("Server supports pipelining!");
    ///     }
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub fn supports_pipelining(&self) -> bool {
        self.is_pipelining
    }

    /// Upgrade the connection to TLS via the STLS command (POP3 STARTTLS).
    ///
    /// Must be called before authentication — STLS is only valid in the
    /// AUTHORIZATION state per RFC 2595. After a successful upgrade,
    /// [`is_encrypted()`](Self::is_encrypted) returns `true`.
    ///
    /// The `hostname` parameter is used for TLS server name verification (SNI).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect(
    ///         ("pop.example.com", 110),
    ///         std::time::Duration::from_secs(30),
    ///     ).await?;
    ///     client.stls("pop.example.com").await?;
    ///     client.login("user", "pass").await?;
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    pub async fn stls(&mut self, hostname: &str) -> Result<()> {
        if self.state == SessionState::Authenticated {
            return Err(Pop3Error::ServerError(
                "STLS not allowed after authentication (RFC 2595)".to_string(),
            ));
        }
        if self.is_encrypted() {
            return Err(Pop3Error::ServerError(
                "connection is already encrypted".to_string(),
            ));
        }

        self.send_and_check("STLS").await?;
        self.transport.upgrade_in_place(hostname).await?;

        Ok(())
    }

    /// Authenticate with the server using the USER/PASS command sequence.
    ///
    /// On success, the session transitions to [`SessionState::Authenticated`].
    /// Server rejection of credentials returns [`Pop3Error::AuthFailed`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("alice", "secret").await?;
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// - [`Pop3Error::AuthFailed`] — server rejected the credentials
    /// - [`Pop3Error::NotAuthenticated`] — called when already authenticated
    /// - [`Pop3Error::InvalidInput`] — username or password contains CR or LF
    pub async fn login(&mut self, username: &str, password: &str) -> Result<()> {
        if self.state != SessionState::Connected {
            return Err(Pop3Error::NotAuthenticated);
        }
        check_no_crlf(username)?;
        check_no_crlf(password)?;

        // USER command — auth failure if rejected
        self.send_and_check(&format!("USER {username}"))
            .await
            .map_err(|e| match e {
                Pop3Error::ServerError(msg) => Pop3Error::AuthFailed(msg),
                other => other,
            })?;

        // PASS command — auth failure if rejected
        self.send_and_check(&format!("PASS {password}"))
            .await
            .map_err(|e| match e {
                Pop3Error::ServerError(msg) => Pop3Error::AuthFailed(msg),
                other => other,
            })?;

        // Only set authenticated after both commands succeed
        self.state = SessionState::Authenticated;

        // Probe CAPA for PIPELINING support (PIPE-02).
        // Don't propagate CAPA errors -- not all servers support CAPA (RFC 1939).
        let caps = self.capa().await.unwrap_or_default();
        self.is_pipelining = caps.iter().any(|c| c.name == "PIPELINING");

        Ok(())
    }

    /// Authenticate with the server using the APOP command (RFC 1939 section 7).
    ///
    /// APOP uses an MD5 digest of the server greeting timestamp concatenated with
    /// the password. The server must include a timestamp in its greeting
    /// (e.g., `+OK POP3 server ready <1896.697170952@dbc.mtview.ca.us>`).
    ///
    /// # Security Warning
    ///
    /// **APOP uses MD5, which is cryptographically broken.** MD5 collision attacks
    /// are practical and trivial. APOP provides no protection against offline
    /// dictionary attacks if the exchanged messages are intercepted. Use only with
    /// legacy servers where USER/PASS over TLS is unavailable. For modern servers,
    /// prefer [`login()`](Self::login) over a TLS connection.
    ///
    /// # Errors
    ///
    /// - [`Pop3Error::ServerError`] -- server greeting has no APOP timestamp
    /// - [`Pop3Error::AuthFailed`] -- server rejected the APOP credentials
    /// - [`Pop3Error::NotAuthenticated`] -- called when not in Connected state
    /// - [`Pop3Error::InvalidInput`] -- username or password contains CR or LF
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     // Server greeting must contain an APOP timestamp: <timestamp@host>
    ///     #[allow(deprecated)]
    ///     client.apop("user", "password").await?;
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    #[deprecated(
        note = "APOP uses MD5 which is cryptographically broken. Prefer login() over a TLS connection."
    )]
    pub async fn apop(&mut self, username: &str, password: &str) -> Result<()> {
        if self.state != SessionState::Connected {
            return Err(Pop3Error::NotAuthenticated);
        }
        check_no_crlf(username)?;
        check_no_crlf(password)?;

        let timestamp = extract_apop_timestamp(&self.greeting)
            .ok_or_else(|| {
                Pop3Error::ServerError(
                    "server does not support APOP (no timestamp in greeting)".to_string(),
                )
            })?
            .to_string();

        let digest = compute_apop_digest(&timestamp, password);
        let cmd = format!("APOP {username} {digest}");
        self.send_and_check(&cmd).await.map_err(|e| match e {
            Pop3Error::ServerError(msg) => Pop3Error::AuthFailed(msg),
            other => other,
        })?;

        self.state = SessionState::Authenticated;

        // Probe CAPA for PIPELINING support (PIPE-02).
        let caps = self.capa().await.unwrap_or_default();
        self.is_pipelining = caps.iter().any(|c| c.name == "PIPELINING");

        Ok(())
    }

    /// Get mailbox statistics: total message count and total size in bytes.
    ///
    /// Sends the `STAT` command and returns a [`Stat`] with the results.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///     let stat = client.stat().await?;
    ///     println!("{} messages, {} bytes total", stat.message_count, stat.mailbox_size);
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn stat(&mut self) -> Result<Stat> {
        self.require_auth()?;
        let text = self.send_and_check("STAT").await?;
        response::parse_stat(&text)
    }

    /// List messages with their sizes.
    ///
    /// - `Some(id)` — returns size info for the single message with that number
    /// - `None` — returns a list of all messages with their sizes
    ///
    /// Message numbers start at 1 per RFC 1939.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///
    ///     // List all messages
    ///     let all = client.list(None).await?;
    ///     for entry in &all {
    ///         println!("Message {}: {} bytes", entry.message_id, entry.size);
    ///     }
    ///
    ///     // List a single message
    ///     let single = client.list(Some(1)).await?;
    ///     println!("Message 1 is {} bytes", single[0].size);
    ///
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn list(&mut self, message_id: Option<u32>) -> Result<Vec<ListEntry>> {
        self.require_auth()?;
        match message_id {
            Some(id) => {
                validate_message_id(id)?;
                let text = self.send_and_check(&format!("LIST {id}")).await?;
                let entry = response::parse_list_single(&text)?;
                Ok(vec![entry])
            }
            None => {
                self.send_and_check("LIST").await?;
                let body = self.transport.read_multiline().await?;
                response::parse_list_multi(&body)
            }
        }
    }

    /// Get unique IDs (UIDs) for messages.
    ///
    /// UIDs are stable across sessions — use them to detect which messages were
    /// already downloaded, even after the server renumbers messages.
    ///
    /// - `Some(id)` — returns the UID for the single message with that number
    /// - `None` — returns UIDs for all messages
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///
    ///     let uids = client.uidl(None).await?;
    ///     for entry in &uids {
    ///         println!("Message {}: UID {}", entry.message_id, entry.unique_id);
    ///     }
    ///
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn uidl(&mut self, message_id: Option<u32>) -> Result<Vec<UidlEntry>> {
        self.require_auth()?;
        match message_id {
            Some(id) => {
                validate_message_id(id)?;
                let text = self.send_and_check(&format!("UIDL {id}")).await?;
                let entry = response::parse_uidl_single(&text)?;
                Ok(vec![entry])
            }
            None => {
                self.send_and_check("UIDL").await?;
                let body = self.transport.read_multiline().await?;
                response::parse_uidl_multi(&body)
            }
        }
    }

    /// Retrieve a full message by its message number.
    ///
    /// Returns the complete message content (headers + body) as a [`Message`].
    /// Dot-unstuffing is applied per RFC 1939 — double-leading dots are reduced
    /// to single dots.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///     let msg = client.retr(1).await?;
    ///     println!("{}", msg.data);
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn retr(&mut self, message_id: u32) -> Result<Message> {
        self.require_auth()?;
        validate_message_id(message_id)?;
        self.send_and_check(&format!("RETR {message_id}")).await?;
        let data = self.transport.read_multiline().await?;
        Ok(Message { data })
    }

    /// Retrieve multiple messages by their message numbers.
    ///
    /// Returns a `Vec` with one `Result<Message>` per input ID, in the same
    /// order as the input slice. Each result is independently `Ok` or `Err`:
    /// a server `-ERR` for one message does not affect other messages.
    ///
    /// When the server supports pipelining (detected via CAPA after login),
    /// commands are sent in batches for higher throughput. Otherwise, messages
    /// are retrieved one at a time.
    ///
    /// # I/O Error Handling
    ///
    /// If an I/O error occurs mid-pipeline, all successfully-received messages
    /// so far are preserved. The remaining entries in the result vector contain
    /// the I/O error (cloned as `Pop3Error::ConnectionClosed` or similar).
    ///
    /// # Errors
    ///
    /// Returns `Err(Pop3Error::InvalidInput)` immediately if any ID in the
    /// slice is 0. Returns `Err(Pop3Error::NotAuthenticated)` if the client
    /// has not logged in.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///
    ///     let results = client.retr_many(&[1, 2, 3]).await?;
    ///     for (i, result) in results.into_iter().enumerate() {
    ///         match result {
    ///             Ok(msg) => println!("Message {}: {} bytes", i + 1, msg.data.len()),
    ///             Err(e) => eprintln!("Message {} failed: {}", i + 1, e),
    ///         }
    ///     }
    ///
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn retr_many(&mut self, ids: &[u32]) -> Result<Vec<Result<Message>>> {
        self.require_auth()?;

        // Validate all IDs upfront
        for &id in ids {
            validate_message_id(id)?;
        }

        if ids.is_empty() {
            return Ok(Vec::new());
        }

        if !self.is_pipelining {
            // Sequential fallback (PIPE-03)
            let mut results = Vec::with_capacity(ids.len());
            for &id in ids {
                results.push(self.retr(id).await);
            }
            return Ok(results);
        }

        // Pipelined path (PIPE-01, PIPE-04)
        self.retr_many_pipelined(ids).await
    }

    /// Pipelined RETR: send commands in windows of PIPELINE_WINDOW, drain responses.
    async fn retr_many_pipelined(&mut self, ids: &[u32]) -> Result<Vec<Result<Message>>> {
        use tokio::io::AsyncWriteExt;

        let mut results: Vec<Result<Message>> = Vec::with_capacity(ids.len());

        for chunk in ids.chunks(PIPELINE_WINDOW) {
            // Send phase: write all commands, single flush
            for &id in chunk {
                let cmd = format!("RETR {id}\r\n");
                if let Err(e) = self.transport.writer.write_all(cmd.as_bytes()).await {
                    // I/O error during send -- fill remaining with error
                    let remaining = ids.len() - results.len();
                    for _ in 0..remaining {
                        results.push(Err(Pop3Error::Io(std::io::Error::new(
                            e.kind(),
                            e.to_string(),
                        ))));
                    }
                    return Ok(results);
                }
            }
            if let Err(e) = self.transport.writer.flush().await {
                let remaining = ids.len() - results.len();
                for _ in 0..remaining {
                    results.push(Err(Pop3Error::Io(std::io::Error::new(
                        e.kind(),
                        e.to_string(),
                    ))));
                }
                return Ok(results);
            }

            // Receive phase: drain exactly chunk.len() responses
            for _ in chunk {
                match self.read_retr_response().await {
                    Ok(msg) => results.push(Ok(msg)),
                    Err(Pop3Error::ConnectionClosed)
                    | Err(Pop3Error::Timeout)
                    | Err(Pop3Error::Io(_)) => {
                        // I/O-level error: connection may be dead.
                        // Preserve what we have, fill rest with error.
                        let remaining = ids.len() - results.len();
                        for _ in 0..remaining {
                            results.push(Err(Pop3Error::ConnectionClosed));
                        }
                        return Ok(results);
                    }
                    Err(e) => {
                        // Server-level error (-ERR for this message): record it, continue
                        results.push(Err(e));
                    }
                }
            }
        }

        Ok(results)
    }

    /// Read a single RETR response (status line + multiline body).
    async fn read_retr_response(&mut self) -> Result<Message> {
        let line = self.transport.read_line().await?;
        response::parse_status_line(&line)?;
        let data = self.transport.read_multiline().await?;
        Ok(Message { data })
    }

    /// Mark a message for deletion.
    ///
    /// The message is not immediately deleted — it is removed when the session
    /// ends via [`quit`](Self::quit). Use [`rset`](Self::rset) to unmark
    /// all pending deletions before `quit`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///     client.dele(1).await?; // mark message 1 for deletion
    ///     client.quit().await?; // deletion is committed here
    ///     Ok(())
    /// }
    /// ```
    pub async fn dele(&mut self, message_id: u32) -> Result<()> {
        self.require_auth()?;
        validate_message_id(message_id)?;
        self.send_and_check(&format!("DELE {message_id}")).await?;
        Ok(())
    }

    /// Mark multiple messages for deletion.
    ///
    /// Returns a `Vec` with one `Result<()>` per input ID, in the same
    /// order as the input slice. Each result is independently `Ok` or `Err`:
    /// a server `-ERR` for one message does not affect other messages.
    ///
    /// Deletions are committed when [`quit()`](Self::quit) is called.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///
    ///     let results = client.dele_many(&[1, 2, 3]).await?;
    ///     for (i, result) in results.into_iter().enumerate() {
    ///         match result {
    ///             Ok(()) => println!("Message {} marked for deletion", i + 1),
    ///             Err(e) => eprintln!("Message {} delete failed: {}", i + 1, e),
    ///         }
    ///     }
    ///
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn dele_many(&mut self, ids: &[u32]) -> Result<Vec<Result<()>>> {
        self.require_auth()?;

        for &id in ids {
            validate_message_id(id)?;
        }

        if ids.is_empty() {
            return Ok(Vec::new());
        }

        if !self.is_pipelining {
            // Sequential fallback (PIPE-03)
            let mut results = Vec::with_capacity(ids.len());
            for &id in ids {
                results.push(self.dele(id).await);
            }
            return Ok(results);
        }

        // Pipelined path
        self.dele_many_pipelined(ids).await
    }

    /// Pipelined DELE: send commands in windows, drain responses.
    async fn dele_many_pipelined(&mut self, ids: &[u32]) -> Result<Vec<Result<()>>> {
        use tokio::io::AsyncWriteExt;

        let mut results: Vec<Result<()>> = Vec::with_capacity(ids.len());

        for chunk in ids.chunks(PIPELINE_WINDOW) {
            // Send phase
            for &id in chunk {
                let cmd = format!("DELE {id}\r\n");
                if let Err(e) = self.transport.writer.write_all(cmd.as_bytes()).await {
                    let remaining = ids.len() - results.len();
                    for _ in 0..remaining {
                        results.push(Err(Pop3Error::Io(std::io::Error::new(
                            e.kind(),
                            e.to_string(),
                        ))));
                    }
                    return Ok(results);
                }
            }
            if let Err(e) = self.transport.writer.flush().await {
                let remaining = ids.len() - results.len();
                for _ in 0..remaining {
                    results.push(Err(Pop3Error::Io(std::io::Error::new(
                        e.kind(),
                        e.to_string(),
                    ))));
                }
                return Ok(results);
            }

            // Receive phase: single-line responses for DELE
            for _ in chunk {
                match self.transport.read_line().await {
                    Ok(line) => match response::parse_status_line(&line) {
                        Ok(_) => results.push(Ok(())),
                        Err(e) => results.push(Err(e)),
                    },
                    Err(Pop3Error::ConnectionClosed)
                    | Err(Pop3Error::Timeout)
                    | Err(Pop3Error::Io(_)) => {
                        let remaining = ids.len() - results.len();
                        for _ in 0..remaining {
                            results.push(Err(Pop3Error::ConnectionClosed));
                        }
                        return Ok(results);
                    }
                    Err(e) => {
                        results.push(Err(e));
                    }
                }
            }
        }

        Ok(results)
    }

    /// Reset the session, unmarking all messages that were marked for deletion.
    ///
    /// After `rset()`, no messages will be deleted when the session ends.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///     client.dele(1).await?;
    ///     client.rset().await?; // cancel the deletion
    ///     client.quit().await?; // message 1 is NOT deleted
    ///     Ok(())
    /// }
    /// ```
    pub async fn rset(&mut self) -> Result<()> {
        self.require_auth()?;
        self.send_and_check("RSET").await?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Incremental Sync
    // -------------------------------------------------------------------------

    /// # Incremental Sync
    ///
    /// These methods provide a high-level API for incremental mailbox sync.
    /// They operate on a caller-managed set of previously-seen unique IDs (UIDs).
    /// The library never persists the seen set — that responsibility belongs to
    /// the caller.
    ///
    /// **UIDL requirement:** All three methods use the `UIDL` command internally.
    /// UIDL is optional in RFC 1939. If the server does not support it, these
    /// methods return a `Pop3Error::ServerError`. Check `capa()` for the `UIDL`
    /// capability if you need to verify support before calling.
    /// Return UIDL entries for messages not in `seen`.
    ///
    /// Calls `UIDL` once and filters the result against the caller-supplied seen
    /// set. Returns full [`UidlEntry`] values so callers have both the session
    /// message number (`message_id`) and the stable unique ID (`unique_id`).
    ///
    /// # Errors
    ///
    /// - [`Pop3Error::NotAuthenticated`] — client has not logged in
    /// - [`Pop3Error::ServerError`] — server does not support `UIDL`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    /// use std::collections::HashSet;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///
    ///     let seen: HashSet<String> = HashSet::new(); // load from persistence in practice
    ///     let new_entries = client.unseen_uids(&seen).await?;
    ///     for entry in &new_entries {
    ///         println!("New message {}: UID {}", entry.message_id, entry.unique_id);
    ///     }
    ///
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn unseen_uids(&mut self, seen: &HashSet<String>) -> Result<Vec<UidlEntry>> {
        let all = self.uidl(None).await?;
        Ok(all
            .into_iter()
            .filter(|entry| !seen.contains(&entry.unique_id))
            .collect())
    }

    /// Fetch full message content for messages not in `seen`.
    ///
    /// Calls [`unseen_uids`](Self::unseen_uids) to determine which messages are
    /// new, then retrieves each one sequentially. Returns tuples of
    /// `(UidlEntry, Message)` so callers can immediately add `entry.unique_id`
    /// to their seen set after processing.
    ///
    /// This method does **not** mutate `seen` — the caller updates the set after
    /// deciding which messages to mark as processed.
    ///
    /// Fails fast on the first retrieval error. No partial results are returned.
    ///
    /// # Errors
    ///
    /// - [`Pop3Error::NotAuthenticated`] — client has not logged in
    /// - [`Pop3Error::ServerError`] — server does not support `UIDL`
    /// - Any error from `RETR` for a specific message (propagated immediately)
    ///
    /// # Performance
    ///
    /// This method retrieves messages sequentially (one `RETR` per message). For
    /// higher throughput on servers that advertise `PIPELINING`, use
    /// [`unseen_uids`](Self::unseen_uids) to get the entry list, then pass the
    /// message IDs to [`retr_many`](Self::retr_many) manually.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    /// use std::collections::HashSet;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///
    ///     // In practice, load `seen` from disk (e.g. serde_json::from_str).
    ///     let mut seen: HashSet<String> = HashSet::new();
    ///
    ///     let new_messages = client.fetch_unseen(&seen).await?;
    ///     for (entry, msg) in &new_messages {
    ///         println!("Message {}: {} bytes", entry.unique_id, msg.data.len());
    ///         seen.insert(entry.unique_id.clone()); // update seen set
    ///     }
    ///
    ///     // Persist `seen` back to disk (e.g. serde_json::to_string(&seen)).
    ///
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn fetch_unseen(
        &mut self,
        seen: &HashSet<String>,
    ) -> Result<Vec<(UidlEntry, Message)>> {
        let new_entries = self.unseen_uids(seen).await?;
        let mut results = Vec::with_capacity(new_entries.len());
        for entry in new_entries {
            let msg = self.retr(entry.message_id).await?;
            results.push((entry, msg));
        }
        Ok(results)
    }

    /// Remove UIDs from `seen` that no longer exist on the server.
    ///
    /// Calls `UIDL` to fetch the server's current message list, then removes
    /// any entry from `seen` whose UID is not present on the server. This
    /// prevents ghost UIDs (from deleted or expired messages) from accumulating
    /// in the seen set and incorrectly marking new messages as already-seen.
    ///
    /// Returns the list of pruned UIDs for logging or auditing. Mutates `seen`
    /// in place.
    ///
    /// Call this after connecting and authenticating, before calling
    /// [`fetch_unseen`](Self::fetch_unseen), to keep the seen set accurate.
    ///
    /// # UID Stability Caveat
    ///
    /// RFC 1939 says servers SHOULD NOT reuse UIDs, but does not mandate it.
    /// This method assumes UID stability. A pathological server that reuses a
    /// deleted UID for a new message would cause `prune_seen` to retain the old
    /// UID, making the new message appear already-seen. This matches the
    /// behavior of all major POP3 clients.
    ///
    /// # Errors
    ///
    /// - [`Pop3Error::NotAuthenticated`] — client has not logged in
    /// - [`Pop3Error::ServerError`] — server does not support `UIDL`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    /// use std::collections::HashSet;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///
    ///     // Load seen set from persistent storage.
    ///     // Example round-trip with serde_json (not a library dependency):
    ///     //   let json = std::fs::read_to_string("seen.json").unwrap_or_default();
    ///     //   let mut seen: HashSet<String> = serde_json::from_str(&json).unwrap_or_default();
    ///     let mut seen: HashSet<String> = HashSet::new();
    ///
    ///     // Prune UIDs that no longer exist on the server.
    ///     let pruned = client.prune_seen(&mut seen).await?;
    ///     if !pruned.is_empty() {
    ///         println!("Pruned {} ghost UIDs: {:?}", pruned.len(), pruned);
    ///     }
    ///
    ///     // Fetch only new messages.
    ///     let new_messages = client.fetch_unseen(&seen).await?;
    ///     for (entry, _msg) in &new_messages {
    ///         seen.insert(entry.unique_id.clone());
    ///     }
    ///
    ///     // Persist seen set back to storage.
    ///     //   let json = serde_json::to_string(&seen).unwrap();
    ///     //   std::fs::write("seen.json", json).unwrap();
    ///
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn prune_seen(&mut self, seen: &mut HashSet<String>) -> Result<Vec<String>> {
        let server_entries = self.uidl(None).await?;
        let server_uids: HashSet<&str> = server_entries
            .iter()
            .map(|e| e.unique_id.as_str())
            .collect();
        let mut pruned = Vec::new();
        seen.retain(|uid| {
            if server_uids.contains(uid.as_str()) {
                true
            } else {
                pruned.push(uid.clone());
                false
            }
        });
        Ok(pruned)
    }

    /// Send a no-op command to keep the connection alive.
    ///
    /// Useful for long-lived connections where the server may time out idle
    /// sessions. The server replies with `+OK` and takes no other action.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///     client.noop().await?; // keepalive ping
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn noop(&mut self) -> Result<()> {
        self.require_auth()?;
        self.send_and_check("NOOP").await?;
        Ok(())
    }

    /// End the session, committing any pending deletions.
    ///
    /// This method **consumes** `self`, providing a compile-time guarantee that
    /// no further commands can be issued after the session ends.
    ///
    /// Messages marked for deletion via [`dele`](Self::dele) are permanently
    /// removed when `quit` completes. Call [`rset`](Self::rset) before `quit`
    /// to cancel all pending deletions.
    ///
    /// Dropping a `Pop3Client` without calling `quit()` closes the TCP connection
    /// silently — pending `DELE` marks are **not** committed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///     client.quit().await?;
    ///     // client is consumed — no further use is possible
    ///     Ok(())
    /// }
    /// ```
    pub async fn quit(self) -> Result<()> {
        let mut this = self;
        this.transport.send_command("QUIT").await?;
        let line = this.transport.read_line().await?;
        response::parse_status_line(&line)?;
        this.transport.set_closed();
        Ok(())
        // `this` is dropped here, TCP connection closes
    }

    /// Retrieve the headers and the first `lines` lines of a message body.
    ///
    /// Useful for previewing messages without downloading the full content.
    /// Pass `lines = 0` to retrieve only the headers.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     client.login("user", "pass").await?;
    ///     let preview = client.top(1, 5).await?; // headers + 5 body lines
    ///     println!("{}", preview.data);
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn top(&mut self, message_id: u32, lines: u32) -> Result<Message> {
        self.require_auth()?;
        validate_message_id(message_id)?;
        self.send_and_check(&format!("TOP {message_id} {lines}"))
            .await?;
        let data = self.transport.read_multiline().await?;
        Ok(Message { data })
    }

    /// Query the server for its supported capabilities (RFC 2449).
    ///
    /// Returns a list of [`Capability`] items. Common capabilities include
    /// `TOP`, `UIDL`, `SASL`, and `STLS`. This command is permitted before
    /// authentication.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pop3::Pop3Client;
    ///
    /// #[tokio::main]
    /// async fn main() -> pop3::Result<()> {
    ///     let mut client = Pop3Client::connect_default("pop.example.com:110").await?;
    ///     let caps = client.capa().await?;
    ///     for cap in &caps {
    ///         println!("{}: {:?}", cap.name, cap.arguments);
    ///     }
    ///     client.quit().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn capa(&mut self) -> Result<Vec<Capability>> {
        self.send_and_check("CAPA").await?;
        let body = self.transport.read_multiline().await?;
        Ok(response::parse_capa(&body))
    }

    /// Send a command to the server, read the status line, and return the status text.
    async fn send_and_check(&mut self, cmd: &str) -> Result<String> {
        self.transport.send_command(cmd).await?;
        let line = self.transport.read_line().await?;
        let text = response::parse_status_line(&line)?;
        Ok(text.to_string())
    }

    fn require_auth(&self) -> Result<()> {
        if self.state != SessionState::Authenticated {
            Err(Pop3Error::NotAuthenticated)
        } else {
            Ok(())
        }
    }
}

// ── MIME integration (requires `mime` feature) ──────────────────────────

#[cfg(feature = "mime")]
impl Pop3Client {
    /// Retrieve a message and parse it as a structured RFC 5322 / MIME object.
    ///
    /// Calls [`retr()`](Self::retr) internally, then feeds the raw bytes to
    /// `mail-parser` for structured parsing.  Returns an owned
    /// [`ParsedMessage`](crate::ParsedMessage) with no lifetime ties to the
    /// client.
    ///
    /// # Errors
    ///
    /// - Any error that [`retr()`](Self::retr) can return (I/O, protocol,
    ///   authentication).
    /// - [`Pop3Error::MimeParse`] if the retrieved content cannot be parsed as
    ///   a valid RFC 5322 message (e.g., no recognizable headers).  This means
    ///   the network retrieval succeeded, but the content is not a valid email.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # #[cfg(feature = "mime")]
    /// # async fn example() -> pop3::Result<()> {
    /// use pop3::Pop3Client;
    ///
    /// let mut client = Pop3Client::connect(
    ///     ("pop.example.com", 110),
    ///     std::time::Duration::from_secs(30),
    /// ).await?;
    /// client.login("user", "pass").await?;
    ///
    /// let msg = client.retr_parsed(1).await?;
    /// println!("Subject: {:?}", msg.subject());
    /// println!("From: {:?}", msg.from());
    /// println!("Body: {:?}", msg.body_text(0));
    /// # Ok(())
    /// # }
    /// ```
    pub async fn retr_parsed(
        &mut self,
        message_id: u32,
    ) -> crate::Result<mail_parser::Message<'static>> {
        let msg = self.retr(message_id).await?;
        mail_parser::MessageParser::default()
            .parse(msg.data.as_bytes())
            .map(|m| m.into_owned())
            .ok_or_else(|| {
                Pop3Error::MimeParse(format!(
                    "message {message_id} could not be parsed as RFC 5322"
                ))
            })
    }

    /// Retrieve message headers (and optionally N body lines) and parse them
    /// as a structured RFC 5322 / MIME object.
    ///
    /// Calls [`top()`](Self::top) internally, then feeds the raw bytes to
    /// `mail-parser`.
    ///
    /// **Note:** The TOP command returns all headers but only the first `lines`
    /// lines of the message body (RFC 1939 Section 7).  The parsed message's
    /// body may therefore be truncated or empty.  Use [`retr_parsed()`](Self::retr_parsed)
    /// for the complete message.
    ///
    /// # Errors
    ///
    /// - Any error that [`top()`](Self::top) can return (I/O, protocol,
    ///   authentication).
    /// - [`Pop3Error::MimeParse`] if the retrieved content cannot be parsed as
    ///   a valid RFC 5322 message.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # #[cfg(feature = "mime")]
    /// # async fn example() -> pop3::Result<()> {
    /// use pop3::Pop3Client;
    ///
    /// let mut client = Pop3Client::connect(
    ///     ("pop.example.com", 110),
    ///     std::time::Duration::from_secs(30),
    /// ).await?;
    /// client.login("user", "pass").await?;
    ///
    /// // Retrieve headers + first 0 body lines (headers only)
    /// let msg = client.top_parsed(1, 0).await?;
    /// println!("Subject: {:?}", msg.subject());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn top_parsed(
        &mut self,
        message_id: u32,
        lines: u32,
    ) -> crate::Result<mail_parser::Message<'static>> {
        let msg = self.top(message_id, lines).await?;
        mail_parser::MessageParser::default()
            .parse(msg.data.as_bytes())
            .map(|m| m.into_owned())
            .ok_or_else(|| {
                Pop3Error::MimeParse(format!(
                    "message {message_id} could not be parsed as RFC 5322"
                ))
            })
    }
}

/// Create an authenticated mock Pop3Client for use in tests outside this module.
///
/// This is the `pub(crate)` counterpart to the module-private
/// `build_authenticated_test_client` helper. Used by reconnect.rs tests to
/// inject a mock Pop3Client into ReconnectingClient::new_for_test.
#[cfg(test)]
pub(crate) fn build_authenticated_mock_client(mock: tokio_test::io::Mock) -> Pop3Client {
    let transport = Transport::mock(mock);
    Pop3Client {
        transport,
        greeting: String::new(),
        state: SessionState::Authenticated,
        is_pipelining: false,
    }
}

#[cfg(test)]
fn build_test_client(mock: tokio_test::io::Mock) -> Pop3Client {
    let transport = Transport::mock(mock);
    Pop3Client {
        transport,
        greeting: String::new(),
        state: SessionState::Connected,
        is_pipelining: false,
    }
}

#[cfg(test)]
fn build_authenticated_test_client(mock: tokio_test::io::Mock) -> Pop3Client {
    let transport = Transport::mock(mock);
    Pop3Client {
        transport,
        greeting: String::new(),
        state: SessionState::Authenticated,
        is_pipelining: false,
    }
}

#[cfg(test)]
fn build_authenticated_test_client_with_pipelining(mock: tokio_test::io::Mock) -> Pop3Client {
    let transport = Transport::mock(mock);
    Pop3Client {
        transport,
        greeting: String::new(),
        state: SessionState::Authenticated,
        is_pipelining: true,
    }
}

#[cfg(test)]
fn build_test_client_with_greeting(mock: tokio_test::io::Mock, greeting: &str) -> Pop3Client {
    let transport = Transport::mock(mock);
    Pop3Client {
        transport,
        greeting: greeting.to_string(),
        state: SessionState::Connected,
        is_pipelining: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_test::io::Builder;

    #[test]
    fn test_crlf_injection_rejected() {
        assert!(check_no_crlf("normal").is_ok());
        assert!(check_no_crlf("has\rnewline").is_err());
        assert!(check_no_crlf("has\nnewline").is_err());
        assert!(check_no_crlf("has\r\nboth").is_err());
        assert!(check_no_crlf("").is_ok());
    }

    #[test]
    fn test_validate_message_id() {
        assert!(validate_message_id(1).is_ok());
        assert!(validate_message_id(100).is_ok());
        assert!(validate_message_id(0).is_err());
    }

    #[test]
    fn session_state_derives_debug() {
        // Compile-time proof that SessionState implements Debug
        let state = SessionState::Connected;
        let _ = format!("{:?}", state);
    }

    // --- is_encrypted: plain/mock connections are not encrypted ---

    #[test]
    fn is_encrypted_returns_false_for_plain_client() {
        let mock = Builder::new().build();
        let client = build_test_client(mock);
        assert!(!client.is_encrypted());
    }

    #[test]
    fn is_encrypted_returns_false_for_authenticated_client() {
        let mock = Builder::new().build();
        let client = build_authenticated_test_client(mock);
        assert!(!client.is_encrypted());
    }

    // --- is_closed: tracks connection closed state ---

    #[test]
    fn is_closed_false_on_new_client() {
        let mock = Builder::new().build();
        let client = build_test_client(mock);
        assert!(!client.is_closed());
    }

    #[tokio::test]
    async fn is_closed_true_after_eof() {
        // Write STAT\r\n, then return EOF on the read (no read data)
        let mock = Builder::new().write(b"STAT\r\n").build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.stat().await;
        assert!(result.is_err());
        assert!(client.is_closed());
    }

    // --- FIX-01: rset() must send "RSET\r\n", not "RETR\r\n" ---

    #[tokio::test]
    async fn rset_sends_correct_command_fix01() {
        let mock = Builder::new().write(b"RSET\r\n").read(b"+OK\r\n").build();
        let mut client = build_authenticated_test_client(mock);
        client.rset().await.unwrap();
    }

    // --- FIX-02: noop() must send "NOOP\r\n" (uppercase) ---

    #[tokio::test]
    async fn noop_sends_correct_command_fix02() {
        let mock = Builder::new().write(b"NOOP\r\n").read(b"+OK\r\n").build();
        let mut client = build_authenticated_test_client(mock);
        client.noop().await.unwrap();
    }

    // --- FIX-03: login() sets authenticated only after both USER and PASS succeed ---

    #[tokio::test]
    async fn login_sets_authenticated_after_pass_ok_fix03() {
        let mock = Builder::new()
            .write(b"USER user\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS pass\r\n")
            .read(b"+OK logged in\r\n")
            // CAPA probe after successful login
            .write(b"CAPA\r\n")
            .read(b"+OK\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_test_client(mock);
        assert_eq!(client.state(), SessionState::Connected);
        client.login("user", "pass").await.unwrap();
        assert_eq!(client.state(), SessionState::Authenticated);
    }

    #[tokio::test]
    async fn login_not_authenticated_when_pass_fails_fix03() {
        let mock = Builder::new()
            .write(b"USER user\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS wrongpass\r\n")
            .read(b"-ERR invalid password\r\n")
            .build();
        let mut client = build_test_client(mock);
        let result = client.login("user", "wrongpass").await;
        assert!(result.is_err());
        assert_eq!(
            client.state(),
            SessionState::Connected,
            "FIX-03: must NOT set authenticated when PASS returns -ERR"
        );
        match result.unwrap_err() {
            Pop3Error::AuthFailed(msg) => assert_eq!(msg, "invalid password"),
            other => panic!("expected AuthFailed, got: {other:?}"),
        }
    }

    // --- FIX-04: list round-trip validates parse_list_single via mock I/O ---

    #[tokio::test]
    async fn list_single_round_trip_fix04() {
        let mock = Builder::new()
            .write(b"LIST 1\r\n")
            .read(b"+OK 1 1234\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let entries = client.list(Some(1)).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].size, 1234);
    }

    // --- Login happy and error paths ---

    #[tokio::test]
    async fn login_happy_path() {
        let mock = Builder::new()
            .write(b"USER alice\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS secret\r\n")
            .read(b"+OK logged in\r\n")
            // CAPA probe after successful login
            .write(b"CAPA\r\n")
            .read(b"+OK\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_test_client(mock);
        let result = client.login("alice", "secret").await;
        assert!(result.is_ok());
        assert_eq!(client.state(), SessionState::Authenticated);
    }

    #[tokio::test]
    async fn login_user_command_rejected_returns_auth_failed() {
        let mock = Builder::new()
            .write(b"USER nobody\r\n")
            .read(b"-ERR no such user\r\n")
            .build();
        let mut client = build_test_client(mock);
        let result = client.login("nobody", "pass").await;
        assert!(result.is_err());
        assert_eq!(client.state(), SessionState::Connected);
        match result.unwrap_err() {
            Pop3Error::AuthFailed(msg) => assert!(msg.contains("no such user")),
            other => panic!("expected AuthFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn login_rejects_crlf_in_username() {
        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        let result = client.login("user\r\nINJECT", "pass").await;
        assert!(matches!(result.unwrap_err(), Pop3Error::InvalidInput));
    }

    #[tokio::test]
    async fn login_rejects_crlf_in_password() {
        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        let result = client.login("user", "pass\r\nINJECT").await;
        assert!(matches!(result.unwrap_err(), Pop3Error::InvalidInput));
    }

    #[tokio::test]
    async fn login_rejects_when_already_authenticated() {
        let mock = Builder::new().build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.login("user", "pass").await;
        assert!(matches!(result, Err(Pop3Error::NotAuthenticated)));
    }

    // --- stat happy and error paths ---

    #[tokio::test]
    async fn stat_happy_path() {
        let mock = Builder::new()
            .write(b"STAT\r\n")
            .read(b"+OK 5 12345\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let stat = client.stat().await.unwrap();
        assert_eq!(stat.message_count, 5);
        assert_eq!(stat.mailbox_size, 12345);
    }

    #[tokio::test]
    async fn stat_server_error() {
        let mock = Builder::new()
            .write(b"STAT\r\n")
            .read(b"-ERR mailbox locked\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.stat().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(msg) => assert!(msg.contains("mailbox locked")),
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    // --- list single happy and error paths ---

    #[tokio::test]
    async fn list_single_happy_path() {
        let mock = Builder::new()
            .write(b"LIST 1\r\n")
            .read(b"+OK 1 1234\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let entries = client.list(Some(1)).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].size, 1234);
    }

    #[tokio::test]
    async fn list_single_server_error() {
        let mock = Builder::new()
            .write(b"LIST 99\r\n")
            .read(b"-ERR no such message\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.list(Some(99)).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(msg) => assert!(msg.contains("no such message")),
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn list_all_happy_path() {
        let mock = Builder::new()
            .write(b"LIST\r\n")
            .read(b"+OK\r\n1 100\r\n2 200\r\n.\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let entries = client.list(None).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].size, 100);
        assert_eq!(entries[1].message_id, 2);
        assert_eq!(entries[1].size, 200);
    }

    // --- retr happy, error, and dot-unstuffing paths ---

    #[tokio::test]
    async fn retr_happy_path() {
        let mock = Builder::new()
            .write(b"RETR 1\r\n")
            .read(b"+OK\r\nSubject: Test\r\n\r\nBody line 1\r\n.\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let msg = client.retr(1).await.unwrap();
        assert!(msg.data.contains("Subject: Test"));
        assert!(msg.data.contains("Body line 1"));
    }

    #[tokio::test]
    async fn retr_server_error() {
        let mock = Builder::new()
            .write(b"RETR 1\r\n")
            .read(b"-ERR no such message\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.retr(1).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(_) => {}
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn retr_dot_unstuffing() {
        // Server sends a line starting with ".." which should become "."
        let mock = Builder::new()
            .write(b"RETR 1\r\n")
            .read(b"+OK\r\n..This had a leading dot\r\nNormal line\r\n.\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let msg = client.retr(1).await.unwrap();
        assert!(
            msg.data.contains(".This had a leading dot"),
            "dot-unstuffing must remove one leading dot"
        );
        assert!(
            !msg.data.contains("..This"),
            "double dot must be reduced to single dot"
        );
    }

    // --- dele happy and error paths ---

    #[tokio::test]
    async fn dele_happy_path() {
        let mock = Builder::new()
            .write(b"DELE 1\r\n")
            .read(b"+OK message deleted\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        client.dele(1).await.unwrap();
    }

    #[tokio::test]
    async fn dele_server_error() {
        let mock = Builder::new()
            .write(b"DELE 1\r\n")
            .read(b"-ERR message locked\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.dele(1).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(_) => {}
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    // --- uidl single, all, and error paths ---

    #[tokio::test]
    async fn uidl_single_happy_path() {
        let mock = Builder::new()
            .write(b"UIDL 1\r\n")
            .read(b"+OK 1 abc123\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let entries = client.uidl(Some(1)).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].unique_id, "abc123");
    }

    #[tokio::test]
    async fn uidl_all_happy_path() {
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n1 uid-a\r\n2 uid-b\r\n.\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let entries = client.uidl(None).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].unique_id, "uid-a");
        assert_eq!(entries[1].message_id, 2);
        assert_eq!(entries[1].unique_id, "uid-b");
    }

    #[tokio::test]
    async fn uidl_server_error() {
        let mock = Builder::new()
            .write(b"UIDL 1\r\n")
            .read(b"-ERR\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.uidl(Some(1)).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(_) => {}
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    // --- quit happy path and move semantics ---

    #[tokio::test]
    async fn quit_happy_path() {
        let mock = Builder::new()
            .write(b"QUIT\r\n")
            .read(b"+OK goodbye\r\n")
            .build();
        let client = build_authenticated_test_client(mock);
        client.quit().await.unwrap();
        // client is consumed — cannot use after this line (compile-time guarantee)
    }

    #[tokio::test]
    async fn quit_consumes_client() {
        let mock = Builder::new()
            .write(b"QUIT\r\n")
            .read(b"+OK goodbye\r\n")
            .build();
        let client = build_authenticated_test_client(mock);
        assert_eq!(client.state(), SessionState::Authenticated);
        client.quit().await.unwrap();
        // Uncomment to verify compile error:
        // client.stat().await; // error[E0382]: borrow of moved value
    }

    // --- capa happy and error paths ---

    #[tokio::test]
    async fn capa_happy_path() {
        let mock = Builder::new()
            .write(b"CAPA\r\n")
            .read(b"+OK\r\nTOP\r\nUIDL\r\nSASL PLAIN\r\n.\r\n")
            .build();
        let mut client = build_test_client(mock);
        let caps = client.capa().await.unwrap();
        assert_eq!(caps.len(), 3);
    }

    #[tokio::test]
    async fn capa_server_error() {
        let mock = Builder::new()
            .write(b"CAPA\r\n")
            .read(b"-ERR not supported\r\n")
            .build();
        let mut client = build_test_client(mock);
        let result = client.capa().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(_) => {}
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    // --- top happy and error paths ---

    #[tokio::test]
    async fn top_happy_path() {
        let mock = Builder::new()
            .write(b"TOP 1 5\r\n")
            .read(b"+OK\r\nSubject: Test\r\n\r\nFirst line\r\n.\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let msg = client.top(1, 5).await.unwrap();
        assert!(msg.data.contains("Subject: Test"));
        assert!(msg.data.contains("First line"));
    }

    #[tokio::test]
    async fn top_server_error() {
        let mock = Builder::new()
            .write(b"TOP 1 5\r\n")
            .read(b"-ERR no such message\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.top(1, 5).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(_) => {}
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    // --- Multi-command integration flow tests ---

    #[tokio::test]
    async fn full_session_flow() {
        let mock = Builder::new()
            .write(b"USER alice\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS secret\r\n")
            .read(b"+OK logged in\r\n")
            // CAPA probe after successful login
            .write(b"CAPA\r\n")
            .read(b"+OK\r\n")
            .read(b".\r\n")
            .write(b"STAT\r\n")
            .read(b"+OK 2 5000\r\n")
            .write(b"LIST\r\n")
            .read(b"+OK\r\n1 2500\r\n2 2500\r\n.\r\n")
            .write(b"RETR 1\r\n")
            .read(b"+OK\r\nSubject: Hello\r\n\r\nBody\r\n.\r\n")
            .write(b"QUIT\r\n")
            .read(b"+OK goodbye\r\n")
            .build();

        let mut client = build_test_client(mock);

        // Login
        client.login("alice", "secret").await.unwrap();
        assert_eq!(client.state(), SessionState::Authenticated);

        // Stat
        let stat = client.stat().await.unwrap();
        assert_eq!(stat.message_count, 2);
        assert_eq!(stat.mailbox_size, 5000);

        // List all
        let entries = client.list(None).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].size, 2500);

        // Retr
        let msg = client.retr(1).await.unwrap();
        assert!(msg.data.contains("Subject: Hello"));

        // Quit (consumes client)
        client.quit().await.unwrap();
    }

    #[tokio::test]
    async fn capa_then_login_then_top_flow() {
        let mock = Builder::new()
            // CAPA before login
            .write(b"CAPA\r\n")
            .read(b"+OK\r\nTOP\r\nUIDL\r\nSASL PLAIN\r\nSTLS\r\n.\r\n")
            // Login
            .write(b"USER bob\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS pass123\r\n")
            .read(b"+OK welcome\r\n")
            // CAPA probe after successful login (no PIPELINING in this server)
            .write(b"CAPA\r\n")
            .read(b"+OK\r\n")
            .read(b"TOP\r\n")
            .read(b"UIDL\r\n")
            .read(b".\r\n")
            // TOP
            .write(b"TOP 1 3\r\n")
            .read(b"+OK\r\nFrom: sender@example.com\r\nSubject: Test\r\n\r\nLine 1\r\nLine 2\r\nLine 3\r\n.\r\n")
            // QUIT
            .write(b"QUIT\r\n")
            .read(b"+OK bye\r\n")
            .build();

        let mut client = build_test_client(mock);

        // CAPA (pre-auth, per RFC 2449)
        let caps = client.capa().await.unwrap();
        assert_eq!(caps.len(), 4);
        assert!(caps.iter().any(|c| c.name == "TOP"));
        assert!(caps.iter().any(|c| c.name == "STLS"));

        // Login
        client.login("bob", "pass123").await.unwrap();

        // TOP 1 3 (headers + 3 lines)
        let msg = client.top(1, 3).await.unwrap();
        assert!(msg.data.contains("From: sender@example.com"));
        assert!(msg.data.contains("Subject: Test"));
        assert!(msg.data.contains("Line 1"));
        assert!(msg.data.contains("Line 3"));

        // Quit
        client.quit().await.unwrap();
    }

    #[tokio::test]
    async fn uidl_then_dele_then_rset_flow() {
        let mock = Builder::new()
            .write(b"USER user\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS pass\r\n")
            .read(b"+OK\r\n")
            // CAPA probe after successful login
            .write(b"CAPA\r\n")
            .read(b"+OK\r\n")
            .read(b".\r\n")
            // UIDL all
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n1 msg-aaa\r\n2 msg-bbb\r\n3 msg-ccc\r\n.\r\n")
            // DELE 2
            .write(b"DELE 2\r\n")
            .read(b"+OK message 2 deleted\r\n")
            // RSET (undelete)
            .write(b"RSET\r\n")
            .read(b"+OK\r\n")
            // NOOP
            .write(b"NOOP\r\n")
            .read(b"+OK\r\n")
            // QUIT
            .write(b"QUIT\r\n")
            .read(b"+OK\r\n")
            .build();

        let mut client = build_test_client(mock);
        client.login("user", "pass").await.unwrap();

        // UIDL all
        let uids = client.uidl(None).await.unwrap();
        assert_eq!(uids.len(), 3);
        assert_eq!(uids[1].unique_id, "msg-bbb");

        // DELE
        client.dele(2).await.unwrap();

        // RSET (unmark deletion)
        client.rset().await.unwrap();

        // NOOP (keepalive)
        client.noop().await.unwrap();

        // Quit
        client.quit().await.unwrap();
    }

    // --- stls: STARTTLS RFC 2595 guards ---

    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    #[tokio::test]
    async fn stls_rejects_when_authenticated() {
        let mock = Builder::new().build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.stls("example.com").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(msg) => {
                assert!(msg.contains("not allowed after authentication"))
            }
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    #[tokio::test]
    async fn stls_rejects_server_err() {
        let mock = Builder::new()
            .write(b"STLS\r\n")
            .read(b"-ERR STLS not supported\r\n")
            .build();
        let mut client = build_test_client(mock);
        let result = client.stls("example.com").await;
        assert!(matches!(result, Err(Pop3Error::ServerError(_))));
    }

    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    #[tokio::test]
    async fn stls_sends_command_correctly() {
        // Mock returns +OK, then upgrade will fail because Mock is not a real
        // TCP stream (it's InnerStream::Mock, not InnerStream::Plain). The test
        // verifies the STLS command was sent and +OK was received before the
        // upgrade attempt.
        let mock = Builder::new()
            .write(b"STLS\r\n")
            .read(b"+OK begin TLS negotiation\r\n")
            .build();
        let mut client = build_test_client(mock);
        let result = client.stls("example.com").await;
        // upgrade_in_place fails because mock is not a Plain TcpStream — expected
        assert!(result.is_err());
    }

    // --- Not-authenticated guard: commands require authentication ---

    #[tokio::test]
    async fn commands_require_authentication() {
        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        assert!(matches!(
            client.stat().await,
            Err(Pop3Error::NotAuthenticated)
        ));

        // Each command needs its own mock since stat() consumed the mock
        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        assert!(matches!(
            client.list(None).await,
            Err(Pop3Error::NotAuthenticated)
        ));

        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        assert!(matches!(
            client.retr(1).await,
            Err(Pop3Error::NotAuthenticated)
        ));

        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        assert!(matches!(
            client.dele(1).await,
            Err(Pop3Error::NotAuthenticated)
        ));
    }

    // --- login() with RESP-CODES ---

    #[tokio::test]
    async fn login_with_auth_resp_code_returns_auth_failed() {
        let mock = Builder::new()
            .write(b"USER user\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS wrong\r\n")
            .read(b"-ERR [AUTH] invalid credentials\r\n")
            .build();
        let mut client = build_test_client(mock);
        let result = client.login("user", "wrong").await;
        match result.unwrap_err() {
            Pop3Error::AuthFailed(msg) => assert_eq!(msg, "invalid credentials"),
            other => panic!("expected AuthFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn login_with_in_use_resp_code_returns_mailbox_in_use() {
        let mock = Builder::new()
            .write(b"USER user\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS pass\r\n")
            .read(b"-ERR [IN-USE] mailbox locked\r\n")
            .build();
        let mut client = build_test_client(mock);
        let result = client.login("user", "pass").await;
        match result.unwrap_err() {
            Pop3Error::MailboxInUse(msg) => assert_eq!(msg, "mailbox locked"),
            other => panic!("expected MailboxInUse, got: {other:?}"),
        }
    }

    // --- APOP authentication tests ---

    #[allow(deprecated)]
    #[tokio::test]
    async fn apop_happy_path() {
        // RFC 1939 section 7 test vector:
        // timestamp = <1896.697170952@dbc.mtview.ca.us>
        // password = tanstaaf
        // digest = c4c9334bac560ecc979e58001b3e22fb
        let mock = Builder::new()
            .write(b"APOP mrose c4c9334bac560ecc979e58001b3e22fb\r\n")
            .read(b"+OK mastrstrmt mastrstrmt is strstrmt\r\n")
            // CAPA probe after successful apop
            .write(b"CAPA\r\n")
            .read(b"+OK\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_test_client_with_greeting(
            mock,
            "POP3 server ready <1896.697170952@dbc.mtview.ca.us>",
        );
        client.apop("mrose", "tanstaaf").await.unwrap();
        assert_eq!(client.state(), SessionState::Authenticated);
    }

    #[allow(deprecated)]
    #[tokio::test]
    async fn apop_no_timestamp_in_greeting() {
        let mock = Builder::new().build();
        let mut client = build_test_client_with_greeting(mock, "POP3 server ready");
        let result = client.apop("user", "pass").await;
        match result.unwrap_err() {
            Pop3Error::ServerError(msg) => assert!(msg.contains("no timestamp")),
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    #[allow(deprecated)]
    #[tokio::test]
    async fn apop_server_rejects_returns_auth_failed() {
        let mock = Builder::new()
            .write(b"APOP user c4c9334bac560ecc979e58001b3e22fb\r\n")
            .read(b"-ERR permission denied\r\n")
            .build();
        let mut client =
            build_test_client_with_greeting(mock, "ready <1896.697170952@dbc.mtview.ca.us>");
        let result = client.apop("user", "tanstaaf").await;
        match result.unwrap_err() {
            Pop3Error::AuthFailed(msg) => assert!(msg.contains("permission denied")),
            other => panic!("expected AuthFailed, got: {other:?}"),
        }
        assert_eq!(client.state(), SessionState::Connected);
    }

    #[allow(deprecated)]
    #[tokio::test]
    async fn apop_with_auth_resp_code_returns_auth_failed() {
        let mock = Builder::new()
            .write(b"APOP user c4c9334bac560ecc979e58001b3e22fb\r\n")
            .read(b"-ERR [AUTH] invalid credentials\r\n")
            .build();
        let mut client =
            build_test_client_with_greeting(mock, "ready <1896.697170952@dbc.mtview.ca.us>");
        let result = client.apop("user", "tanstaaf").await;
        match result.unwrap_err() {
            Pop3Error::AuthFailed(msg) => assert_eq!(msg, "invalid credentials"),
            other => panic!("expected AuthFailed, got: {other:?}"),
        }
    }

    #[allow(deprecated)]
    #[tokio::test]
    async fn apop_with_in_use_resp_code_does_not_promote() {
        let mock = Builder::new()
            .write(b"APOP user c4c9334bac560ecc979e58001b3e22fb\r\n")
            .read(b"-ERR [IN-USE] mailbox locked\r\n")
            .build();
        let mut client =
            build_test_client_with_greeting(mock, "ready <1896.697170952@dbc.mtview.ca.us>");
        let result = client.apop("user", "tanstaaf").await;
        match result.unwrap_err() {
            Pop3Error::MailboxInUse(msg) => assert_eq!(msg, "mailbox locked"),
            other => panic!("expected MailboxInUse, got: {other:?}"),
        }
    }

    #[allow(deprecated)]
    #[tokio::test]
    async fn apop_rejects_when_already_authenticated() {
        let mock = Builder::new().build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.apop("user", "pass").await;
        assert!(matches!(result, Err(Pop3Error::NotAuthenticated)));
    }

    #[allow(deprecated)]
    #[tokio::test]
    async fn apop_rejects_crlf_in_username() {
        let mock = Builder::new().build();
        let mut client = build_test_client_with_greeting(mock, "ready <timestamp@host>");
        let result = client.apop("user\r\n", "pass").await;
        assert!(matches!(result.unwrap_err(), Pop3Error::InvalidInput));
    }

    #[allow(deprecated)]
    #[tokio::test]
    async fn apop_rejects_crlf_in_password() {
        let mock = Builder::new().build();
        let mut client = build_test_client_with_greeting(mock, "ready <timestamp@host>");
        let result = client.apop("user", "pass\r\n").await;
        assert!(matches!(result.unwrap_err(), Pop3Error::InvalidInput));
    }

    #[test]
    fn extract_apop_timestamp_from_greeting() {
        assert_eq!(
            extract_apop_timestamp("POP3 server ready <1896.697170952@dbc.mtview.ca.us>"),
            Some("<1896.697170952@dbc.mtview.ca.us>")
        );
    }

    #[test]
    fn extract_apop_timestamp_at_beginning() {
        assert_eq!(
            extract_apop_timestamp("<ts@host> POP3 ready"),
            Some("<ts@host>")
        );
    }

    #[test]
    fn extract_apop_timestamp_missing() {
        assert_eq!(extract_apop_timestamp("POP3 server ready"), None);
    }

    #[test]
    fn extract_apop_timestamp_no_close_bracket() {
        assert_eq!(extract_apop_timestamp("POP3 <broken"), None);
    }

    #[test]
    fn compute_apop_digest_rfc_vector() {
        // RFC 1939 section 7 test vector
        let digest = compute_apop_digest("<1896.697170952@dbc.mtview.ca.us>", "tanstaaf");
        assert_eq!(digest, "c4c9334bac560ecc979e58001b3e22fb");
    }

    // --- retr_many and dele_many tests (PIPE-01, PIPE-03, PIPE-04, PIPE-05) ---

    #[tokio::test]
    async fn retr_many_sequential_fallback() {
        // Server does NOT advertise PIPELINING -- sequential mode
        let mock = Builder::new()
            // retr(1) sequential
            .write(b"RETR 1\r\n")
            .read(b"+OK\r\n")
            .read(b"message 1 body\r\n")
            .read(b".\r\n")
            // retr(2) sequential
            .write(b"RETR 2\r\n")
            .read(b"+OK\r\n")
            .read(b"message 2 body\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        // is_pipelining is false by default
        let results = client.retr_many(&[1, 2]).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());
        assert_eq!(results[0].as_ref().unwrap().data, "message 1 body\n");
        assert_eq!(results[1].as_ref().unwrap().data, "message 2 body\n");
    }

    #[tokio::test]
    async fn dele_many_sequential_fallback() {
        let mock = Builder::new()
            .write(b"DELE 1\r\n")
            .read(b"+OK message 1 deleted\r\n")
            .write(b"DELE 2\r\n")
            .read(b"+OK message 2 deleted\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let results = client.dele_many(&[1, 2]).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());
    }

    #[tokio::test]
    async fn retr_many_pipelined_path() {
        // With pipelining, writes are batched before reads
        let mock = Builder::new()
            // All writes first (within window)
            .write(b"RETR 1\r\n")
            .write(b"RETR 2\r\n")
            .write(b"RETR 3\r\n")
            // Then all reads
            .read(b"+OK\r\n")
            .read(b"body 1\r\n")
            .read(b".\r\n")
            .read(b"+OK\r\n")
            .read(b"body 2\r\n")
            .read(b".\r\n")
            .read(b"+OK\r\n")
            .read(b"body 3\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client_with_pipelining(mock);
        let results = client.retr_many(&[1, 2, 3]).await.unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap().data, "body 1\n");
        assert_eq!(results[1].as_ref().unwrap().data, "body 2\n");
        assert_eq!(results[2].as_ref().unwrap().data, "body 3\n");
    }

    #[tokio::test]
    async fn dele_many_pipelined_path() {
        let mock = Builder::new()
            .write(b"DELE 1\r\n")
            .write(b"DELE 2\r\n")
            .write(b"DELE 3\r\n")
            .read(b"+OK message 1 deleted\r\n")
            .read(b"+OK message 2 deleted\r\n")
            .read(b"+OK message 3 deleted\r\n")
            .build();
        let mut client = build_authenticated_test_client_with_pipelining(mock);
        let results = client.dele_many(&[1, 2, 3]).await.unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[tokio::test]
    async fn retr_many_pipelined_per_item_error() {
        // Second message returns -ERR, but first and third succeed
        let mock = Builder::new()
            .write(b"RETR 1\r\n")
            .write(b"RETR 2\r\n")
            .write(b"RETR 3\r\n")
            .read(b"+OK\r\n")
            .read(b"body 1\r\n")
            .read(b".\r\n")
            .read(b"-ERR no such message\r\n")
            .read(b"+OK\r\n")
            .read(b"body 3\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client_with_pipelining(mock);
        let results = client.retr_many(&[1, 2, 3]).await.unwrap();
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok());
        assert!(results[1].is_err()); // -ERR for message 2
        assert!(results[2].is_ok());
    }

    #[tokio::test]
    async fn dele_many_pipelined_per_item_error() {
        let mock = Builder::new()
            .write(b"DELE 1\r\n")
            .write(b"DELE 2\r\n")
            .read(b"+OK deleted\r\n")
            .read(b"-ERR message already deleted\r\n")
            .build();
        let mut client = build_authenticated_test_client_with_pipelining(mock);
        let results = client.dele_many(&[1, 2]).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
    }

    #[tokio::test]
    async fn retr_many_rejects_zero_id() {
        let mock = Builder::new().build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.retr_many(&[1, 0, 3]).await;
        assert!(matches!(result, Err(Pop3Error::InvalidInput)));
    }

    #[tokio::test]
    async fn dele_many_rejects_zero_id() {
        let mock = Builder::new().build();
        let mut client = build_authenticated_test_client(mock);
        let result = client.dele_many(&[0]).await;
        assert!(matches!(result, Err(Pop3Error::InvalidInput)));
    }

    #[tokio::test]
    async fn retr_many_empty_ids_returns_empty() {
        let mock = Builder::new().build();
        let mut client = build_authenticated_test_client(mock);
        let results = client.retr_many(&[]).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn retr_many_requires_auth() {
        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        let result = client.retr_many(&[1]).await;
        assert!(matches!(result, Err(Pop3Error::NotAuthenticated)));
    }

    #[tokio::test]
    async fn dele_many_requires_auth() {
        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        let result = client.dele_many(&[1]).await;
        assert!(matches!(result, Err(Pop3Error::NotAuthenticated)));
    }

    #[tokio::test]
    async fn retr_many_windowed_8_messages() {
        // 8 messages = 2 windows of 4 (PIPE-04 windowed strategy)
        let mock = Builder::new()
            // Window 1: write 4
            .write(b"RETR 1\r\n")
            .write(b"RETR 2\r\n")
            .write(b"RETR 3\r\n")
            .write(b"RETR 4\r\n")
            // Window 1: read 4
            .read(b"+OK\r\n")
            .read(b"body1\r\n")
            .read(b".\r\n")
            .read(b"+OK\r\n")
            .read(b"body2\r\n")
            .read(b".\r\n")
            .read(b"+OK\r\n")
            .read(b"body3\r\n")
            .read(b".\r\n")
            .read(b"+OK\r\n")
            .read(b"body4\r\n")
            .read(b".\r\n")
            // Window 2: write 4
            .write(b"RETR 5\r\n")
            .write(b"RETR 6\r\n")
            .write(b"RETR 7\r\n")
            .write(b"RETR 8\r\n")
            // Window 2: read 4
            .read(b"+OK\r\n")
            .read(b"body5\r\n")
            .read(b".\r\n")
            .read(b"+OK\r\n")
            .read(b"body6\r\n")
            .read(b".\r\n")
            .read(b"+OK\r\n")
            .read(b"body7\r\n")
            .read(b".\r\n")
            .read(b"+OK\r\n")
            .read(b"body8\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client_with_pipelining(mock);
        let results = client.retr_many(&[1, 2, 3, 4, 5, 6, 7, 8]).await.unwrap();
        assert_eq!(results.len(), 8);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    // --- Pipelining detection tests (PIPE-02) ---

    #[tokio::test]
    async fn pipelining_detected_via_capa() {
        let mock = Builder::new()
            .write(b"USER user\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS pass\r\n")
            .read(b"+OK logged in\r\n")
            // CAPA probe after login
            .write(b"CAPA\r\n")
            .read(b"+OK\r\n")
            .read(b"TOP\r\n")
            .read(b"UIDL\r\n")
            .read(b"PIPELINING\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_test_client(mock);
        client.login("user", "pass").await.unwrap();
        assert!(client.supports_pipelining());
    }

    #[tokio::test]
    async fn pipelining_not_detected_without_capa_entry() {
        let mock = Builder::new()
            .write(b"USER user\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS pass\r\n")
            .read(b"+OK logged in\r\n")
            // CAPA probe after login -- no PIPELINING
            .write(b"CAPA\r\n")
            .read(b"+OK\r\n")
            .read(b"TOP\r\n")
            .read(b"UIDL\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_test_client(mock);
        client.login("user", "pass").await.unwrap();
        assert!(!client.supports_pipelining());
    }

    #[tokio::test]
    async fn pipelining_false_when_capa_fails() {
        let mock = Builder::new()
            .write(b"USER user\r\n")
            .read(b"+OK\r\n")
            .write(b"PASS pass\r\n")
            .read(b"+OK logged in\r\n")
            // CAPA probe fails -- server doesn't support CAPA
            .write(b"CAPA\r\n")
            .read(b"-ERR command not supported\r\n")
            .build();
        let mut client = build_test_client(mock);
        client.login("user", "pass").await.unwrap();
        assert!(!client.supports_pipelining());
    }

    #[test]
    fn supports_pipelining_false_before_login() {
        let mock = Builder::new().build();
        let client = build_test_client(mock);
        assert!(!client.supports_pipelining());
    }

    // =========================================================================
    // Incremental Sync: unseen_uids
    // =========================================================================

    #[tokio::test]
    async fn unseen_uids_returns_all_when_seen_is_empty() {
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b"1 abc123\r\n")
            .read(b"2 def456\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let seen: HashSet<String> = HashSet::new();
        let result = client.unseen_uids(&seen).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].unique_id, "abc123");
        assert_eq!(result[1].unique_id, "def456");
    }

    #[tokio::test]
    async fn unseen_uids_filters_seen_entries() {
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b"1 abc123\r\n")
            .read(b"2 def456\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let seen: HashSet<String> = ["abc123".to_string()].into();
        let result = client.unseen_uids(&seen).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].unique_id, "def456");
        assert_eq!(result[0].message_id, 2);
    }

    #[tokio::test]
    async fn unseen_uids_returns_empty_when_all_seen() {
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b"1 abc123\r\n")
            .read(b"2 def456\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let seen: HashSet<String> = ["abc123".to_string(), "def456".to_string()].into();
        let result = client.unseen_uids(&seen).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn unseen_uids_empty_mailbox_returns_empty() {
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let seen: HashSet<String> = HashSet::new();
        let result = client.unseen_uids(&seen).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn unseen_uids_requires_auth() {
        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        let seen: HashSet<String> = HashSet::new();
        let result = client.unseen_uids(&seen).await;
        assert!(matches!(result, Err(Pop3Error::NotAuthenticated)));
    }

    // =========================================================================
    // Incremental Sync: fetch_unseen
    // =========================================================================

    #[tokio::test]
    async fn fetch_unseen_returns_new_messages_with_entries() {
        let mock = Builder::new()
            // unseen_uids -> uidl(None)
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b"1 abc123\r\n")
            .read(b"2 def456\r\n")
            .read(b".\r\n")
            // retr(2) -- only def456 is unseen
            .write(b"RETR 2\r\n")
            .read(b"+OK\r\n")
            .read(b"From: test@example.com\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let seen: HashSet<String> = ["abc123".to_string()].into();
        let result = client.fetch_unseen(&seen).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.unique_id, "def456");
        assert_eq!(result[0].0.message_id, 2);
        assert!(result[0].1.data.contains("From: test@example.com"));
    }

    #[tokio::test]
    async fn fetch_unseen_empty_when_all_seen() {
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b"1 abc123\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let seen: HashSet<String> = ["abc123".to_string()].into();
        let result = client.fetch_unseen(&seen).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn fetch_unseen_does_not_mutate_seen() {
        // fetch_unseen takes &HashSet<String> -- can only be verified by type, but
        // we confirm the seen set is unchanged from the caller's perspective by
        // checking the count before and after (structural test).
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b"1 abc123\r\n")
            .read(b"2 def456\r\n")
            .read(b".\r\n")
            .write(b"RETR 2\r\n")
            .read(b"+OK\r\n")
            .read(b"body\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let seen: HashSet<String> = ["abc123".to_string()].into();
        let before_len = seen.len();
        let _ = client.fetch_unseen(&seen).await.unwrap();
        // seen is unchanged -- fetch_unseen only takes &HashSet (immutable borrow)
        assert_eq!(seen.len(), before_len);
    }

    #[tokio::test]
    async fn fetch_unseen_fails_fast_on_retr_error() {
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b"1 abc123\r\n")
            .read(b"2 def456\r\n")
            .read(b".\r\n")
            // retr(1) succeeds
            .write(b"RETR 1\r\n")
            .read(b"+OK\r\n")
            .read(b"body of 1\r\n")
            .read(b".\r\n")
            // retr(2) fails -- server returns -ERR
            .write(b"RETR 2\r\n")
            .read(b"-ERR no such message\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let seen: HashSet<String> = HashSet::new();
        // fetch_unseen propagates the error immediately; no partial results
        let result = client.fetch_unseen(&seen).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fetch_unseen_requires_auth() {
        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        let seen: HashSet<String> = HashSet::new();
        let result = client.fetch_unseen(&seen).await;
        assert!(matches!(result, Err(Pop3Error::NotAuthenticated)));
    }

    // =========================================================================
    // Incremental Sync: prune_seen
    // =========================================================================

    #[tokio::test]
    async fn prune_seen_removes_ghost_uids() {
        // Server only has message 2 (def456); abc123 is a ghost
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b"2 def456\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let mut seen: HashSet<String> = ["abc123".to_string(), "def456".to_string()].into();
        let pruned = client.prune_seen(&mut seen).await.unwrap();
        assert!(!seen.contains("abc123"), "ghost uid should be pruned");
        assert!(seen.contains("def456"), "live uid should be retained");
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0], "abc123");
    }

    #[tokio::test]
    async fn prune_seen_returns_empty_when_no_ghosts() {
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b"1 abc123\r\n")
            .read(b"2 def456\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let mut seen: HashSet<String> = ["abc123".to_string()].into();
        let pruned = client.prune_seen(&mut seen).await.unwrap();
        assert!(pruned.is_empty());
        assert!(seen.contains("abc123"));
    }

    #[tokio::test]
    async fn prune_seen_empties_seen_when_server_is_empty() {
        // Server has no messages at all
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let mut seen: HashSet<String> = ["abc123".to_string(), "def456".to_string()].into();
        let pruned = client.prune_seen(&mut seen).await.unwrap();
        assert!(
            seen.is_empty(),
            "all uids should be pruned when server has no messages"
        );
        assert_eq!(pruned.len(), 2);
    }

    #[tokio::test]
    async fn prune_seen_empty_seen_is_noop() {
        let mock = Builder::new()
            .write(b"UIDL\r\n")
            .read(b"+OK\r\n")
            .read(b"1 abc123\r\n")
            .read(b".\r\n")
            .build();
        let mut client = build_authenticated_test_client(mock);
        let mut seen: HashSet<String> = HashSet::new();
        let pruned = client.prune_seen(&mut seen).await.unwrap();
        assert!(pruned.is_empty());
        assert!(seen.is_empty());
    }

    #[tokio::test]
    async fn prune_seen_requires_auth() {
        let mock = Builder::new().build();
        let mut client = build_test_client(mock);
        let mut seen: HashSet<String> = HashSet::new();
        let result = client.prune_seen(&mut seen).await;
        assert!(matches!(result, Err(Pop3Error::NotAuthenticated)));
    }
}
