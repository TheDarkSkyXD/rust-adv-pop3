use std::net::ToSocketAddrs;

use crate::error::{Pop3Error, Result};
use crate::response;
use crate::transport::Transport;
use crate::types::{Capability, ListEntry, Message, Stat, UidlEntry};

/// How to connect to the POP3 server.
pub enum TlsMode {
    /// Plain TCP (no encryption).
    Plain,
    /// TLS with the given hostname for certificate verification.
    Tls(String),
}

/// A POP3 client connection.
pub struct Pop3Client {
    transport: Transport,
    greeting: String,
    authenticated: bool,
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
    /// Connect to a POP3 server.
    ///
    /// Use `TlsMode::Plain` for an unencrypted connection, or
    /// `TlsMode::Tls(hostname)` for a TLS-encrypted connection.
    pub fn connect(addr: impl ToSocketAddrs, tls: TlsMode) -> Result<Self> {
        let mut transport = match tls {
            TlsMode::Plain => Transport::connect_plain(addr)?,
            TlsMode::Tls(ref hostname) => Transport::connect_tls(addr, hostname)?,
        };

        let greeting_line = transport.read_line()?;
        let greeting_text = response::parse_status_line(&greeting_line)?;

        Ok(Pop3Client {
            transport,
            greeting: greeting_text.to_string(),
            authenticated: false,
        })
    }

    /// Returns the server greeting message.
    pub fn greeting(&self) -> &str {
        &self.greeting
    }

    /// Authenticate with the server using USER/PASS.
    pub fn login(&mut self, username: &str, password: &str) -> Result<()> {
        check_no_crlf(username)?;
        check_no_crlf(password)?;

        // USER command — auth failure if rejected
        self.send_and_check(&format!("USER {username}"))
            .map_err(|e| match e {
                Pop3Error::ServerError(msg) => Pop3Error::AuthFailed(msg),
                other => other,
            })?;

        // PASS command — auth failure if rejected
        self.send_and_check(&format!("PASS {password}"))
            .map_err(|e| match e {
                Pop3Error::ServerError(msg) => Pop3Error::AuthFailed(msg),
                other => other,
            })?;

        // Only set authenticated after both commands succeed
        self.authenticated = true;
        Ok(())
    }

    /// Get mailbox statistics (message count and total size).
    pub fn stat(&mut self) -> Result<Stat> {
        self.require_auth()?;
        let text = self.send_and_check("STAT")?;
        response::parse_stat(&text)
    }

    /// List messages. If `message_id` is `Some`, returns info for that message only.
    /// If `None`, returns info for all messages.
    pub fn list(&mut self, message_id: Option<u32>) -> Result<Vec<ListEntry>> {
        self.require_auth()?;
        match message_id {
            Some(id) => {
                validate_message_id(id)?;
                let text = self.send_and_check(&format!("LIST {id}"))?;
                let entry = response::parse_list_single(&text)?;
                Ok(vec![entry])
            }
            None => {
                self.send_and_check("LIST")?;
                let body = self.transport.read_multiline()?;
                response::parse_list_multi(&body)
            }
        }
    }

    /// Get unique IDs for messages. If `message_id` is `Some`, returns the UID for that
    /// message only. If `None`, returns UIDs for all messages.
    pub fn uidl(&mut self, message_id: Option<u32>) -> Result<Vec<UidlEntry>> {
        self.require_auth()?;
        match message_id {
            Some(id) => {
                validate_message_id(id)?;
                let text = self.send_and_check(&format!("UIDL {id}"))?;
                let entry = response::parse_uidl_single(&text)?;
                Ok(vec![entry])
            }
            None => {
                self.send_and_check("UIDL")?;
                let body = self.transport.read_multiline()?;
                response::parse_uidl_multi(&body)
            }
        }
    }

    /// Retrieve a message by its message number.
    pub fn retr(&mut self, message_id: u32) -> Result<Message> {
        self.require_auth()?;
        validate_message_id(message_id)?;
        self.send_and_check(&format!("RETR {message_id}"))?;
        let data = self.transport.read_multiline()?;
        Ok(Message { data })
    }

    /// Mark a message for deletion.
    pub fn dele(&mut self, message_id: u32) -> Result<()> {
        self.require_auth()?;
        validate_message_id(message_id)?;
        self.send_and_check(&format!("DELE {message_id}"))?;
        Ok(())
    }

