//! The file transport writes the emails to the given directory. The name of the file will be
//! `message_id.txt`.
//! It can be useful for testing purposes, or if you want to keep track of sent messages.
//!

use std::{path::PathBuf, time::Duration};

use async_std::fs::File;
use async_std::io::copy;
use async_std::io::Write;
use async_std::path::Path;
use async_std::prelude::*;
use async_trait::async_trait;
use std::pin::Pin;

use crate::file::error::{Error, FileResult};
use crate::Envelope;
use crate::Transport;
use crate::{SendableEmail, SendableEmailWithoutBody};

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

#[derive(Debug)]
pub struct FileStream {
    file: File,
}

#[async_trait]
impl<'a> Transport<'a> for FileTransport {
    type Result = FileResult;
    type StreamResult = Result<FileStream, Error>;

    async fn send_stream(
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

        Ok(FileStream { file })
    }

    async fn send(&mut self, email: SendableEmail) -> FileResult {
        let email_nobody =
            SendableEmailWithoutBody::new(email.envelope().clone(), email.message_id().to_string());

        let stream = self.send_stream(email_nobody, None).await?;

        copy(email.message(), stream).await?;
        Ok(())
    }

    async fn send_with_timeout(
        &mut self,
        email: SendableEmail,
        _timeout: Option<&Duration>,
    ) -> Self::Result {
        self.send(email).await // Writing to a file does not have a timeout, so just ignore it.
    }
}

impl Write for FileStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::result::Result<usize, std::io::Error>> {
        Pin::new(&mut self.file).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        Pin::new(&mut self.file).poll_flush(cx)
    }
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        Pin::new(&mut self.file).poll_close(cx)
    }
}
