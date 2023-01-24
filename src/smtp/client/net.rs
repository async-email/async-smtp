//! A trait to represent a stream

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use async_native_tls::{TlsConnector, TlsStream};
#[cfg(feature = "runtime-async-std")]
use async_std::{io::Read, io::Write, net::TcpStream};
use async_trait::async_trait;
use futures::io::{self, ErrorKind};
use pin_project::pin_project;
#[cfg(feature = "runtime-tokio")]
use tokio::{
    io::{AsyncRead as Read, AsyncWrite as Write},
    net::TcpStream,
};

use super::inner::with_timeout;
use crate::smtp::client::mock::MockStream;

/// Parameters to use for secure clients
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
#[pin_project(project = NetworkStreamProj)]
#[allow(missing_debug_implementations)]
pub enum NetworkStream {
    /// Plain TCP stream
    Tcp(#[pin] TcpStream),
    /// Encrypted TCP stream
    Tls(#[pin] TlsStream<TcpStream>),
    /// Socks5 stream
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

    /// Shutdowns the connection.
    #[cfg(feature = "runtime-tokio")]
    pub async fn shutdown(&mut self) -> io::Result<()> {
        use tokio::io::AsyncWriteExt;
        match *self {
            NetworkStream::Tcp(ref mut s) => s.shutdown().await,
            NetworkStream::Tls(ref mut s) => s.get_mut().shutdown().await,
            NetworkStream::Mock(_) => Ok(()),
        }
    }

    /// Shutdowns the connection.
    #[cfg(feature = "runtime-async-std")]
    pub async fn shutdown(&mut self) -> io::Result<()> {
        use std::net::Shutdown;

        match *self {
            NetworkStream::Tcp(ref s) => s.shutdown(Shutdown::Both),
            NetworkStream::Tls(ref s) => s.get_ref().shutdown(Shutdown::Both),
            NetworkStream::Mock(_) => Ok(()),
        }
    }
}

#[cfg(feature = "runtime-tokio")]
impl Read for NetworkStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.project() {
            NetworkStreamProj::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_read(cx, buf)
            }
            NetworkStreamProj::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_read(cx, buf)
            }
            NetworkStreamProj::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_read(cx, buf)
            }
        }
    }
}

#[cfg(feature = "runtime-tokio")]
impl Write for NetworkStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.project() {
            NetworkStreamProj::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_write(cx, buf)
            }
            NetworkStreamProj::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_write(cx, buf)
            }
            NetworkStreamProj::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_write(cx, buf)
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.project() {
            NetworkStreamProj::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_flush(cx)
            }
            NetworkStreamProj::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_flush(cx)
            }
            NetworkStreamProj::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_flush(cx)
            }
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.project() {
            NetworkStreamProj::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_shutdown(cx)
            }
            NetworkStreamProj::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_shutdown(cx)
            }
            NetworkStreamProj::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_shutdown(cx)
            }
        }
    }
}

#[cfg(feature = "runtime-async-std")]
impl Read for NetworkStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            NetworkStreamProj::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_read(cx, buf)
            }
            NetworkStreamProj::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_read(cx, buf)
            }
            NetworkStreamProj::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_read(cx, buf)
            }
        }
    }
}

#[cfg(feature = "runtime-async-std")]
impl Write for NetworkStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.project() {
            NetworkStreamProj::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_write(cx, buf)
            }
            NetworkStreamProj::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_write(cx, buf)
            }
            NetworkStreamProj::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_write(cx, buf)
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.project() {
            NetworkStreamProj::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_flush(cx)
            }
            NetworkStreamProj::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_flush(cx)
            }
            NetworkStreamProj::Mock(s) => {
                let _: Pin<&mut MockStream> = s;
                s.poll_flush(cx)
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.project() {
            NetworkStreamProj::Tcp(s) => {
                let _: Pin<&mut TcpStream> = s;
                s.poll_close(cx)
            }
            NetworkStreamProj::Tls(s) => {
                let _: Pin<&mut TlsStream<TcpStream>> = s;
                s.poll_close(cx)
            }
            NetworkStreamProj::Mock(s) => {
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
            Some(ref duration) => with_timeout(Some(duration), TcpStream::connect(addr)).await?,
            None => TcpStream::connect(addr).await?,
        };

        match tls_parameters {
            Some(context) => {
                let connector = async {
                    context
                        .connector
                        .connect(&context.domain, tcp_stream)
                        .await
                        .map(NetworkStream::Tls)
                        .map_err(|e| io::Error::new(ErrorKind::Other, e))
                };

                match timeout {
                    Some(ref duration) => with_timeout(Some(duration), connector).await,
                    None => connector.await,
                }
            }
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
            NetworkStream::Tls(_) => Ok(self),
            NetworkStream::Mock(_) => Ok(self),
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
