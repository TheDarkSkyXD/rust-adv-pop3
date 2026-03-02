use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{
    self, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter, ReadBuf,
};
use tokio::net::TcpStream;

use crate::error::{Pop3Error, Result};

/// Default read timeout for POP3 operations (30 seconds).
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Concrete stream types — enables unsplit() for STARTTLS upgrade.
pub(crate) enum InnerStream {
    Plain(TcpStream),
    #[cfg(feature = "rustls-tls")]
    RustlsTls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
    #[cfg(feature = "openssl-tls")]
    OpensslTls(tokio_openssl::SslStream<TcpStream>),
    #[cfg(test)]
    Mock(tokio_test::io::Mock),
    /// Temporary placeholder during STARTTLS upgrade (Plan 02). Never performs real I/O;
    /// this variant exists only transiently inside upgrade_in_place and is immediately
    /// replaced by the TLS variant before the method returns.
    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    #[allow(dead_code)]
    Upgrading,
}

impl AsyncRead for InnerStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            InnerStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "rustls-tls")]
            InnerStream::RustlsTls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
            #[cfg(feature = "openssl-tls")]
            InnerStream::OpensslTls(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(test)]
            InnerStream::Mock(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
            InnerStream::Upgrading => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stream is upgrading to TLS",
            ))),
        }
    }
}

impl AsyncWrite for InnerStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            InnerStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "rustls-tls")]
            InnerStream::RustlsTls(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "openssl-tls")]
            InnerStream::OpensslTls(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(test)]
            InnerStream::Mock(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
            InnerStream::Upgrading => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stream is upgrading to TLS",
            ))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            InnerStream::Plain(s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "rustls-tls")]
            InnerStream::RustlsTls(s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "openssl-tls")]
            InnerStream::OpensslTls(s) => Pin::new(s).poll_flush(cx),
            #[cfg(test)]
            InnerStream::Mock(s) => Pin::new(s).poll_flush(cx),
            #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
            InnerStream::Upgrading => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stream is upgrading to TLS",
            ))),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            InnerStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "rustls-tls")]
            InnerStream::RustlsTls(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "openssl-tls")]
            InnerStream::OpensslTls(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(test)]
            InnerStream::Mock(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
            InnerStream::Upgrading => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stream is upgrading to TLS",
            ))),
        }
    }
}

pub(crate) struct Transport {
    pub(crate) reader: BufReader<io::ReadHalf<InnerStream>>,
    pub(crate) writer: io::BufWriter<io::WriteHalf<InnerStream>>,
    pub(crate) timeout: Duration,
    encrypted: bool,
    is_closed: bool,
}

