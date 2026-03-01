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
        self.transport
            .send_command(&format!("USER {username}"))?;
        let line = self.transport.read_line()?;
        response::parse_status_line(&line)?;

        // PASS command
        self.transport
            .send_command(&format!("PASS {password}"))?;
        let line = self.transport.read_line()?;
        response::parse_status_line(&line)?;

        // Only set authenticated after both commands succeed
        self.authenticated = true;
        Ok(())
    }

    /// Get mailbox statistics (message count and total size).
    pub fn stat(&mut self) -> Result<Stat> {
        self.require_auth()?;
        self.transport.send_command("STAT")?;
        let line = self.transport.read_line()?;
        let text = response::parse_status_line(&line)?;
        response::parse_stat(text)
    }

    /// List messages. If `message_id` is `Some`, returns info for that message only.
    /// If `None`, returns info for all messages.
    pub fn list(&mut self, message_id: Option<u32>) -> Result<Vec<ListEntry>> {
        self.require_auth()?;
        match message_id {
            Some(id) => {
                self.transport
                    .send_command(&format!("LIST {id}"))?;
                let line = self.transport.read_line()?;
                let text = response::parse_status_line(&line)?;
                let entry = response::parse_list_single(text)?;
                Ok(vec![entry])
            }
            None => {
                self.transport.send_command("LIST")?;
                let line = self.transport.read_line()?;
                response::parse_status_line(&line)?;
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
                self.transport
                    .send_command(&format!("UIDL {id}"))?;
                let line = self.transport.read_line()?;
                let text = response::parse_status_line(&line)?;
                let entry = response::parse_uidl_single(text)?;
                Ok(vec![entry])
            }
            None => {
                self.transport.send_command("UIDL")?;
                let line = self.transport.read_line()?;
                response::parse_status_line(&line)?;
                let body = self.transport.read_multiline()?;
                response::parse_uidl_multi(&body)
            }
        }
    }

    /// Retrieve a message by its message number.
    pub fn retr(&mut self, message_id: u32) -> Result<Message> {
        self.require_auth()?;
        self.transport
            .send_command(&format!("RETR {message_id}"))?;
        let line = self.transport.read_line()?;
        response::parse_status_line(&line)?;
        let data = self.transport.read_multiline()?;
        Ok(Message { data })
    }

    /// Mark a message for deletion.
    pub fn dele(&mut self, message_id: u32) -> Result<()> {
        self.require_auth()?;
        self.transport
            .send_command(&format!("DELE {message_id}"))?;
        let line = self.transport.read_line()?;
        response::parse_status_line(&line)?;
        Ok(())
    }

    /// Reset the session — unmark all messages marked for deletion.
    pub fn rset(&mut self) -> Result<()> {
        self.require_auth()?;
        self.transport.send_command("RSET")?;
        let line = self.transport.read_line()?;
        response::parse_status_line(&line)?;
        Ok(())
    }

    /// No-op, keeps the connection alive.
    pub fn noop(&mut self) -> Result<()> {
        self.require_auth()?;
        self.transport.send_command("NOOP")?;
        let line = self.transport.read_line()?;
        response::parse_status_line(&line)?;
        Ok(())
    }

    /// End the session. Messages marked for deletion are removed.
    pub fn quit(&mut self) -> Result<()> {
        self.transport.send_command("QUIT")?;
        let line = self.transport.read_line()?;
        response::parse_status_line(&line)?;
        self.authenticated = false;
        Ok(())
    }

    /// Retrieve the headers and the first `lines` lines of a message body.
    pub fn top(&mut self, message_id: u32, lines: u32) -> Result<Message> {
        self.require_auth()?;
        self.transport
            .send_command(&format!("TOP {message_id} {lines}"))?;
        let line = self.transport.read_line()?;
        response::parse_status_line(&line)?;
        let data = self.transport.read_multiline()?;
        Ok(Message { data })
    }

    /// Query server capabilities.
    pub fn capa(&mut self) -> Result<Vec<Capability>> {
        self.transport.send_command("CAPA")?;
        let line = self.transport.read_line()?;
        response::parse_status_line(&line)?;
        let body = self.transport.read_multiline()?;
        Ok(response::parse_capa(&body))
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
}
