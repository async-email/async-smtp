/// Error type for email content
#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Missing from in envelope
    #[error("missing source address")]
    MissingFrom,
    /// Missing to in envelope
    #[error("missing destination address")]
    MissingTo,
    /// Invalid email
    #[error("invalid email address")]
    InvalidEmailAddress,
}

/// Email result type
pub type EmailResult<T> = Result<T, Error>;
