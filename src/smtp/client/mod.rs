//! SMTP client

use std::fmt::{Debug, Display};
use std::string::String;
use std::time::Duration;

use async_std::io::{self, BufReader, Read, Write};
use async_std::net::ToSocketAddrs;
use async_std::pin::Pin;
use async_std::prelude::*;
use log::debug;
use pin_project::pin_project;

use crate::smtp::authentication::{Credentials, Mechanism};
use crate::smtp::client::net::{ClientTlsParameters, Connector, NetworkStream, Timeout};
use crate::smtp::commands::*;
use crate::smtp::error::{Error, SmtpResult};
use crate::smtp::response::Response;

pub mod mock;
pub mod net;

/// The codec used for transparency
#[derive(Default, Clone, Copy, Debug)]
#[cfg_attr(
    feature = "serde-impls",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
pub struct ClientCodec {
    escape_count: u8,
}

impl ClientCodec {
    /// Creates a new client codec
    pub fn new() -> Self {
        ClientCodec::default()
    }
}

impl ClientCodec {
    /// Adds transparency
    /// TODO: replace CR and LF by CRLF
    async fn encode(&mut self, frame: &[u8], buf: &mut Vec<u8>) -> Result<(), Error> {
        match frame.len() {
            0 => {
                match self.escape_count {
                    0 => buf.write_all(b"\r\n.\r\n").await?,
                    1 => buf.write_all(b"\n.\r\n").await?,
                    2 => buf.write_all(b".\r\n").await?,
                    _ => unreachable!(),
                }
                self.escape_count = 0;
                Ok(())
            }
            _ => {
                let mut start = 0;
                for (idx, byte) in frame.iter().enumerate() {
                    match self.escape_count {
                        0 => self.escape_count = if *byte == b'\r' { 1 } else { 0 },
                        1 => self.escape_count = if *byte == b'\n' { 2 } else { 0 },
                        2 => self.escape_count = if *byte == b'.' { 3 } else { 0 },
                        _ => unreachable!(),
                    }
                    if self.escape_count == 3 {
                        self.escape_count = 0;
                        buf.write_all(&frame[start..idx]).await?;
                        buf.write_all(b".").await?;
                        start = idx;
                    }
                }
                buf.write_all(&frame[start..]).await?;
                Ok(())
            }
        }
    }
}

/// Returns the string replacing all the CRLF with "\<CRLF\>"
/// Used for debug displays
fn escape_crlf(string: &str) -> String {
    string.replace("\r\n", "<CRLF>")
}

/// Structure that implements the SMTP client
#[pin_project]
#[derive(Debug, Default)]
pub struct InnerClient<S: Write + Read = NetworkStream> {
    /// TCP stream between client and server
    /// Value is None before connection
    #[pin]
    pub(crate) stream: Option<S>,
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
        InnerClient { stream: None }
    }
}

impl<S: Connector + Write + Read + Timeout + Unpin> InnerClient<S> {
    /// Closes the SMTP transaction if possible
    pub fn close(mut self: Pin<&mut Self>) {
        let _ = self.as_mut().command(QuitCommand);
        self.get_mut().stream = None;
    }

    /// Sets the underlying stream
    pub fn set_stream(&mut self, stream: S) {
        self.stream = Some(stream);
    }

    /// Upgrades the underlying connection to SSL/TLS
    pub async fn upgrade_tls_stream(
        self,
        tls_parameters: &ClientTlsParameters,
    ) -> io::Result<Self> {
        match self.stream {
            Some(stream) => Ok(InnerClient {
                stream: Some(stream.upgrade_tls(tls_parameters).await?),
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

    /// Set timeout
    pub fn set_timeout(&mut self, duration: Option<Duration>) -> io::Result<()> {
        if let Some(ref mut stream) = self.stream {
            stream.set_read_timeout(duration)?;
            stream.set_write_timeout(duration)?;
        }
        Ok(())
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
    #[cfg_attr(feature = "cargo-clippy", allow(clippy::wrong_self_convention))]
    pub fn is_connected(&mut self) -> bool {
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

    /// Sends the message content
    pub async fn message<T: Read + Unpin>(mut self: Pin<&mut Self>, message: T) -> SmtpResult {
        let mut codec = ClientCodec::new();

        let mut message_reader = BufReader::new(message);

        // TODO: streaming encoding

        let mut message_bytes = Vec::new();
        message_reader.read_to_end(&mut message_bytes).await?;

        let mut out_buf: Vec<u8> = vec![];
        codec.encode(&message_bytes, &mut out_buf).await?;

        self.as_mut().write(&out_buf).await?;
        self.as_mut().write(b"\r\n.\r\n").await?;

        self.read_response().await
    }

    /// Sends an SMTP command
    pub async fn command<C: Display>(mut self: Pin<&mut Self>, command: C) -> SmtpResult {
        self.as_mut().write(command.to_string().as_bytes()).await?;
        self.read_response().await
    }

    /// Writes a string to the server
    async fn write(mut self: Pin<&mut Self>, string: &[u8]) -> Result<(), Error> {
        if self.stream.is_none() {
            return Err(From::from("Connection closed"));
        }
        let this = self.as_mut().project();
        let _: Pin<&mut Option<S>> = this.stream;
        let mut stream = this.stream.as_pin_mut().unwrap();

        stream.write_all(string).await?;
        stream.flush().await?;

        debug!(
            "Wrote: {}",
            escape_crlf(String::from_utf8_lossy(string).as_ref())
        );
        Ok(())
    }

    /// Gets the SMTP response
    pub async fn read_response(mut self: Pin<&mut Self>) -> SmtpResult {
        let mut raw_response = String::new();
        let mut response = raw_response.parse::<Response>();

        let this = self.as_mut().project();
        let stream: Pin<&mut S> = this.stream.as_pin_mut().unwrap();

        let mut reader = BufReader::new(stream);

        while response.is_err() {
            if let Error::Parsing(nom::error::ErrorKind::Complete) =
                response.as_ref().err().unwrap()
            {
                break;
            }
            // TODO read more than one line
            let read_count = reader.read_line(&mut raw_response).await?;

            // EOF is reached
            if read_count == 0 {
                break;
            }

            response = raw_response.parse::<Response>();
        }

        debug!("Read: {}", escape_crlf(raw_response.as_ref()));

        let final_response = response?;

        if final_response.is_positive() {
            Ok(final_response)
        } else {
            Err(From::from(final_response))
        }
    }
}

#[cfg(test)]
mod test {
    use super::{escape_crlf, ClientCodec};

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
