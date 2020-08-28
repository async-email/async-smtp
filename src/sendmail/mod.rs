//! The sendmail transport sends the email using the local sendmail command.
//!

use crate::sendmail::error::{Error, SendmailResult};
use crate::Transport;
use crate::{SendableEmail, SendableEmailWithoutBody};

use async_trait::async_trait;
use log::info;
use std::convert::AsRef;
use std::process::Child;
use std::{
    process::{Command, Stdio},
    time::Duration,
};

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

#[allow(clippy::unwrap_used)]
#[async_trait]
impl<'a> Transport<'a> for SendmailTransport {
    type Result = SendmailResult;
    type StreamResult = Result<Child, Error>;

    async fn send_stream(
        &mut self,
        email: SendableEmailWithoutBody,
        _timeout: Option<&Duration>,
    ) -> Self::StreamResult {
        let command = self.command.clone();

        let from = email
            .envelope()
            .from()
            .map(AsRef::as_ref)
            .unwrap_or("\"\"") // Checkme: shouldthis be "<>"?
            .to_owned();
        let to = email.envelope().to().to_owned();

        let child = Command::new(command)
            .arg("-i")
            .arg("-f")
            .arg(from)
            .args(to)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(Error::Io)?;

        Ok(child)
    }

    async fn send(&mut self, email: SendableEmail) -> SendmailResult {
        let email_nobody =
            SendableEmailWithoutBody::new(email.envelope().clone(), email.message_id().to_string());

        let mut child = self.send_stream(email_nobody, None).await?;
        let message_id = email.message_id().to_string();
        let msg = email.message_to_string().await?.into_bytes();

        // TODO: Convert to real async, once async-std has a process implementation.
        let output = async_std::task::spawn_blocking(move || {
            let mut input = child
                .stdin
                .as_mut()
                .ok_or(Error::Client("Child process has no input".to_owned()))?;
            // FixMe: avoid loading the whole message into memory
            // in the absence of async process IO we can poll and shoufle bytes
            std::io::copy(&mut &msg[..], &mut input)?;
            child.wait_with_output().map_err(Error::Io)
        })
        .await?;
        info!("Wrote {} message to stdin", message_id);

        if output.status.success() {
            Ok(())
        } else {
            Err(error::Error::Client(String::from_utf8(output.stderr)?))
        }
    }

    async fn send_with_timeout(
        &mut self,
        email: SendableEmail,
        _timeout: Option<&Duration>,
    ) -> Self::Result {
        self.send(email).await
    }
}
