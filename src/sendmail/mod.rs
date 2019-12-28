//! The sendmail transport sends the email using the local sendmail command.
//!

use crate::sendmail::error::SendmailResult;
use crate::SendableEmail;
use crate::Transport;

use async_std::prelude::*;
use async_trait::async_trait;
use log::info;
use std::convert::AsRef;
use std::io::prelude::*;
use std::process::{Command, Stdio};

pub mod error;

/// Sends an email using the `sendmail` command
#[derive(Debug, Default)]
#[cfg_attr(
    feature = "serde-impls",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
pub struct SendmailTransport {
    command: String,
}

impl SendmailTransport {
    /// Creates a new transport with the default `/usr/sbin/sendmail` command
    pub fn new() -> SendmailTransport {
        SendmailTransport {
            command: "/usr/sbin/sendmail".to_string(),
        }
    }

    /// Creates a new transport to the given sendmail command
    pub fn new_with_command<S: Into<String>>(command: S) -> SendmailTransport {
        SendmailTransport {
            command: command.into(),
        }
    }
}

#[allow(clippy::option_unwrap_used)]
#[async_trait]
impl<'a> Transport<'a> for SendmailTransport {
    type Result = SendmailResult;

    async fn send(&mut self, email: SendableEmail) -> SendmailResult {
        let message_id = email.message_id().to_string();
        let command = self.command.clone();

        let from = email
            .envelope()
            .from()
            .map(AsRef::as_ref)
            .unwrap_or("\"\"")
            .to_owned();
        let to = email.envelope().to().to_owned();
        let mut message_content = String::new();
        let _ = email.message().read_to_string(&mut message_content).await;

        // TODO: Convert to real async, once async-std has a process implementation.
        let output = async_std::task::spawn_blocking(move || {
            // Spawn the sendmail command
            let mut process = Command::new(command)
                .arg("-i")
                .arg("-f")
                .arg(from)
                .args(to)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;

            process
                .stdin
                .as_mut()
                .unwrap()
                .write_all(message_content.as_bytes())?;

            info!("Wrote {} message to stdin", message_id);

            process.wait_with_output()
        })
        .await?;

        if output.status.success() {
            return Ok(());
        }

        Err(error::Error::Client {
            msg: String::from_utf8(output.stderr)?,
        })
    }
}
