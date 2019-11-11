//! Error and result type for sendmail transport

use async_std::io;
use snafu::Snafu;
use std::string::FromUtf8Error;

/// An enum of all error kinds.
#[derive(Debug, Snafu)]
pub enum Error {
    /// Internal client error
    #[snafu(display("client error: {}", msg))]
    Client { msg: String },
    /// Error parsing UTF8in response
    #[snafu(display("utf8 error: {}", err))]
    Utf8Parsing { err: FromUtf8Error },
    /// IO error
    #[snafu(display("io error: {}", err))]
    Io { err: io::Error },
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::Io { err }
    }
}

impl From<FromUtf8Error> for Error {
    fn from(err: FromUtf8Error) -> Error {
        Error::Utf8Parsing { err }
    }
}

/// sendmail result type
pub type SendmailResult = Result<(), Error>;
