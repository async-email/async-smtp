//! Error and result type for SMTP clients

use self::Error::*;
use crate::response::{Response, Severity};
use base64::DecodeError;
use std::io;
use std::net::AddrParseError;
use std::string::FromUtf8Error;

/// An enum of all error kinds.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Transient SMTP error, 4xx reply code
    ///
    /// [RFC 5321, section 4.2.1](https://tools.ietf.org/html/rfc5321#section-4.2.1)
    #[error("transient: {}", if .0.message.is_empty() { "undetailed error during SMTP transaction".to_string() } else { .0.message.join("; ") })]
    Transient(Response),
    /// Permanent SMTP error, 5xx reply code
    ///
    /// [RFC 5321, section 4.2.1](https://tools.ietf.org/html/rfc5321#section-4.2.1)
    #[error("permanent: {}", if .0.message.is_empty() { "undetailed error during SMTP transaction".to_string() } else { .0.message.join("; ") })]
    Permanent(Response),
    /// Error parsing a response
    #[error("{0}")]
    ResponseParsing(&'static str),
    /// Error parsing a base64 string in response
    #[error("challenge parsing: {0}")]
    ChallengeParsing(#[from] DecodeError),
    /// Error parsing UTF8in response
    #[error("utf8: {0}")]
    Utf8Parsing(#[from] FromUtf8Error),
    /// Internal client error
    #[error("client: {0}")]
    Client(&'static str),
    /// DNS resolution error
    #[error("could not resolve hostname")]
    Resolution,
    /// IO error
    #[error("io: {0}")]
    Io(#[from] io::Error),
    /// TLS error
    #[error("tls: {0}")]
    Tls(#[from] async_native_tls::Error),
    /// Parsing error
    #[error("parsing: {0:?}")]
    Parsing(nom::error::ErrorKind),
    #[cfg(feature = "runtime-tokio")]
    /// Timeout error
    #[error("timeout: {0}")]
    Timeout(#[from] tokio::time::error::Elapsed),
    #[cfg(feature = "runtime-async-std")]
    /// Timeout error
    #[error("timeout: {0}")]
    Timeout(#[from] async_std::future::TimeoutError),
    /// Failure to parse email address.
    #[error("address parse error: {0}")]
    AddrParseError(#[from] AddrParseError),
}

impl From<nom::Err<nom::error::Error<&str>>> for Error {
    fn from(err: nom::Err<nom::error::Error<&str>>) -> Error {
        Parsing(match err {
            nom::Err::Incomplete(_) => nom::error::ErrorKind::Complete,
            nom::Err::Failure(e) => e.code,
            nom::Err::Error(e) => e.code,
        })
    }
}

impl From<Response> for Error {
    fn from(response: Response) -> Error {
        match response.code.severity {
            Severity::TransientNegativeCompletion => Transient(response),
            Severity::PermanentNegativeCompletion => Permanent(response),
            _ => Client("Unknown error code"),
        }
    }
}

impl From<&'static str> for Error {
    fn from(string: &'static str) -> Error {
        Client(string)
    }
}

/// SMTP result type
pub type SmtpResult = Result<Response, Error>;

#[cfg(test)]
mod test {
    use super::*;
    use crate::response::{Category, Code, Detail, Response, Severity};

    #[test]
    fn test_error_response_to_string() {
        let err = Error::Permanent(Response::new(
            Code::new(
                Severity::PermanentNegativeCompletion,
                Category::Information,
                Detail::Zero,
            ),
            vec![
                "gmx.net (mxgmx117) Nemesis ESMTP Service not available".to_string(),
                "No SMTP service".to_string(),
                "IP address is block listed.".to_string(),
                "For explanation visit https://www.gmx.net/mail/senderguidelines?c=bl".to_string(),
            ],
        ));
        assert_eq!(format!("{}", err), "permanent: gmx.net (mxgmx117) Nemesis ESMTP Service not available; No SMTP service; IP address is block listed.; For explanation visit https://www.gmx.net/mail/senderguidelines?c=bl".to_string());
    }
}
