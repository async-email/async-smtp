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

#[cfg(not(any(feature = "runtime-tokio", feature = "runtime-async-std")))]
compile_error!("one of 'runtime-async-std' or 'runtime-tokio' features must be enabled");

#[cfg(all(feature = "runtime-tokio", feature = "runtime-async-std"))]
compile_error!("only one of 'runtime-async-std' or 'runtime-tokio' features must be enabled");

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
pub use crate::smtp::{ClientSecurity, ServerAddress, SmtpClient, SmtpTransport};

#[cfg(features = "socks5")]
pub use crate::smtp::SmtpClient::Socks5Config;

use async_trait::async_trait;
use std::time::Duration;

/// Transport method for emails
#[async_trait]
pub trait Transport<'a> {
    /// Result type for the transport
    type Result;

    /// Sends the email
    async fn send(&mut self, email: SendableEmail) -> Self::Result;

    async fn send_with_timeout(
        &mut self,
        email: SendableEmail,
        timeout: Option<&Duration>,
    ) -> Self::Result;
}

#[macro_export]
macro_rules! async_test {
    ($name:ident, $block:block) => {
        #[cfg(feature = "runtime-tokio")]
        #[tokio::test]
        async fn $name() {
            $block
        }

        #[cfg(feature = "runtime-async-std")]
        #[async_std::test]
        async fn $name() {
            $block
        }
    };
}

#[macro_export]
macro_rules! async_test_ignore {
    ($name:ident, $block:block) => {
        #[cfg(feature = "runtime-tokio")]
        #[tokio::test]
        #[ignore]
        async fn $name() {
            $block
        }

        #[cfg(feature = "runtime-async-std")]
        #[async_std::test]
        #[ignore]
        async fn $name() {
            $block
        }
    };
}
