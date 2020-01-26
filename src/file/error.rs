//! Error and result type for file transport

use async_std::io;
use serde_json;

/// An enum of all error kinds.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Internal client error
    #[error("client error: {0}")]
    Client(&'static str),
    /// IO error
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// JSON serialization error
    #[error("serialization error: {0}")]
    JsonSerialization(#[from] serde_json::Error),
}

impl From<&'static str> for Error {
    fn from(string: &'static str) -> Error {
        Error::Client(string)
    }
}

/// SMTP result type
pub type FileResult = Result<(), Error>;
