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

use async_std::io::{copy, Write};
use async_trait::async_trait;
use futures::io::AsyncWriteExt;
use std::time::Duration;

/// Transport method for emails
#[async_trait]
pub trait StreamingTransport {
    /// Result type for the transport
    type StreamResult;

    /// Start sending e-mail and return a stream to write the body to with a timeout
    async fn send_stream_with_timeout(
        &mut self,
        email: SendableEmailWithoutBody,
        timeout: Option<&Duration>,
    ) -> Self::StreamResult;

    /// Get the default timeout for this transport
    fn default_timeout(&self) -> Option<Duration>;
}
#[async_trait]
pub trait Transport: StreamingTransport {
    type SendResult;
    /// Sends the email
    async fn send(&mut self, email: SendableEmail) -> Self::SendResult;
    // Send an e-mail with a timeout
    async fn send_with_timeout(
        &mut self,
        email: SendableEmail,
        timeout: Option<&Duration>,
    ) -> Self::SendResult;
    /// Start sending e-mail and return a stream to write the body to
    async fn send_stream(&mut self, email: SendableEmailWithoutBody) -> Self::StreamResult;
}

pub trait MailStream: Write {
    type Output;
    type Error;
    fn result(self) -> Result<Self::Output, Self::Error>;
}

#[async_trait]
impl<T, S, E> Transport for T
where
    T: StreamingTransport<StreamResult = Result<S, E>>,
    T: Sync + Send,
    S: MailStream + Unpin + Send + 'static,
    S::Error: From<std::io::Error>,
    S::Error: From<E>,
    E: 'static,
{
    type SendResult = Result<S::Output, S::Error>;
    /// Sends the email
    async fn send(&mut self, email: SendableEmail) -> Self::SendResult {
        self.send_with_timeout(email, self.default_timeout().as_ref())
            .await
    }
    // Send an e-mail with a timeout
    async fn send_with_timeout(
        &mut self,
        email: SendableEmail,
        timeout: Option<&Duration>,
    ) -> Self::SendResult {
        let mut stream = self
            .send_stream_with_timeout(
                SendableEmailWithoutBody::new(
                    email.envelope().clone(),
                    email.message_id().to_string(),
                ),
                timeout,
            )
            .await?;

        copy(email.message(), &mut stream).await?;
        stream.close().await?;
        stream.result()
    }
    /// Start sending e-mail and return a stream to write the body to
    async fn send_stream(
        &mut self,
        email: SendableEmailWithoutBody,
    ) -> <Self as StreamingTransport>::StreamResult {
        self.send_stream_with_timeout(email, self.default_timeout().as_ref())
            .await
    }
}
