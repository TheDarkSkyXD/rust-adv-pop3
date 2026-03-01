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

        // USER command
        self.send_and_check(&format!("USER {username}"))?;

        // PASS command
        self.send_and_check(&format!("PASS {password}"))?;

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
}