impl Transport {
    /// Connect over plain TCP.
    pub(crate) async fn connect_plain(
        addr: impl tokio::net::ToSocketAddrs,
        timeout: Duration,
    ) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let inner = InnerStream::Plain(stream);
        let (read_half, write_half) = io::split(inner);
        Ok(Transport {
            reader: BufReader::new(read_half),
            writer: BufWriter::new(write_half),
            timeout,
            encrypted: false,
            is_closed: false,
        })
    }

    /// Connect over TLS using rustls.
    #[cfg(feature = "rustls-tls")]
    pub(crate) async fn connect_tls(
        addr: impl tokio::net::ToSocketAddrs,
        hostname: &str,
        timeout: Duration,
    ) -> Result<Self> {
        use std::sync::Arc;
        use tokio_rustls::rustls::{ClientConfig, RootCertStore};
        use tokio_rustls::TlsConnector;

        // Validate hostname — use the pki_types re-export from tokio_rustls
        let server_name =
            tokio_rustls::rustls::pki_types::ServerName::try_from(hostname.to_owned())
                .map_err(|e| Pop3Error::InvalidDnsName(e.to_string()))?;

        // Load system trust store (rustls-native-certs 0.8 API)
        let native_certs = rustls_native_certs::load_native_certs();
        // native_certs.errors contains non-fatal cert load errors
        let mut root_store = RootCertStore::empty();
        for cert in native_certs.certs {
            root_store
                .add(cert)
                .map_err(|e| Pop3Error::Tls(e.to_string()))?;
        }

        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));

        let tcp_stream = TcpStream::connect(addr).await?;
        let tls_stream = connector
            .connect(server_name, tcp_stream)
            .await
            .map_err(|e| Pop3Error::Tls(e.to_string()))?;

        let inner = InnerStream::RustlsTls(Box::new(tls_stream));
        let (read_half, write_half) = io::split(inner);
        Ok(Transport {
            reader: BufReader::new(read_half),
            writer: BufWriter::new(write_half),
            timeout,
            encrypted: true,
            is_closed: false,
        })
    }

    /// Connect over TLS using OpenSSL.
    #[cfg(feature = "openssl-tls")]
    pub(crate) async fn connect_tls(
        addr: impl tokio::net::ToSocketAddrs,
        hostname: &str,
        timeout: Duration,
    ) -> Result<Self> {
        use openssl::ssl::{SslConnector, SslMethod};
        use tokio_openssl::SslStream;

        let connector = SslConnector::builder(SslMethod::tls())
            .map_err(|e| Pop3Error::Tls(e.to_string()))?
            .build();

        let ssl = connector
            .configure()
            .map_err(|e| Pop3Error::Tls(e.to_string()))?
            .into_ssl(hostname)
            .map_err(|e| Pop3Error::Tls(e.to_string()))?;

        let tcp_stream = TcpStream::connect(addr).await?;
        let mut tls_stream =
            SslStream::new(ssl, tcp_stream).map_err(|e| Pop3Error::Tls(e.to_string()))?;

        std::pin::Pin::new(&mut tls_stream)
            .connect()
            .await
            .map_err(|e| Pop3Error::Tls(e.to_string()))?;

        let inner = InnerStream::OpensslTls(tls_stream);
        let (read_half, write_half) = io::split(inner);
        Ok(Transport {
            reader: BufReader::new(read_half),
            writer: BufWriter::new(write_half),
            timeout,
            encrypted: true,
            is_closed: false,
        })
    }

    /// Stub for when no TLS feature is active.
    #[cfg(not(any(feature = "rustls-tls", feature = "openssl-tls")))]
    #[allow(dead_code)]
    pub(crate) async fn connect_tls(
        _addr: impl tokio::net::ToSocketAddrs,
        _hostname: &str,
        _timeout: Duration,
    ) -> Result<Self> {
        Err(Pop3Error::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "TLS not available — enable the `rustls-tls` or `openssl-tls` feature",
        )))
    }

    /// Returns `true` if the connection is encrypted via TLS.
    pub(crate) fn is_encrypted(&self) -> bool {
        self.encrypted
    }

    /// Returns `true` if the connection is known to be closed.
    ///
    /// This is set when `read_line()` receives EOF or when `quit()` completes.
    /// It is NOT a live TCP probe -- it tracks known-closed state only.
    pub(crate) fn is_closed(&self) -> bool {
        self.is_closed
    }

    /// Mark the transport as closed (called by quit() after successful QUIT).
    pub(crate) fn set_closed(&mut self) {
        self.is_closed = true;
    }

    /// Upgrade a plain TCP connection to TLS in-place (STARTTLS).
    ///
    /// Verifies the BufReader buffer is empty before upgrading. Uses the
    /// `Upgrading` placeholder variant to safely swap the halves, recovers the
    /// original `TcpStream` via `unsplit()`, performs a TLS handshake, then
    /// rebuilds `reader` and `writer` with the new TLS stream.
    #[cfg(any(feature = "rustls-tls", feature = "openssl-tls"))]
    #[allow(dead_code)] // Used in Plan 02 (STARTTLS) — not yet called from client.rs
    pub(crate) async fn upgrade_in_place(&mut self, hostname: &str) -> Result<()> {
        let pending = self.reader.buffer().len();
        if pending > 0 {
            return Err(Pop3Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected {} bytes in buffer before TLS upgrade", pending),
            )));
        }

        if self.encrypted {
            return Err(Pop3Error::Tls(
                "connection is already encrypted".to_string(),
            ));
        }

        let (placeholder_read, placeholder_write) = io::split(InnerStream::Upgrading);

        let old_reader = std::mem::replace(&mut self.reader, BufReader::new(placeholder_read));
        let old_writer = std::mem::replace(&mut self.writer, BufWriter::new(placeholder_write));

        let read_half = old_reader.into_inner();
        let write_half = old_writer.into_inner();
        let inner_stream = read_half.unsplit(write_half);

        let tcp_stream = match inner_stream {
            InnerStream::Plain(tcp) => tcp,
            _ => {
                return Err(Pop3Error::Tls(
                    "upgrade_in_place requires a plain TCP connection".to_string(),
                ));
            }
        };

        let tls_inner = Self::tls_handshake(tcp_stream, hostname).await?;

        let (new_read, new_write) = io::split(tls_inner);
        self.reader = BufReader::new(new_read);
        self.writer = BufWriter::new(new_write);
        self.encrypted = true;

        Ok(())
    }

    #[cfg(feature = "rustls-tls")]
    #[allow(dead_code)] // Used by upgrade_in_place (Plan 02)
    async fn tls_handshake(tcp_stream: TcpStream, hostname: &str) -> Result<InnerStream> {
        use std::sync::Arc;
        use tokio_rustls::rustls::{ClientConfig, RootCertStore};
        use tokio_rustls::TlsConnector;

        let server_name =
            tokio_rustls::rustls::pki_types::ServerName::try_from(hostname.to_owned())
                .map_err(|e| Pop3Error::InvalidDnsName(e.to_string()))?;

        let native_certs = rustls_native_certs::load_native_certs();
        let mut root_store = RootCertStore::empty();
        for cert in native_certs.certs {
            root_store
                .add(cert)
                .map_err(|e| Pop3Error::Tls(e.to_string()))?;
        }

        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));

        let tls_stream = connector
            .connect(server_name, tcp_stream)
            .await
            .map_err(|e| Pop3Error::Tls(e.to_string()))?;

        Ok(InnerStream::RustlsTls(Box::new(tls_stream)))
    }

    #[cfg(feature = "openssl-tls")]
    #[allow(dead_code)] // Used by upgrade_in_place (Plan 02)
    async fn tls_handshake(tcp_stream: TcpStream, hostname: &str) -> Result<InnerStream> {
        use openssl::ssl::{SslConnector, SslMethod};
        use tokio_openssl::SslStream;

        let connector = SslConnector::builder(SslMethod::tls())
            .map_err(|e| Pop3Error::Tls(e.to_string()))?
            .build();

        let ssl = connector
            .configure()
            .map_err(|e| Pop3Error::Tls(e.to_string()))?
            .into_ssl(hostname)
            .map_err(|e| Pop3Error::Tls(e.to_string()))?;

        let mut tls_stream =
            SslStream::new(ssl, tcp_stream).map_err(|e| Pop3Error::Tls(e.to_string()))?;

        std::pin::Pin::new(&mut tls_stream)
            .connect()
            .await
            .map_err(|e| Pop3Error::Tls(e.to_string()))?;

        Ok(InnerStream::OpensslTls(tls_stream))
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
            self.is_closed = true;
            return Err(Pop3Error::ConnectionClosed);
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
        let inner = InnerStream::Mock(mock);
        let (read_half, write_half) = io::split(inner);
        Transport {
            reader: BufReader::new(read_half),
            writer: BufWriter::new(write_half),
            timeout: Duration::from_secs(30),
            encrypted: false,
            is_closed: false,
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
        assert!(matches!(
            result,
            Err(crate::error::Pop3Error::ConnectionClosed)
        ));
    }

    #[tokio::test]
    async fn is_closed_false_initially() {
        let mock = Builder::new().build();
        let transport = Transport::mock(mock);
        assert!(!transport.is_closed());
    }

    #[tokio::test]
    async fn is_closed_true_after_eof() {
        let mock = Builder::new().build(); // empty -- EOF on read
        let mut transport = Transport::mock(mock);
        let result = transport.read_line().await;
        assert!(matches!(
            result,
            Err(crate::error::Pop3Error::ConnectionClosed)
        ));
        assert!(transport.is_closed());
    }

    #[tokio::test]
    async fn send_command_works_with_bufwriter() {
        // Verify that send_command still works after BufWriter upgrade.
        // The existing send_command_writes_crlf test also validates this,
        // but this test explicitly checks flush behavior.
        let mock = Builder::new().write(b"NOOP\r\n").build();
        let mut transport = Transport::mock(mock);
        transport.send_command("NOOP").await.unwrap();
    }

    #[tokio::test]
    async fn read_line_returns_line() {
        let mock = Builder::new().read(b"+OK ready\r\n").build();
        let mut transport = Transport::mock(mock);
        let line = transport.read_line().await.unwrap();
        assert_eq!(line, "+OK ready\r\n");
    }

    #[tokio::test]
    async fn is_encrypted_false_for_mock() {
        let mock = Builder::new().build();
        let transport = Transport::mock(mock);
        assert!(!transport.is_encrypted());
    }
}
