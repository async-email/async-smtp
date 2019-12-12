//! A trait to represent a stream

use std::fmt;
use std::time::Duration;

use async_std::io::{self, ErrorKind, Read, Write};
use async_std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpStream};
use async_std::pin::Pin;
use async_std::sync::Arc;
use async_std::task::{Context, Poll};
use async_tls::{client::TlsStream, TlsConnector};
use async_trait::async_trait;
use pin_project::{pin_project, project};
use rustls::{ClientConfig, ProtocolVersion};

use crate::smtp::client::mock::MockStream;

/// Parameters to use for secure clients
#[derive(Clone)]
pub struct ClientTlsParameters {
    /// A connector from `native-tls`
    pub connector: ClientConfig,
    /// The domain to send during the TLS handshake
    pub domain: String,
}

impl fmt::Debug for ClientTlsParameters {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("ClientTlsParameters")
            .field("connector", &"ClientConfig")
            .field("domain", &self.domain)
            .finish()
    }
}

impl ClientTlsParameters {
    /// Creates a `ClientTlsParameters`
    pub fn new(domain: String, connector: ClientConfig) -> ClientTlsParameters {
        ClientTlsParameters { connector, domain }
    }
}

/// Accepted protocols by default.
/// This removes TLS 1.0 and 1.1 compared to tls-native defaults.
pub const DEFAULT_TLS_MIN_PROTOCOL: ProtocolVersion = ProtocolVersion::TLSv1_2;

/// Represents the different types of underlying network streams
#[pin_project]
#[allow(missing_debug_implementations)]
pub enum NetworkStream {
    /// Plain TCP stream
    Tcp(#[pin] TcpStream),
    /// Encrypted TCP stream
    Tls(#[pin] TlsStream<TcpStream>),
    /// Mock stream
    Mock(#[pin] MockStream),
}

impl NetworkStream {
    /// Returns peer's address
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        match *self {
            NetworkStream::Tcp(ref s) => s.peer_addr(),
            NetworkStream::Tls(ref s) => s.get_ref().peer_addr(),
            NetworkStream::Mock(_) => Ok(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(127, 0, 0, 1),
                80,
            ))),
        }
    }

    /// Shutdowns the connection
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        match *self {
            NetworkStream::Tcp(ref s) => s.shutdown(how),
            NetworkStream::Tls(ref s) => s.get_ref().shutdown(how),
            NetworkStream::Mock(_) => Ok(()),
        }
    }
}

impl Read for NetworkStream {
    #[project]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        #[project]
        match self.project() {
            NetworkStream::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_read(cx, buf)
            }
            NetworkStream::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_read(cx, buf)
            }
            NetworkStream::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_read(cx, buf)
            }
        }
    }
}

impl Write for NetworkStream {
    #[project]
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        #[project]
        match self.project() {
            NetworkStream::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_write(cx, buf)
            }
            NetworkStream::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_write(cx, buf)
            }
            NetworkStream::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_write(cx, buf)
            }
        }
    }

    #[project]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        #[project]
        match self.project() {
            NetworkStream::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_flush(cx)
            }
            NetworkStream::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_flush(cx)
            }
            NetworkStream::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_flush(cx)
            }
        }
    }

    #[project]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        #[project]
        match self.project() {
            NetworkStream::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_close(cx)
            }
            NetworkStream::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_close(cx)
            }
            NetworkStream::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_close(cx)
            }
        }
    }
}

/// A trait for the concept of opening a stream
#[async_trait]
pub trait Connector: Sized {
    /// Opens a connection to the given IP socket
    async fn connect(
        addr: &SocketAddr,
        timeout: Option<Duration>,
        tls_parameters: Option<&ClientTlsParameters>,
    ) -> io::Result<Self>;
    /// Upgrades to TLS connection
    async fn upgrade_tls(self, tls_parameters: &ClientTlsParameters) -> io::Result<Self>;

    /// Is the NetworkStream encrypted
    fn is_encrypted(&self) -> bool;
}

#[async_trait]
impl Connector for NetworkStream {
    async fn connect(
        addr: &SocketAddr,
        timeout: Option<Duration>,
        tls_parameters: Option<&ClientTlsParameters>,
    ) -> io::Result<NetworkStream> {
        let tcp_stream = match timeout {
            Some(duration) => {
                io::timeout(duration, async move { TcpStream::connect(addr).await }).await?
            }
            None => TcpStream::connect(addr).await?,
        };

        match tls_parameters {
            Some(context) => {
                let connector: TlsConnector = Arc::new(context.connector.clone()).into();
                connector
                    .connect(&context.domain, tcp_stream)
                    .await
                    .map(NetworkStream::Tls)
                    .map_err(|e| io::Error::new(ErrorKind::Other, e))
            }
            None => Ok(NetworkStream::Tcp(tcp_stream)),
        }
    }

    async fn upgrade_tls(self, tls_parameters: &ClientTlsParameters) -> io::Result<Self> {
        match self {
            NetworkStream::Tcp(stream) => {
                let connector: TlsConnector = Arc::new(tls_parameters.connector.clone()).into();
                let tls_stream = connector.connect(&tls_parameters.domain, stream);
                Ok(NetworkStream::Tls(tls_stream.await?))
            }
            _ => Ok(self),
        }
    }

    #[cfg_attr(feature = "cargo-clippy", allow(clippy::match_same_arms))]
    fn is_encrypted(&self) -> bool {
        match *self {
            NetworkStream::Tcp(_) => false,
            NetworkStream::Tls(_) => true,
            NetworkStream::Mock(_) => false,
        }
    }
}

/// A trait for read and write timeout support
pub trait Timeout: Sized {
    /// Set read timeout for IO calls
    fn set_read_timeout(&mut self, duration: Option<Duration>) -> io::Result<()>;
    /// Set write timeout for IO calls
    fn set_write_timeout(&mut self, duration: Option<Duration>) -> io::Result<()>;
}

impl Timeout for NetworkStream {
    fn set_read_timeout(&mut self, _duration: Option<Duration>) -> io::Result<()> {
        // FIXME
        // match *self {
        //     NetworkStream::Tcp(ref mut stream) => stream.set_read_timeout(duration),
        //     NetworkStream::Tls(ref mut stream) => stream.get_ref().set_read_timeout(duration),
        //     NetworkStream::Mock(_) => Ok(()),
        // }

        Ok(())
    }

    /// Set write timeout for IO calls
    fn set_write_timeout(&mut self, _duration: Option<Duration>) -> io::Result<()> {
        // FIXME
        // match *self {
        //     NetworkStream::Tcp(ref mut stream) => stream.set_write_timeout(duration),
        //     NetworkStream::Tls(ref mut stream) => stream.get_ref().set_write_timeout(duration),
        //     NetworkStream::Mock(_) => Ok(()),
        // }
        Ok(())
    }
}
