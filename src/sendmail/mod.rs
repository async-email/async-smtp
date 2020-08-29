//! The sendmail transport sends the email using the local sendmail command.
//!

use async_std::io::Write;
use async_std::task;
use async_trait::async_trait;
use futures::{ready, Future};
use log::info;
use std::convert::AsRef;
use std::ops::DerefMut;
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

        Ok(ProcStream::Ready(ProcStreamInner { child, message_id }))
    }
    /// Get the default timeout for this transport
    fn default_timeout(&self) -> Option<Duration> {
        None
    }
}

#[allow(missing_debug_implementations)]
pub enum ProcStream {
    Busy,
    Ready(ProcStreamInner),
    Closing(Pin<Box<dyn Future<Output = SendmailResult> + Send>>),
    Done(SendmailResult),
}

#[allow(missing_debug_implementations)]
pub struct ProcStreamInner {
    child: Child,
    message_id: String,
}

impl MailStream for ProcStream {
    type Output = ();
    type Error = Error;
    fn result(self) -> SendmailResult {
        match self {
            ProcStream::Done(result) => result,
            _ => Err(Error::Client(
                "Mail sending did not finish properly".to_owned(),
            )),
        }
    }
}

/// Todo: async when available
impl Write for ProcStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        loop {
            break match self.deref_mut() {
                ProcStream::Ready(ref mut inner) => {
                    use std::io::Write;
                    let len = inner.child.stdin.as_mut().ok_or_else(broken)?.write(buf)?;
                    Poll::Ready(Ok(len))
                }
                mut otherwise => {
                    ready!(Pin::new(&mut otherwise).poll_flush(cx))?;
                    continue;
                }
            };
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        loop {
            break match self.deref_mut() {
                ProcStream::Ready(ref mut inner) => {
                    use std::io::Write;
                    inner.child.stdin.as_mut().ok_or_else(broken)?.flush()?;
                    Poll::Ready(Ok(()))
                }
                ProcStream::Closing(ref mut fut) => match fut.as_mut().poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(inner) => {
                        *self = ProcStream::Done(inner);
                        continue;
                    }
                },
                ProcStream::Done(Ok(_)) => Poll::Ready(Ok(())),
                ProcStream::Done(Err(_)) => Poll::Ready(Err(broken())),
                ProcStream::Busy => Poll::Ready(Err(broken())),
            };
        }
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        loop {
            break match std::mem::replace(self.deref_mut(), ProcStream::Busy) {
                ProcStream::Ready(ProcStreamInner { child, message_id }) => {
                    let fut = async move {
                        let output = task::spawn_blocking(move || {
                            child.wait_with_output().map_err(Error::Io)
                        })
                        .await?;

                        info!("Wrote {} message to stdin", message_id);

                        if output.status.success() {
                            Ok(())
                        } else {
                            Err(error::Error::Client(String::from_utf8(output.stderr)?))
                        }
                    };
                    *self = ProcStream::Closing(Box::pin(fut));
                    continue;
                }
                otherwise @ ProcStream::Closing(_) => {
                    *self = otherwise;
                    ready!(Pin::new(&mut self).poll_flush(cx))?;
                    continue;
                }
                otherwise => {
                    *self = otherwise;
                    ready!(Pin::new(&mut self).poll_flush(cx))?;
                    Poll::Ready(Ok(()))
                }
            };
        }
    }
}

fn broken() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::NotConnected)
}
