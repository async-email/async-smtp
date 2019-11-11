//! Error and result type for file transport

use async_std::io;
use serde_json;
use snafu::Snafu;

/// An enum of all error kinds.
#[derive(Snafu, Debug)]
pub enum Error {
    /// Internal client error
    #[snafu(display("client error: {}", msg))]
    Client { msg: &'static str },
    /// IO error
    #[snafu(display("io error: {}", err))]
    Io { err: io::Error },
    /// JSON serialization error
    #[snafu(display("serialization error: {}", err))]
    JsonSerialization { err: serde_json::Error },
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::Io { err }
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Error {
        Error::JsonSerialization { err }
    }
}

impl From<&'static str> for Error {
    fn from(string: &'static str) -> Error {
        Error::Client { msg: string }
    }
}

/// SMTP result type
pub type FileResult = Result<(), Error>;
