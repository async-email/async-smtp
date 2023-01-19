//! Async implementation of the SMTP protocol client in Rust.
//!
//! This SMTP client follows [RFC 5321](https://tools.ietf.org/html/rfc5321),
//! and is designed to efficiently send emails from an application to a relay email server,
//! as it relies as much as possible on the relay server for sanity and RFC compliance checks.
//!
//! It implements the following extensions:
//!
//! * 8BITMIME ([RFC 6152](https://tools.ietf.org/html/rfc6152))
//! * AUTH ([RFC 4954](http://tools.ietf.org/html/rfc4954)) with PLAIN, LOGIN and XOAUTH2 mechanisms
//! * STARTTLS ([RFC 2487](http://tools.ietf.org/html/rfc2487))
//! * SMTPUTF8 ([RFC 6531](http://tools.ietf.org/html/rfc6531))
//! * PIPELINING ([RFC 2920](<https://tools.ietf.org/html/rfc2920>))

#![deny(
    missing_copy_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    missing_debug_implementations,
    missing_docs,
    clippy::unwrap_used
)]

#[cfg(not(any(feature = "runtime-tokio", feature = "runtime-async-std")))]
compile_error!("one of 'runtime-async-std' or 'runtime-tokio' features must be enabled");

#[cfg(all(feature = "runtime-tokio", feature = "runtime-async-std"))]
compile_error!("only one of 'runtime-async-std' or 'runtime-tokio' features must be enabled");

pub mod authentication;
mod codec;
pub mod commands;
pub mod error;
pub mod extension;
pub mod response;
mod smtp_client;
mod stream;
mod types;
pub mod util;
pub use crate::smtp_client::{SmtpClient, SmtpTransport};
pub use types::*;

#[cfg(test)]
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
