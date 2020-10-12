use async_std::io::{self, BufReader, Read, Write};
use async_std::net::ToSocketAddrs;
use async_std::pin::Pin;
use async_std::prelude::*;
use log::debug;
use pin_project::pin_project;
use std::fmt::{Debug, Display};
use std::string::String;
use std::time::Duration;

use crate::smtp::authentication::{Credentials, Mechanism};
use crate::smtp::client::net::{ClientTlsParameters, Connector, NetworkStream};
use crate::smtp::commands::*;
use crate::smtp::error::{Error, SmtpResult};
use crate::smtp::response::parse_response;
use crate::smtp::smtp_client::ConnectionReuseParameters;

/// Returns the string replacing all the CRLF with "\<CRLF\>"
/// Used for debug displays
fn escape_crlf(string: &str) -> String {
    string.replace("\r\n", "<CRLF>")
}

/// Structure that implements the SMTP client
#[pin_project]
#[derive(Debug)]
pub struct InnerClient<S: Write + Read = NetworkStream> {
    /// TCP stream between client and server
    /// Value is None before connection
    #[pin]
    pub(crate) stream: Option<S>,
    timeout: Option<Duration>,
    /// Connection reuse counter
    connection_reuse_count: u16,
    /// Enable connection reuse
    connection_reuse: ConnectionReuseParameters,
}

macro_rules! return_err (
    ($err: expr, $client: ident) => ({
        return Err(From::from($err))
    })
);

impl<S: Write + Read> Default for InnerClient<S> {
    fn default() -> Self {
        InnerClient {
            stream: None,
            timeout: None,
            connection_reuse_count: 0,
            connection_reuse: ConnectionReuseParameters::NoReuse,
        }
    }
}

impl<S: Write + Read> InnerClient<S> {
    /// Creates a new SMTP client
    ///
    /// It does not connects to the server, but only creates the `Client`
    pub fn new(connection_reuse: ConnectionReuseParameters) -> InnerClient<S> {
        InnerClient {
            connection_reuse,
            ..Default::default()
        }
    }
    pub fn debug_stats(&self) -> String {
        format!("conn_use={}", self.connection_reuse_count)
    }
    /// Checks if the underlying server socket is connected
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }
    /// Checks if this is an ongoing connection, already set up
    pub fn has_been_used(&self) -> bool {
        self.connection_reuse_count != 0
    }
    /// Test if we can reuse the existing connection
    pub fn can_be_reused(&self) -> bool {
        if !self.is_connected() {
            return false;
        }
        use ConnectionReuseParameters::*;
        match self.connection_reuse {
            ReuseUnlimited => true,
            NoReuse if self.connection_reuse_count == 0 => true,
            ReuseLimited(limit) if self.connection_reuse_count < limit => true,
            _ => false,
        }
    }

    /// The client is about to be reused, is it OK?
    pub fn reuse(&mut self) -> bool {
        self.connection_reuse_count += 1;
        self.can_be_reused()
    }

    /// Closes the SMTP transaction if possible.
    pub async fn close(mut self: Pin<&mut Self>) -> Result<(), Error> {
        self.as_mut().command(QuitCommand).await?;
        let mut this = self.project();
        *this.connection_reuse_count = 0;
        this.stream.set(None);
        Ok(())
    }

    /// Sets the underlying stream.
    pub fn set_stream(&mut self, stream: S) {
        self.stream = Some(stream);
    }

    /// Set read and write timeout.
    pub fn set_timeout(&mut self, duration: Option<Duration>) {
        self.timeout = duration;
    }

    /// Get the read and write timeout.
    pub fn timeout(&self) -> Option<&Duration> {
        self.timeout.as_ref()
    }

    /// Connects to a pre-defined stream
    pub async fn connect_with_stream(&mut self, stream: S) -> Result<(), Error> {
        // Connect should not be called when the client is already connected
        if self.stream.is_some() {
            return_err!("The connection is already established", self);
        }

        // Try to connect
        self.set_stream(stream);
        Ok(())
    }

    /// Sends an AUTH command with the given mechanism, and handles challenge if needed
    pub async fn auth(
        mut self: Pin<&mut Self>,
        mechanism: Mechanism,
        credentials: &Credentials,
    ) -> SmtpResult {
        // TODO
        let mut challenges = 10;
        let mut response = self
            .as_mut()
            .command(AuthCommand::new(mechanism, credentials.clone(), None)?)
            .await?;

        while challenges > 0 && response.has_code(334) {
            challenges -= 1;
            response = self
                .as_mut()
                .command(AuthCommand::new_from_response(
                    mechanism,
                    credentials.clone(),
                    &response,
                )?)
                .await?;
        }

        if challenges == 0 {
            Err(Error::ResponseParsing("Unexpected number of challenges"))
        } else {
            Ok(response)
        }
    }

    /// Send the given SMTP command to the server.
    pub async fn command<C: Display>(self: Pin<&mut Self>, command: C) -> SmtpResult {
        let timeout = self.timeout;
        self.command_with_timeout(command, timeout.as_ref()).await
    }

    pub async fn command_with_timeout<C: Display>(
        mut self: Pin<&mut Self>,
        command: C,
        timeout: Option<&Duration>,
    ) -> SmtpResult {
        with_timeout(timeout, self.as_mut().write(command.to_string().as_bytes())).await?;
        with_timeout(timeout, self.read_response_no_timeout()).await
    }

    /// Writes the given data to the server.
    async fn write(mut self: Pin<&mut Self>, string: &[u8]) -> Result<(), Error> {
        if self.stream.is_none() {
            return Err(From::from("Connection closed"));
        }
        let this = self.as_mut().project();
        let _: Pin<&mut Option<S>> = this.stream;
        let mut stream = this.stream.as_pin_mut().ok_or(Error::NoStream)?;

        stream.write_all(string).await?;
        stream.flush().await?;

        debug!(
            ">> {}",
            escape_crlf(String::from_utf8_lossy(string).as_ref())
        );
        Ok(())
    }

    pub async fn read_response_with_timeout(
        self: Pin<&mut Self>,
        timeout: Option<&Duration>,
    ) -> SmtpResult {
        with_timeout(timeout, self.read_response_no_timeout()).await
    }
    pub async fn read_response(self: Pin<&mut Self>) -> SmtpResult {
        let timeout = self.timeout;
        with_timeout(timeout.as_ref(), self.read_response_no_timeout()).await
    }

    /// Read an SMTP response from the wire.
    pub async fn read_response_no_timeout(mut self: Pin<&mut Self>) -> SmtpResult {
        let this = self.as_mut().project();
        let stream = this.stream.as_pin_mut().ok_or(Error::NoStream)?;

        let mut reader = BufReader::new(stream);
        let mut buffer = String::with_capacity(100);

        loop {
            let read = reader.read_line(&mut buffer).await?;
            if read == 0 {
                break;
            }
            debug!("<< {}", escape_crlf(&buffer));
            match parse_response(&buffer) {
                Ok((_remaining, response)) => {
                    if response.is_positive() {
                        return Ok(response);
                    }

                    return Err(response.into());
                }
                Err(nom::Err::Failure(e)) => {
                    return Err(Error::Parsing(e.1));
                }
                Err(nom::Err::Incomplete(_)) => { /* read more */ }
                Err(nom::Err::Error(e)) => {
                    return Err(Error::Parsing(e.1));
                }
            }
        }

        Err(io::Error::new(io::ErrorKind::Other, "incomplete").into())
    }
}