    /// Reset the session — unmark all messages marked for deletion.
    pub fn rset(&mut self) -> Result<()> {
        self.require_auth()?;
        self.send_and_check("RSET")?;
        Ok(())
    }

    /// No-op, keeps the connection alive.
    pub fn noop(&mut self) -> Result<()> {
        self.require_auth()?;
        self.send_and_check("NOOP")?;
        Ok(())
    }

    /// End the session. Messages marked for deletion are removed.
    pub fn quit(&mut self) -> Result<()> {
        self.send_and_check("QUIT")?;
        self.authenticated = false;
        Ok(())
    }

    /// Retrieve the headers and the first `lines` lines of a message body.
    pub fn top(&mut self, message_id: u32, lines: u32) -> Result<Message> {
        self.require_auth()?;
        validate_message_id(message_id)?;
        self.send_and_check(&format!("TOP {message_id} {lines}"))?;
        let data = self.transport.read_multiline()?;
        Ok(Message { data })
    }

    /// Query server capabilities.
    pub fn capa(&mut self) -> Result<Vec<Capability>> {
        self.send_and_check("CAPA")?;
        let body = self.transport.read_multiline()?;
        Ok(response::parse_capa(&body))
    }

    /// Send a command to the server, read the status line, and return the status text.
    fn send_and_check(&mut self, cmd: &str) -> Result<String> {
        self.transport.send_command(cmd)?;
        let line = self.transport.read_line()?;
        let text = response::parse_status_line(&line)?;
        Ok(text.to_string())
    }

