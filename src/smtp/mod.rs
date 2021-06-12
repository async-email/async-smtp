//! The SMTP transport sends emails using the SMTP protocol.
//!
//! This SMTP client follows [RFC
//! 5321](https://tools.ietf.org/html/rfc5321), and is designed to efficiently send emails from an
//! application to a relay email server, as it relies as much as possible on the relay server
//! for sanity and RFC compliance checks.
//!
//! It implements the following extensions:
//!
//! * 8BITMIME ([RFC 6152](https://tools.ietf.org/html/rfc6152))
//! * AUTH ([RFC 4954](http://tools.ietf.org/html/rfc4954)) with PLAIN, LOGIN and XOAUTH2 mechanisms
//! * STARTTLS ([RFC 2487](http://tools.ietf.org/html/rfc2487))
//! * SMTPUTF8 ([RFC 6531](http://tools.ietf.org/html/rfc6531))

pub mod authentication;
pub mod client;
pub mod commands;
pub mod error;
pub mod extension;
pub mod response;
mod smtp_client;
pub mod util;

pub use smtp_client::*;

#[cfg(feature = "socks5")]
pub use smtp_client::Socks5Config;