impl<S: Write + Read + Connector> InnerClient<S> {
    /// Connects to the configured server
    pub async fn connect<A: ToSocketAddrs>(
        &mut self,
        addr: &A,
        timeout: Option<Duration>,
        tls_parameters: Option<&ClientTlsParameters>,
    ) -> Result<(), Error> {
        let mut addresses = addr.to_socket_addrs().await?;

        let server_addr = match addresses.next() {
            Some(addr) => addr,
            None => return_err!("Could not resolve hostname", self),
        };

        self.connect_with_stream(Connector::connect(&server_addr, timeout, tls_parameters).await?)
            .await
    }
    /// Upgrades the underlying connection to SSL/TLS.
    pub async fn upgrade_tls_stream(
        &mut self,
        tls_parameters: &ClientTlsParameters,
    ) -> io::Result<()> {
        if let Some(stream) = self.stream.take() {
            self.stream = Some(stream.upgrade_tls(tls_parameters).await?);
        }
        Ok(())
    }
    /// Tells if the underlying stream is currently encrypted
    pub fn is_encrypted(&self) -> bool {
        self.stream
            .as_ref()
            .map(|s| s.is_encrypted())
            .unwrap_or(false)
    }
}

/// Execute io operations with an optional timeout using.
async fn with_timeout<T, F, E>(timeout: Option<&Duration>, f: F) -> std::result::Result<T, E>
where
    F: Future<Output = std::result::Result<T, E>>,
    E: From<async_std::future::TimeoutError>,
{
    if let Some(timeout) = timeout {
        let res = async_std::future::timeout(*timeout, f).await??;
        Ok(res)
    } else {
        f.await
    }
}

#[cfg(test)]
mod test {
    use super::escape_crlf;
    use crate::smtp::client::ClientCodec;

    #[async_attributes::test]
    async fn test_codec() {
        let mut codec = ClientCodec::new();
        let mut buf: Vec<u8> = vec![];

        assert!(codec.encode(b"test\r\n", &mut buf).await.is_ok());
        assert!(codec.encode(b".\r\n", &mut buf).await.is_ok());
        assert!(codec.encode(b"\r\ntest", &mut buf).await.is_ok());
        assert!(codec.encode(b"te\r\n.\r\nst", &mut buf).await.is_ok());
        assert!(codec.encode(b"test", &mut buf).await.is_ok());
        assert!(codec.encode(b"test.", &mut buf).await.is_ok());
        assert!(codec.encode(b"test\n", &mut buf).await.is_ok());
        assert!(codec.encode(b".test\n", &mut buf).await.is_ok());
        assert!(codec.encode(b"test", &mut buf).await.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "test\r\n..\r\n\r\ntestte\r\n..\r\nsttesttest.test\n.test\ntest"
        );
    }

    #[test]
    fn test_escape_crlf() {
        assert_eq!(escape_crlf("\r\n"), "<CRLF>");
        assert_eq!(escape_crlf("EHLO my_name\r\n"), "EHLO my_name<CRLF>");
        assert_eq!(
            escape_crlf("EHLO my_name\r\nSIZE 42\r\n"),
            "EHLO my_name<CRLF>SIZE 42<CRLF>"
        );
    }
}
