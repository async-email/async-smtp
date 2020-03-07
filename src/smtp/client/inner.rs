use std::io;
use std::fmt::{Debug, Display};
use std::future::Future;
use std::pin::Pin;
use std::string::String;
use std::time::Duration;

use log::debug;
use pin_project::pin_project;

use crate::smtp::authentication::{Credentials, Mechanism};
use crate::smtp::client::net::{ClientTlsParameters, Connector, NetworkStream};
use crate::smtp::client::ClientCodec;
use crate::smtp::commands::*;
use crate::smtp::error::{Error, SmtpResult};
use crate::smtp::response::parse_response;
use crate::runtime::{
    Read,
    Write,
    AsyncWriteExt,
    AsyncReadExt,
    BufReadExt,
    BufReader,
    ToSocketAddrs,
    io_timeout
};

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
}

impl<S: Write + Read> Default for InnerClient<S> {
    fn default() -> Self {
        InnerClient {
            stream: None,
            timeout: None,
        }
    }
}

macro_rules! return_err (
    ($err: expr, $client: ident) => ({
        return Err(From::from($err))
    })
);

impl<S: Write + Read> InnerClient<S> {
    /// Creates a new SMTP client
    ///
    /// It does not connects to the server, but only creates the `Client`
    pub fn new() -> InnerClient<S> {
        InnerClient::default()
    }
}

impl<S: Connector + Write + Read + Unpin> InnerClient<S> {
    /// Closes the SMTP transaction if possible.
    pub async fn close(mut self: Pin<&mut Self>) -> Result<(), Error> {
        self.as_mut().command(QuitCommand).await?;
        self.get_mut().stream = None;

        Ok(())
    }

    /// Sets the underlying stream.
    pub fn set_stream(&mut self, stream: S) {
        self.stream = Some(stream);
    }

    /// Upgrades the underlying connection to SSL/TLS.
    pub async fn upgrade_tls_stream(
        self,
        tls_parameters: &ClientTlsParameters,
    ) -> io::Result<Self> {
        match self.stream {
            Some(stream) => Ok(InnerClient {
                stream: Some(stream.upgrade_tls(tls_parameters).await?),
                timeout: self.timeout,
            }),
            None => Ok(self),
        }
    }

    /// Tells if the underlying stream is currently encrypted
    pub fn is_encrypted(&self) -> bool {
        self.stream
            .as_ref()
            .map(|s| s.is_encrypted())
            .unwrap_or(false)
    }

    /// Set read and write timeout.
    pub fn set_timeout(&mut self, duration: Option<Duration>) {
        self.timeout = duration;
    }

    /// Get the read and write timeout.
    pub fn timeout(&mut self) -> Option<&Duration> {
        self.timeout.as_ref()
    }

    /// Connects to the configured server
    pub async fn connect<A: ToSocketAddrs>(
        &mut self,
        addr: &A,
        timeout: Option<Duration>,
        tls_parameters: Option<&ClientTlsParameters>,
    ) -> Result<(), Error> {
        // Connect should not be called when the client is already connected
        if self.stream.is_some() {
            return_err!("The connection is already established", self);
        }

        let mut addresses = addr.to_socket_addrs().await?;

        let server_addr = match addresses.next() {
            Some(addr) => addr,
            None => return_err!("Could not resolve hostname", self),
        };

        debug!("connecting to {}", server_addr);

        // Try to connect
        self.set_stream(Connector::connect(&server_addr, timeout, tls_parameters).await?);
        Ok(())
    }

    /// Checks if the underlying server socket is connected
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
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

    /// Sends the message content.
    pub async fn message<T: Read + Unpin>(mut self: Pin<&mut Self>, message: T) -> SmtpResult {
        let mut codec = ClientCodec::new();

        let mut message_reader = BufReader::new(message);

        let mut message_bytes = Vec::new();
        message_reader.read_to_end(&mut message_bytes).await?;

        if self.stream.is_none() {
            return Err(From::from("Connection closed"));
        }
        let this = self.as_mut().project();
        let _: Pin<&mut Option<S>> = this.stream;

        let mut stream = this.stream.as_pin_mut().ok_or(Error::NoStream)?;

        with_timeout(this.timeout.as_ref(), async move {
            codec.encode(&message_bytes, &mut stream).await?;
            stream.write_all(b"\r\n.\r\n").await?;
            Ok(())
        })
        .await?;

        self.read_response().await
    }

    /// Send the given SMTP command to the server.
    pub async fn command<C: Display>(mut self: Pin<&mut Self>, command: C) -> SmtpResult {
        self.as_mut().write(command.to_string().as_bytes()).await?;
        self.read_response().await
    }

    /// Writes the given data to the server.
    async fn write(mut self: Pin<&mut Self>, string: &[u8]) -> Result<(), Error> {
        if self.stream.is_none() {
            return Err(From::from("Connection closed"));
        }
        let this = self.as_mut().project();
        let _: Pin<&mut Option<S>> = this.stream;
        let mut stream = this.stream.as_pin_mut().ok_or(Error::NoStream)?;

        with_timeout(this.timeout.as_ref(), async move {
            stream.write_all(string).await?;
            stream.flush().await?;
            Ok(())
        })
        .await?;

        debug!(
            ">> {}",
            escape_crlf(String::from_utf8_lossy(string).as_ref())
        );
        Ok(())
    }

    /// Read an SMTP response from the wire.
    pub async fn read_response(mut self: Pin<&mut Self>) -> SmtpResult {
        let this = self.as_mut().project();
        let stream = this.stream.as_pin_mut().ok_or(Error::NoStream)?;

        let mut reader = BufReader::new(stream);
        let mut buffer = String::with_capacity(100);

        loop {
            let read = with_timeout(this.timeout.as_ref(), reader.read_line(&mut buffer)).await?;
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

/// Execute io operations with an optional timeout using.
async fn with_timeout<T, F>(timeout_duration: Option<&Duration>, f: F) -> Result<T, Error>
where
    F: Future<Output = io::Result<T>>,
{
    let r = if let Some(timeout_duration) = timeout_duration {
        io_timeout(*timeout_duration, f).await?
    } else {
        f.await?
    };

    Ok(r)
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
