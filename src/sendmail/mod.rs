//! The sendmail transport sends the email using the local sendmail command.
//!

use async_std::io::Write;
use async_std::task;
use async_trait::async_trait;
use log::info;
use std::convert::AsRef;
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::task::{Context, Poll};
use std::time::Duration;

use crate::sendmail::error::{Error, SendmailResult};
use crate::{MailStream, SendableEmailWithoutBody, StreamingTransport};

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
impl StreamingTransport for SendmailTransport {
    type StreamResult = Result<ProcStream, Error>;

    async fn send_stream_with_timeout(
        &mut self,
        email: SendableEmailWithoutBody,
        _timeout: Option<&Duration>,
    ) -> Self::StreamResult {
        let command = self.command.clone();
        let message_id = email.message_id().to_string();

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

        Ok(ProcStream { child, message_id })
    }
}

#[allow(missing_debug_implementations)]
pub struct ProcStream {
    child: Child,
    message_id: String,
}

#[async_trait]
impl MailStream for ProcStream {
    type Output = ();
    type Error = Error;
    async fn done(self) -> SendmailResult {
        let child = self.child;
        let output =
            task::spawn_blocking(move || child.wait_with_output().map_err(Error::Io)).await?;

        info!("Wrote {} message to stdin", self.message_id);

        if output.status.success() {
            Ok(())
        } else {
            Err(error::Error::Client(String::from_utf8(output.stderr)?))
        }
    }
}

/// Todo: async when available
impl Write for ProcStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        use std::io::Write;
        let len = self.child.stdin.as_mut().ok_or(broken())?.write(buf)?;
        Poll::Ready(Ok(len))
    }
    fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        use std::io::Write;
        self.child.stdin.as_mut().ok_or(broken())?.flush()?;
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.poll_flush(cx)
    }
}

fn broken() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::NotConnected)
}
