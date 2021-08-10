use std::ffi::OsStr;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use async_std::io::{self, Cursor, Read};
use async_std::pin::Pin;
use async_std::prelude::*;
use async_std::task::{Context, Poll};
use pin_project::pin_project;

use crate::error::EmailResult;
use crate::error::Error;

/// Email address
#[derive(PartialEq, Eq, Clone, Debug)]
#[cfg_attr(
    feature = "serde-impls",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
pub struct EmailAddress(String);

impl EmailAddress {
    pub fn new(address: String) -> EmailResult<EmailAddress> {
        // Do basic checks to avoid injection of control characters into SMTP protocol.  Actual
        // email validation should be done by the server.
        if address.chars().any(|c| {
            !c.is_ascii() || c.is_ascii_control() || c.is_ascii_whitespace() || c == '<' || c == '>'
        }) {
            return Err(Error::InvalidEmailAddress);
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
#[cfg_attr(
    feature = "serde-impls",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
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
    pub fn new(from: Option<EmailAddress>, to: Vec<EmailAddress>) -> EmailResult<Envelope> {
        if to.is_empty() {
            return Err(Error::MissingTo);
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

#[pin_project(project = MessageProj)]
#[allow(missing_debug_implementations)]
pub enum Message {
    Reader(#[pin] Box<dyn Read + Send + Sync>),
    Bytes(#[pin] Cursor<Vec<u8>>),
}

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
    envelope: Envelope,
    message_id: String,
    message: Message,
}

impl SendableEmail {
    pub fn new<S: AsRef<str>, T: AsRef<[u8]>>(
        envelope: Envelope,
        message_id: S,
        message: T,
    ) -> SendableEmail {
        SendableEmail {
            envelope,
            message_id: message_id.as_ref().into(),
            message: Message::Bytes(Cursor::new(message.as_ref().to_vec())),
        }
    }

    pub fn new_with_reader(
        envelope: Envelope,
        message_id: String,
        message: Box<dyn Read + Send + Sync>,
    ) -> SendableEmail {
        SendableEmail {
            envelope,
            message_id,
            message: Message::Reader(message),
        }
    }

    pub fn envelope(&self) -> &Envelope {
        &self.envelope
    }

    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    pub fn message(self) -> Message {
        self.message
    }

    pub async fn message_to_string(mut self) -> Result<String, io::Error> {
        let mut message_content = String::new();
        self.message.read_to_string(&mut message_content).await?;
        Ok(message_content)
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
