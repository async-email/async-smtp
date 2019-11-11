use snafu::Snafu;

/// Error type for email content
#[derive(Snafu, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Missing from in envelope
    #[snafu(display("missing source address"))]
    MissingFrom,
    /// Missing to in envelope
    #[snafu(display("missing destination address"))]
    MissingTo,
    /// Invalid email
    #[snafu(display("invalid email address"))]
    InvalidEmailAddress,
}

/// Email result type
pub type EmailResult<T> = Result<T, Error>;
