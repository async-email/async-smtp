//! Async-Smtp is an implementation of the smtp protocol in Rust.

#![deny(
    missing_copy_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    missing_debug_implementations,
    clippy::unwrap_used
)]

pub mod error;
#[cfg(feature = "file-transport")]
pub mod file;
#[cfg(feature = "sendmail-transport")]
pub mod sendmail;
#[cfg(feature = "smtp-transport")]
pub mod smtp;
pub mod stub;
mod types;

pub use types::*;

#[cfg(feature = "file-transport")]
pub use crate::file::FileTransport;
#[cfg(feature = "sendmail-transport")]
pub use crate::sendmail::SendmailTransport;
#[cfg(feature = "smtp-transport")]
pub use crate::smtp::client::net::ClientTlsParameters;
#[cfg(feature = "smtp-transport")]
pub use crate::smtp::{ClientSecurity, SmtpClient, SmtpTransport};

use async_trait::async_trait;
use std::time::Duration;

/// Transport method for emails
#[async_trait]
pub trait Transport<'a> {
    /// Result type for the transport
    type Result;
    type StreamResult;

    /// Start sending e-mail andreturn a stream to write the body to
    async fn send_stream(
        &mut self,
        email: SendableEmailWithoutBody,
        timeout: Option<&Duration>,
    ) -> Self::StreamResult;

    /// Sends the email
    async fn send(&mut self, email: SendableEmail) -> Self::Result;

    async fn send_with_timeout(
        &mut self,
        email: SendableEmail,
        timeout: Option<&Duration>,
    ) -> Self::Result;
}
