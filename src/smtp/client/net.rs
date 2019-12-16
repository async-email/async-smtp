//! A trait to represent a stream

use std::fmt;
use std::time::Duration;

use async_native_tls::{TlsConnector, TlsStream};
use async_std::io::{self, ErrorKind, Read, Write};
use async_std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpStream};
use async_std::pin::Pin;
use async_std::task::{Context, Poll};
use async_trait::async_trait;
use pin_project::{pin_project, project};

use crate::smtp::client::mock::MockStream;

/// Parameters to use for secure clients
#[derive(Clone)]
pub struct ClientTlsParameters {
    /// A connector from `native-tls`
    pub connector: TlsConnector,
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
    pub fn new(domain: String, connector: TlsConnector) -> ClientTlsParameters {
        ClientTlsParameters { connector, domain }
    }
}

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
            Some(duration) => io::timeout(duration, TcpStream::connect(addr)).await?,
            None => TcpStream::connect(addr).await?,
        };

        match tls_parameters {
            Some(context) => match timeout {
                Some(duration) => async_std::future::timeout(
                    duration,
                    context.connector.connect(&context.domain, tcp_stream),
                )
                .await
                .map_err(|e| io::Error::new(ErrorKind::TimedOut, e))?
                .map(NetworkStream::Tls)
                .map_err(|e| io::Error::new(ErrorKind::Other, e)),
                None => context
                    .connector
                    .connect(&context.domain, tcp_stream)
                    .await
                    .map(NetworkStream::Tls)
                    .map_err(|e| io::Error::new(ErrorKind::Other, e)),
            },
            None => Ok(NetworkStream::Tcp(tcp_stream)),
        }
    }

    async fn upgrade_tls(self, tls_parameters: &ClientTlsParameters) -> io::Result<Self> {
        match self {
            NetworkStream::Tcp(stream) => {
                let tls_stream = tls_parameters
                    .connector
                    .connect(&tls_parameters.domain, stream)
                    .await
                    .map_err(|err| io::Error::new(ErrorKind::Other, err))?;
                Ok(NetworkStream::Tls(tls_stream))
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