    fn require_auth(&self) -> Result<()> {
        if !self.authenticated {
            Err(Pop3Error::NotAuthenticated)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
fn build_test_client(
    server_bytes: &[u8],
) -> (Pop3Client, std::rc::Rc<std::cell::RefCell<Vec<u8>>>) {
    let (transport, writer) = Transport::mock(server_bytes);
    let client = Pop3Client {
        transport,
        greeting: String::new(),
        authenticated: false,
    };
    (client, writer)
}

#[cfg(test)]
fn build_authenticated_test_client(
    server_bytes: &[u8],
) -> (Pop3Client, std::rc::Rc<std::cell::RefCell<Vec<u8>>>) {
    let (mut client, writer) = build_test_client(server_bytes);
    client.authenticated = true;
    (client, writer)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // --- FIX-01: rset() must send "RSET\r\n", not "RETR\r\n" ---

    #[test]
    fn rset_sends_correct_command_fix01() {
        let (mut client, writer) = build_authenticated_test_client(b"+OK\r\n");
        client.rset().unwrap();
        let sent = writer.borrow();
        assert_eq!(&*sent, b"RSET\r\n", "FIX-01: rset must send RSET, not RETR");
    }

    // --- FIX-02: noop() must send "NOOP\r\n" (uppercase) ---

    #[test]
    fn noop_sends_correct_command_fix02() {
        let (mut client, writer) = build_authenticated_test_client(b"+OK\r\n");
        client.noop().unwrap();
        let sent = writer.borrow();
        assert_eq!(
            &*sent, b"NOOP\r\n",
            "FIX-02: noop must send NOOP (uppercase)"
        );
    }

    // --- FIX-03: login() sets authenticated only after both USER and PASS succeed ---

    #[test]
    fn login_sets_authenticated_after_pass_ok_fix03() {
        let (mut client, _) = build_test_client(b"+OK\r\n+OK logged in\r\n");
        client.login("user", "pass").unwrap();
        assert!(client.authenticated);
    }

    #[test]
    fn login_not_authenticated_when_pass_fails_fix03() {
        let (mut client, _) = build_test_client(b"+OK\r\n-ERR invalid password\r\n");
        let result = client.login("user", "wrongpass");
        assert!(result.is_err());
        assert!(
            !client.authenticated,
            "FIX-03: must NOT set authenticated when PASS returns -ERR"
        );
        match result.unwrap_err() {
            Pop3Error::AuthFailed(msg) => assert_eq!(msg, "invalid password"),
            other => panic!("expected AuthFailed, got: {other:?}"),
        }
    }

    // --- FIX-04: list round-trip validates parse_list_single via mock I/O ---

    #[test]
    fn list_single_round_trip_fix04() {
        let (mut client, writer) = build_authenticated_test_client(b"+OK 1 1234\r\n");
        let entries = client.list(Some(1)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].size, 1234);
        let sent = writer.borrow();
        assert_eq!(
            &*sent, b"LIST 1\r\n",
            "FIX-04: LIST 1 wire command must be correct"
        );
    }

    // --- Login happy and error paths ---

    #[test]
    fn login_happy_path() {
        let (mut client, _) = build_test_client(b"+OK\r\n+OK logged in\r\n");
        let result = client.login("alice", "secret");
        assert!(result.is_ok());
        assert!(client.authenticated);
    }

    #[test]
    fn login_user_command_rejected_returns_auth_failed() {
        let (mut client, _) = build_test_client(b"-ERR no such user\r\n");
        let result = client.login("nobody", "pass");
        assert!(result.is_err());
        assert!(!client.authenticated);
        match result.unwrap_err() {
            Pop3Error::AuthFailed(msg) => assert!(msg.contains("no such user")),
            other => panic!("expected AuthFailed, got: {other:?}"),
        }
    }

    #[test]
    fn login_rejects_crlf_in_username() {
        let (mut client, _) = build_test_client(b"");
        let result = client.login("user\r\nINJECT", "pass");
        assert!(matches!(result.unwrap_err(), Pop3Error::InvalidInput));
    }

    #[test]
    fn login_rejects_crlf_in_password() {
        let (mut client, _) = build_test_client(b"");
        let result = client.login("user", "pass\r\nINJECT");
        assert!(matches!(result.unwrap_err(), Pop3Error::InvalidInput));
    }

    // --- stat happy and error paths ---

    #[test]
    fn stat_happy_path() {
        let (mut client, writer) = build_authenticated_test_client(b"+OK 5 12345\r\n");
        let stat = client.stat().unwrap();
        assert_eq!(stat.message_count, 5);
        assert_eq!(stat.mailbox_size, 12345);
        assert_eq!(&*writer.borrow(), b"STAT\r\n");
    }

    #[test]
    fn stat_server_error() {
        let (mut client, _) = build_authenticated_test_client(b"-ERR mailbox locked\r\n");
        let result = client.stat();
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(msg) => assert!(msg.contains("mailbox locked")),
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    // --- list single happy and error paths ---

    #[test]
    fn list_single_happy_path() {
        let (mut client, writer) = build_authenticated_test_client(b"+OK 1 1234\r\n");
        let entries = client.list(Some(1)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].size, 1234);
        assert_eq!(&*writer.borrow(), b"LIST 1\r\n");
    }

    #[test]
    fn list_single_server_error() {
        let (mut client, _) = build_authenticated_test_client(b"-ERR no such message\r\n");
        let result = client.list(Some(99));
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(msg) => assert!(msg.contains("no such message")),
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    #[test]
    fn list_all_happy_path() {
        let server_response = b"+OK\r\n1 100\r\n2 200\r\n.\r\n";
        let (mut client, writer) = build_authenticated_test_client(server_response);
        let entries = client.list(None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].size, 100);
        assert_eq!(entries[1].message_id, 2);
        assert_eq!(entries[1].size, 200);
        assert_eq!(&*writer.borrow(), b"LIST\r\n");
    }

    // --- retr happy, error, and dot-unstuffing paths ---

    #[test]
    fn retr_happy_path() {
        let server = b"+OK\r\nSubject: Test\r\n\r\nBody line 1\r\n.\r\n";
        let (mut client, writer) = build_authenticated_test_client(server);
        let msg = client.retr(1).unwrap();
        assert!(msg.data.contains("Subject: Test"));
        assert!(msg.data.contains("Body line 1"));
        assert_eq!(&*writer.borrow(), b"RETR 1\r\n");
    }

    #[test]
    fn retr_server_error() {
        let (mut client, _) = build_authenticated_test_client(b"-ERR no such message\r\n");
        let result = client.retr(1);
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(_) => {}
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    #[test]
    fn retr_dot_unstuffing() {
        // Server sends a line starting with ".." which should become "."
        let server = b"+OK\r\n..This had a leading dot\r\nNormal line\r\n.\r\n";
        let (mut client, _) = build_authenticated_test_client(server);
        let msg = client.retr(1).unwrap();
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

    #[test]
    fn dele_happy_path() {
        let (mut client, writer) = build_authenticated_test_client(b"+OK message deleted\r\n");
        client.dele(1).unwrap();
        assert_eq!(&*writer.borrow(), b"DELE 1\r\n");
    }

    #[test]
    fn dele_server_error() {
        let (mut client, _) = build_authenticated_test_client(b"-ERR message locked\r\n");
        let result = client.dele(1);
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(_) => {}
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    // --- uidl single, all, and error paths ---

    #[test]
    fn uidl_single_happy_path() {
        let (mut client, writer) = build_authenticated_test_client(b"+OK 1 abc123\r\n");
        let entries = client.uidl(Some(1)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].unique_id, "abc123");
        assert_eq!(&*writer.borrow(), b"UIDL 1\r\n");
    }

    #[test]
    fn uidl_all_happy_path() {
        let server = b"+OK\r\n1 uid-a\r\n2 uid-b\r\n.\r\n";
        let (mut client, writer) = build_authenticated_test_client(server);
        let entries = client.uidl(None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message_id, 1);
        assert_eq!(entries[0].unique_id, "uid-a");
        assert_eq!(entries[1].message_id, 2);
        assert_eq!(entries[1].unique_id, "uid-b");
        assert_eq!(&*writer.borrow(), b"UIDL\r\n");
    }

    #[test]
    fn uidl_server_error() {
        let (mut client, _) = build_authenticated_test_client(b"-ERR\r\n");
        let result = client.uidl(Some(1));
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(_) => {}
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    // --- quit happy path and authenticated flag reset ---

    #[test]
    fn quit_happy_path() {
        let (mut client, writer) = build_authenticated_test_client(b"+OK goodbye\r\n");
        client.quit().unwrap();
        assert_eq!(&*writer.borrow(), b"QUIT\r\n");
    }

    #[test]
    fn quit_resets_authenticated() {
        let (mut client, _) = build_authenticated_test_client(b"+OK goodbye\r\n");
        assert!(client.authenticated);
        client.quit().unwrap();
        assert!(!client.authenticated, "quit must set authenticated=false");
    }

    // --- capa happy and error paths ---

    #[test]
    fn capa_happy_path() {
        let server = b"+OK\r\nTOP\r\nUIDL\r\nSASL PLAIN\r\n.\r\n";
        let (mut client, writer) = build_test_client(server);
        let caps = client.capa().unwrap();
        assert_eq!(caps.len(), 3);
        assert_eq!(&*writer.borrow(), b"CAPA\r\n");
    }

    #[test]
    fn capa_server_error() {
        let (mut client, _) = build_test_client(b"-ERR not supported\r\n");
        let result = client.capa();
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(_) => {}
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    // --- top happy and error paths ---

    #[test]
    fn top_happy_path() {
        let server = b"+OK\r\nSubject: Test\r\n\r\nFirst line\r\n.\r\n";
        let (mut client, writer) = build_authenticated_test_client(server);
        let msg = client.top(1, 5).unwrap();
        assert!(msg.data.contains("Subject: Test"));
        assert!(msg.data.contains("First line"));
        assert_eq!(&*writer.borrow(), b"TOP 1 5\r\n");
    }

    #[test]
    fn top_server_error() {
        let (mut client, _) = build_authenticated_test_client(b"-ERR no such message\r\n");
        let result = client.top(1, 5);
        assert!(result.is_err());
        match result.unwrap_err() {
            Pop3Error::ServerError(_) => {}
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }

    // --- Not-authenticated guard: commands require authentication ---

    #[test]
    fn commands_require_authentication() {
        let (mut client, _) = build_test_client(b"");
        assert!(matches!(client.stat(), Err(Pop3Error::NotAuthenticated)));

        // Re-create because the mock buffer is consumed
        let (mut client, _) = build_test_client(b"");
        assert!(matches!(
            client.list(None),
            Err(Pop3Error::NotAuthenticated)
        ));

        let (mut client, _) = build_test_client(b"");
        assert!(matches!(client.retr(1), Err(Pop3Error::NotAuthenticated)));

        let (mut client, _) = build_test_client(b"");
        assert!(matches!(client.dele(1), Err(Pop3Error::NotAuthenticated)));
    }
}
