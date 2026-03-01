use std::time::Duration;

use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::error::{Pop3Error, Result};

/// Default read timeout for POP3 operations (30 seconds).
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct Transport {
    reader: BufReader<Box<dyn io::AsyncRead + Unpin + Send>>,
    writer: Box<dyn io::AsyncWrite + Unpin + Send>,
    timeout: Duration,
}

impl Transport {
    /// Connect over plain TCP.
    pub(crate) async fn connect_plain(
        addr: impl tokio::net::ToSocketAddrs,
        timeout: Duration,
    ) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let (read_half, write_half) = io::split(stream);
        Ok(Transport {
            reader: BufReader::new(Box::new(read_half)),
            writer: Box::new(write_half),
            timeout,
        })
    }

    /// Connect over TLS.
    ///
    /// Not yet supported in the async rewrite — TLS will be added in Phase 3.
    // Phase 3 will call this from the TLS connection method; kept here as a stub.
    #[allow(dead_code)]
    pub(crate) async fn connect_tls(
        _addr: impl tokio::net::ToSocketAddrs,
        _hostname: &str,
        _timeout: Duration,
    ) -> Result<Self> {
        Err(Pop3Error::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "TLS not yet supported in async mode — use Phase 3",
        )))
    }

    /// Send a command to the server (appends CRLF).
    pub(crate) async fn send_command(&mut self, cmd: &str) -> Result<()> {
        self.writer.write_all(cmd.as_bytes()).await?;
        self.writer.write_all(b"\r\n").await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Read a single CRLF-terminated line from the server.
    ///
    /// Returns `Pop3Error::Timeout` if the read does not complete within the
    /// configured timeout duration, and `Pop3Error::Io` on unexpected EOF.
    pub(crate) async fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        let n = tokio::time::timeout(self.timeout, self.reader.read_line(&mut line))
            .await
            .map_err(|_| Pop3Error::Timeout)??;
        if n == 0 {
            return Err(Pop3Error::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed",
            )));
        }
        Ok(line)
    }

    /// Read a multi-line response body (after the status line) until the dot terminator.
    ///
    /// Applies dot-unstuffing per RFC 1939: lines starting with `..` have the
    /// leading dot removed.
    pub(crate) async fn read_multiline(&mut self) -> Result<String> {
        let mut body = String::new();
        loop {
            let line = self.read_line().await?;
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
impl Transport {
    /// Create a mock transport for testing using `tokio_test::io::Builder`.
    ///
    /// Write expectations are baked into the mock — the builder panics if the
    /// actual write differs, so there is no need to return a separate writer handle.
    pub(crate) fn mock(mock: tokio_test::io::Mock) -> Self {
        let (read_half, write_half) = io::split(mock);
        Transport {
            reader: BufReader::new(Box::new(read_half)),
            writer: Box::new(write_half),
            timeout: Duration::from_secs(30),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_test::io::Builder;

    #[tokio::test]
    async fn dot_unstuffing_via_transport() {
        let mock = Builder::new()
            .read(b"..This had a leading dot\r\n")
            .read(b"Normal line\r\n")
            .read(b".\r\n")
            .build();
        let mut transport = Transport::mock(mock);
        let body = transport.read_multiline().await.unwrap();
        assert!(body.contains(".This had a leading dot"));
        assert!(!body.contains("..This"));
        assert!(body.contains("Normal line"));
    }

    #[tokio::test]
    async fn send_command_writes_crlf() {
        let mock = Builder::new().write(b"STAT\r\n").build();
        let mut transport = Transport::mock(mock);
        transport.send_command("STAT").await.unwrap();
        // tokio_test mock validates the write automatically
    }

    #[tokio::test]
    async fn read_line_returns_eof_error() {
        let mock = Builder::new().build(); // empty — EOF
        let mut transport = Transport::mock(mock);
        let result = transport.read_line().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_line_returns_line() {
        let mock = Builder::new().read(b"+OK ready\r\n").build();
        let mut transport = Transport::mock(mock);
        let line = transport.read_line().await.unwrap();
        assert_eq!(line, "+OK ready\r\n");
    }
}
