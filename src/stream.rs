use std::fmt::{Debug, Display};
use std::string::String;

use log::debug;

use crate::codec::ClientCodec;
use crate::commands::*;
use crate::error::{Error, SmtpResult};
use crate::extension::ClientId;
use crate::response::parse_response;

#[cfg(feature = "runtime-async-std")]
use async_std::io::{prelude::*, BufReader, Read, ReadExt, Write, WriteExt};
#[cfg(feature = "runtime-tokio")]
use tokio::io::{
    AsyncBufReadExt, AsyncRead as Read, AsyncReadExt, AsyncWrite as Write, AsyncWriteExt, BufReader,
};

/// SMTP stream.
#[derive(Debug)]
pub struct SmtpStream<S: Read + Write + Unpin> {
    /// Inner stream.
    inner: BufReader<S>,
}

impl<S: Read + Write + Unpin> SmtpStream<S> {
    /// Creates new SMTP stream.
    pub fn new(stream: S) -> Self {
        Self {
            inner: BufReader::new(stream),
        }
    }

    /// Returns inner stream.
    ///
    /// Should only be used when there are no unread responses,
    /// because the buffer of `BufReader` may be lost.
    pub fn into_inner(self) -> S {
        self.inner.into_inner()
    }

    /// Sends EHLO command and returns server response.
    pub async fn ehlo(&mut self, client_id: ClientId) -> SmtpResult {
        // Extended Hello
        let ehlo_response = self.command(EhloCommand::new(client_id)).await?;
        Ok(ehlo_response)
    }

    /// Send the given SMTP command to the server.
    pub async fn command(&mut self, command: impl Display) -> SmtpResult {
        self.send_command(command).await?;
        self.read_response().await
    }

    /// Sends the given SMTP command to the server without waiting for response.
    pub async fn send_command(&mut self, command: impl Display) -> Result<(), Error> {
        self.write(command.to_string().as_bytes()).await?;
        Ok(())
    }

    /// Writes the given data to the server.
    async fn write(&mut self, string: &[u8]) -> Result<(), Error> {
        self.inner.get_mut().write_all(string).await?;
        self.inner.get_mut().flush().await?;

        debug!(
            ">> {}",
            escape_crlf(String::from_utf8_lossy(string).as_ref())
        );
        Ok(())
    }

    /// Read an SMTP response from the wire.
    pub async fn read_response(&mut self) -> SmtpResult {
        let reader = &mut self.inner;
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
                    return Err(Error::Parsing(e.code));
                }
                Err(nom::Err::Incomplete(_)) => { /* read more */ }
                Err(nom::Err::Error(e)) => {
                    return Err(Error::Parsing(e.code));
                }
            }
        }

        Err(std::io::Error::new(std::io::ErrorKind::Other, "incomplete").into())
    }

    /// Sends the message content.
    pub(crate) async fn message<T: Read + Unpin>(&mut self, message: T) -> SmtpResult {
        let mut codec = ClientCodec::new();

        let mut message_reader = BufReader::new(message);

        let mut message_bytes = Vec::new();
        message_reader.read_to_end(&mut message_bytes).await?;

        let res: Result<(), Error> = async {
            codec.encode(&message_bytes, self.inner.get_mut()).await?;
            self.inner.get_mut().write_all(b"\r\n.\r\n").await?;
            self.inner.get_mut().flush().await?;
            Ok(())
        }
        .await;
        res?;

        self.read_response().await
    }
}

/// Returns the string replacing all the CRLF with "\<CRLF\>"
/// Used for debug displays
fn escape_crlf(string: &str) -> String {
    string.replace("\r\n", "<CRLF>")
}

#[cfg(test)]
mod test {
    use super::escape_crlf;

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
