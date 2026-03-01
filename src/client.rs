use std::time::Duration;

use crate::error::{Pop3Error, Result};
use crate::response;
use crate::transport::Transport;
use crate::types::{Capability, ListEntry, Message, SessionState, Stat, UidlEntry};

/// A POP3 client connection.
pub struct Pop3Client {
    transport: Transport,
    greeting: String,
    state: SessionState,
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

impl Pop3Client {
    /// Connect to a POP3 server over plain TCP.
    ///
    /// The `timeout` duration is applied to every read operation for the lifetime
    /// of this connection. Use `DEFAULT_TIMEOUT` for a sensible default (30s).
    pub async fn connect(addr: impl tokio::net::ToSocketAddrs, timeout: Duration) -> Result<Self> {
        let mut transport = Transport::connect_plain(addr, timeout).await?;
        let greeting_line = transport.read_line().await?;
        let greeting_text = response::parse_status_line(&greeting_line)?;
        Ok(Pop3Client {
            transport,
            greeting: greeting_text.to_string(),
            state: SessionState::Connected,
        })
    }

    /// Connect with the default timeout (30 seconds).
    pub async fn connect_default(addr: impl tokio::net::ToSocketAddrs) -> Result<Self> {
        Self::connect(addr, crate::transport::DEFAULT_TIMEOUT).await
    }

    /// Connect to a POP3 server over TLS (typically port 995).
    ///
    /// The `hostname` is used for TLS server name verification (SNI).
    /// The `timeout` duration is applied to every read operation.
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
        })
    }

    /// Connect over TLS with the default timeout (30 seconds).
    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    pub async fn connect_tls_default(
        addr: impl tokio::net::ToSocketAddrs,
        hostname: &str,
    ) -> Result<Self> {
        Self::connect_tls(addr, hostname, crate::transport::DEFAULT_TIMEOUT).await
    }

    /// Returns the server greeting message.
    pub fn greeting(&self) -> &str {
        &self.greeting
    }

    /// Returns the current session state.
    pub fn state(&self) -> SessionState {
        self.state.clone()
    }

    /// Returns `true` if the connection is encrypted via TLS.
    pub fn is_encrypted(&self) -> bool {
        self.transport.is_encrypted()
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

    /// Authenticate with the server using USER/PASS.
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
        Ok(())
    }

    /// Get mailbox statistics (message count and total size).
    pub async fn stat(&mut self) -> Result<Stat> {
        self.require_auth()?;
        let text = self.send_and_check("STAT").await?;
        response::parse_stat(&text)
    }

    /// List messages. If `message_id` is `Some`, returns info for that message only.
    /// If `None`, returns info for all messages.
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

    /// Get unique IDs for messages. If `message_id` is `Some`, returns the UID for that
    /// message only. If `None`, returns UIDs for all messages.
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

    /// Retrieve a message by its message number.
    pub async fn retr(&mut self, message_id: u32) -> Result<Message> {
        self.require_auth()?;
        validate_message_id(message_id)?;
        self.send_and_check(&format!("RETR {message_id}")).await?;
        let data = self.transport.read_multiline().await?;
        Ok(Message { data })
    }

    /// Mark a message for deletion.
    pub async fn dele(&mut self, message_id: u32) -> Result<()> {
        self.require_auth()?;
        validate_message_id(message_id)?;
        self.send_and_check(&format!("DELE {message_id}")).await?;
        Ok(())
    }

    /// Reset the session — unmark all messages marked for deletion.
    pub async fn rset(&mut self) -> Result<()> {
        self.require_auth()?;
        self.send_and_check("RSET").await?;
        Ok(())
    }

    /// No-op, keeps the connection alive.
    pub async fn noop(&mut self) -> Result<()> {
        self.require_auth()?;
        self.send_and_check("NOOP").await?;
        Ok(())
    }

    /// End the session. Messages marked for deletion are removed.
    ///
    /// This method consumes the client, preventing any further use.
    /// If the caller drops the client without calling quit(), the TCP
    /// connection closes silently and pending DELE marks are NOT committed.
    pub async fn quit(self) -> Result<()> {
        let mut this = self;
        this.transport.send_command("QUIT").await?;
        let line = this.transport.read_line().await?;
        response::parse_status_line(&line)?;
        Ok(())
        // `this` is dropped here, TCP connection closes
    }

    /// Retrieve the headers and the first `lines` lines of a message body.
    pub async fn top(&mut self, message_id: u32, lines: u32) -> Result<Message> {
        self.require_auth()?;
        validate_message_id(message_id)?;
        self.send_and_check(&format!("TOP {message_id} {lines}"))
            .await?;
        let data = self.transport.read_multiline().await?;
        Ok(Message { data })
    }

    /// Query server capabilities.
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

#[cfg(test)]
fn build_test_client(mock: tokio_test::io::Mock) -> Pop3Client {
    let transport = Transport::mock(mock);
    Pop3Client {
        transport,
        greeting: String::new(),
        state: SessionState::Connected,
    }
}

#[cfg(test)]
fn build_authenticated_test_client(mock: tokio_test::io::Mock) -> Pop3Client {
    let transport = Transport::mock(mock);
    Pop3Client {
        transport,
        greeting: String::new(),
        state: SessionState::Authenticated,
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
}
