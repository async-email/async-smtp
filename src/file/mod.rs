//! The file transport writes the emails to the given directory. The name of the file will be
//! `message_id.txt`.
//! It can be useful for testing purposes, or if you want to keep track of sent messages.
//!

use async_std::fs::File;
use async_std::io::Write;
use async_std::path::Path;
use async_trait::async_trait;
use futures::io::AsyncWriteExt;
use futures::ready;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{path::PathBuf, time::Duration};

use crate::file::error::{Error, FileResult};
use crate::Envelope;
use crate::MailStream;
use crate::SendableEmailWithoutBody;
use crate::StreamingTransport;

pub mod error;

/// Writes the content and the envelope information to a file.
#[derive(Debug)]
#[cfg_attr(
    feature = "serde-impls",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
pub struct FileTransport {
    path: PathBuf,
}

impl FileTransport {
    /// Creates a new transport to the given directory
    pub fn new<P: AsRef<Path>>(path: P) -> FileTransport {
        FileTransport {
            path: PathBuf::from(path.as_ref()),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
#[cfg_attr(
    feature = "serde-impls",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
struct SerializableEmail {
    envelope: Envelope,
    message_id: String,
}

#[async_trait]
impl StreamingTransport for FileTransport {
    type StreamResult = Result<FileStream, Error>;

    async fn send_stream_with_timeout(
        &mut self,
        email: SendableEmailWithoutBody,
        _timeout: Option<&Duration>,
    ) -> Self::StreamResult {
        let message_id = email.message_id().to_string();
        let envelope = email.envelope().clone();

        let mut file = self.path.clone();
        file.push(format!("{}.json", message_id));

        let mut serialized = serde_json::to_string(&SerializableEmail {
            envelope,
            message_id,
        })?;

        serialized += "\n";

        let mut file = File::create(file).await?;
        file.write_all(serialized.as_bytes()).await?;

        Ok(FileStream {
            file,
            closed: false,
        })
    }
    /// Get the default timeout for this transport
    fn default_timeout(&self) -> Option<Duration> {
        None
    }
}

#[derive(Debug)]
pub struct FileStream {
    file: File,
    closed: bool,
}

impl MailStream for FileStream {
    type Output = ();
    type Error = Error;
    fn result(self) -> FileResult {
        if self.closed {
            Ok(())
        } else {
            Err(Error::Client("file was not closed properly"))
        }
    }
}

impl Write for FileStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, std::io::Error>> {
        Pin::new(&mut self.file).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        Pin::new(&mut self.file).poll_flush(cx)
    }
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        ready!(Pin::new(&mut self.file).poll_close(cx)?);
        self.closed = true;
        Poll::Ready(Ok(()))
    }
}
