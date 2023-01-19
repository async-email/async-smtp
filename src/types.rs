use std::ffi::OsStr;
use std::fmt::{self, Display, Formatter};
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};

use anyhow::{bail, Error, Result};
#[cfg(feature = "runtime-async-std")]
use async_std::io::{Cursor, Read};
use futures::io;
use pin_project::pin_project;
#[cfg(feature = "runtime-tokio")]
use std::io::Cursor;
#[cfg(feature = "runtime-tokio")]
use tokio::io::AsyncRead as Read;

/// Email address
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct EmailAddress(String);

impl EmailAddress {
    /// Creates new email address, checking that it does not contain invalid characters.
    pub fn new(address: String) -> Result<EmailAddress> {
        // Do basic checks to avoid injection of control characters into SMTP protocol.  Actual
        // email validation should be done by the server.
        if address.chars().any(|c| {
            !c.is_ascii() || c.is_ascii_control() || c.is_ascii_whitespace() || c == '<' || c == '>'
        }) {
            bail!("invalid email address");
        }

        Ok(EmailAddress(address))
    }
}

impl FromStr for EmailAddress {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        EmailAddress::new(s.to_string())
    }
}

impl Display for EmailAddress {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for EmailAddress {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<OsStr> for EmailAddress {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

/// Simple email envelope representation
///
/// We only accept mailboxes, and do not support source routes (as per RFC).
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Envelope {
    /// The envelope recipients' addresses
    ///
    /// This can not be empty.
    forward_path: Vec<EmailAddress>,
    /// The envelope sender address
    reverse_path: Option<EmailAddress>,
}

impl Envelope {
    /// Creates a new envelope, which may fail if `to` is empty.
    pub fn new(from: Option<EmailAddress>, to: Vec<EmailAddress>) -> Result<Envelope> {
        if to.is_empty() {
            bail!("missing destination address");
        }
        Ok(Envelope {
            forward_path: to,
            reverse_path: from,
        })
    }

    /// Destination addresses of the envelope
    pub fn to(&self) -> &[EmailAddress] {
        self.forward_path.as_slice()
    }

    /// Source address of the envelope
    pub fn from(&self) -> Option<&EmailAddress> {
        self.reverse_path.as_ref()
    }
}

/// Message buffer for sending.
#[pin_project(project = MessageProj)]
#[allow(missing_debug_implementations)]
pub enum Message {
    /// Message constructed from a reader.
    Reader(#[pin] Box<dyn Read + Send + Sync>),
    /// Message constructed from a byte vector.
    Bytes(#[pin] Cursor<Vec<u8>>),
}

#[cfg(feature = "runtime-tokio")]
impl Read for Message {
    #[allow(unsafe_code)]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.project() {
            MessageProj::Reader(mut rdr) => {
                // Probably safe..
                let r: Pin<&mut _> = unsafe { Pin::new_unchecked(&mut **rdr) };
                r.poll_read(cx, buf)
            }
            MessageProj::Bytes(rdr) => {
                let _: Pin<&mut _> = rdr;
                rdr.poll_read(cx, buf)
            }
        }
    }
}

#[cfg(feature = "runtime-async-std")]
impl Read for Message {
    #[allow(unsafe_code)]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            MessageProj::Reader(mut rdr) => {
                // Probably safe..
                let r: Pin<&mut _> = unsafe { Pin::new_unchecked(&mut **rdr) };
                r.poll_read(cx, buf)
            }
            MessageProj::Bytes(rdr) => {
                let _: Pin<&mut _> = rdr;
                rdr.poll_read(cx, buf)
            }
        }
    }
}

/// Sendable email structure
#[allow(missing_debug_implementations)]
pub struct SendableEmail {
    /// Email envelope.
    envelope: Envelope,
    message: Message,
}

impl SendableEmail {
    /// Creates new email out of an envelope and a byte slice.
    pub fn new(envelope: Envelope, message: impl Into<Vec<u8>>) -> SendableEmail {
        let message: Vec<u8> = message.into();
        SendableEmail {
            envelope,
            message: Message::Bytes(Cursor::new(message)),
        }
    }

    /// Creates new email out of an envelope and a byte reader.
    pub fn new_with_reader(
        envelope: Envelope,
        message: Box<dyn Read + Send + Sync>,
    ) -> SendableEmail {
        SendableEmail {
            envelope,
            message: Message::Reader(message),
        }
    }

    /// Returns email envelope.
    pub fn envelope(&self) -> &Envelope {
        &self.envelope
    }

    /// Returns email message.
    pub fn message(self) -> Message {
        self.message
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_email_address() {
        assert!(EmailAddress::new("foobar@example.org".to_string()).is_ok());
        assert!(EmailAddress::new("foobar@localhost".to_string()).is_ok());
        assert!(EmailAddress::new("foo\rbar@localhost".to_string()).is_err());
        assert!(EmailAddress::new("foobar@localhost".to_string()).is_ok());
        assert!(EmailAddress::new(
            "617b5772c6d10feda41fc6e0e43b976c4cc9383d3729310d3dc9e1332f0d9acd@yggmail".to_string()
        )
        .is_ok());
        assert!(EmailAddress::new(">foobar@example.org".to_string()).is_err());
        assert!(EmailAddress::new("foo bar@example.org".to_string()).is_err());
        assert!(EmailAddress::new("foobar@exa\r\nmple.org".to_string()).is_err());
    }
}
