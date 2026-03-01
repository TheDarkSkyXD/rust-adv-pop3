use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};

use crate::error::{Pop3Error, Result};

enum Stream {
    Plain(BufReader<TcpStream>),
    Tls(Box<BufReader<StreamOwned<ClientConnection, TcpStream>>>),
}

pub(crate) struct Transport {
    stream: Stream,
}

impl Transport {
    /// Connect over plain TCP.
    pub(crate) fn connect_plain(addr: impl ToSocketAddrs) -> Result<Self> {
        let tcp = TcpStream::connect(addr)?;
        Ok(Transport {
            stream: Stream::Plain(BufReader::new(tcp)),
        })
    }

    /// Connect over TLS using rustls with native certificate roots.
    pub(crate) fn connect_tls(addr: impl ToSocketAddrs, hostname: &str) -> Result<Self> {
        let tcp = TcpStream::connect(addr)?;

        let certs = rustls_native_certs::load_native_certs();
        let mut root_store = rustls::RootCertStore::empty();
        for cert in certs.certs {
            let _ = root_store.add(cert);
        }

        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let server_name: ServerName<'static> = hostname
            .to_string()
            .try_into()
            .map_err(|_| Pop3Error::InvalidDnsName(hostname.to_string()))?;

        let conn = ClientConnection::new(Arc::new(config), server_name)?;
        let tls_stream = StreamOwned::new(conn, tcp);
        Ok(Transport {
            stream: Stream::Tls(Box::new(BufReader::new(tls_stream))),
        })
    }

    /// Send a command to the server (appends CRLF).
    pub(crate) fn send_command(&mut self, cmd: &str) -> Result<()> {
        match &mut self.stream {
            Stream::Plain(ref mut reader) => {
                let stream = reader.get_mut();
                stream.write_all(cmd.as_bytes())?;
                stream.write_all(b"\r\n")?;
                stream.flush()?;
            }
            Stream::Tls(ref mut reader) => {
                let stream = reader.get_mut();
                stream.write_all(cmd.as_bytes())?;
                stream.write_all(b"\r\n")?;
                stream.flush()?;
            }
        }
        Ok(())
    }

    /// Read a single CRLF-terminated line from the server.
    pub(crate) fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        match &mut self.stream {
            Stream::Plain(ref mut reader) => {
                reader.read_line(&mut line)?;
            }
            Stream::Tls(ref mut reader) => {
                reader.read_line(&mut line)?;
            }
        }
        if line.is_empty() {
            return Err(Pop3Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed",
            )));
        }
        Ok(line)
    }

    /// Read a multi-line response body (after the status line) until the dot terminator.
    /// Applies dot-unstuffing per RFC 1939: lines starting with `..` have the leading dot removed.
    pub(crate) fn read_multiline(&mut self) -> Result<String> {
        let mut body = String::new();
        loop {
            let line = self.read_line()?;
            let trimmed = line.trim_end_matches("\r\n").trim_end_matches('\n');
            if trimmed == "." {
                break;
            }
            // Dot-unstuffing: if a line starts with "..", remove the leading dot
            let content = if let Some(rest) = trimmed.strip_prefix("..") {
                format!(".{rest}")
            } else {
                trimmed.to_string()
            };
            body.push_str(&content);
            body.push('\n');
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_dot_unstuffing_logic() {
        // Test the dot-unstuffing logic in isolation
        let lines = vec!["..This starts with a dot", "Normal line", "."];
        let mut body = String::new();
        for line in &lines {
            let trimmed = line.trim_end_matches("\r\n").trim_end_matches('\n');
            if trimmed == "." {
                break;
            }
            let content = if let Some(rest) = trimmed.strip_prefix("..") {
                format!(".{rest}")
            } else {
                trimmed.to_string()
            };
            body.push_str(&content);
            body.push('\n');
        }
        assert_eq!(body, ".This starts with a dot\nNormal line\n");
    }
}
