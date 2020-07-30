//! The file transport writes the emails to the given directory. The name of the file will be
//! `message_id.txt`.
//! It can be useful for testing purposes, or if you want to keep track of sent messages.
//!

use std::path::PathBuf;

use async_std::fs::File;
use async_std::path::Path;
use async_std::prelude::*;
use async_trait::async_trait;

use crate::file::error::FileResult;
use crate::Envelope;
use crate::SendableEmail;
use crate::Transport;

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
    message: Vec<u8>,
}

#[async_trait]
impl<'a> Transport<'a> for FileTransport {
    type Result = FileResult;

    async fn send(&mut self, email: SendableEmail) -> FileResult {
        let message_id = email.message_id().to_string();
        let envelope = email.envelope().clone();

        let mut file = self.path.clone();
        file.push(format!("{}.json", message_id));

        let serialized = serde_json::to_string(&SerializableEmail {
            envelope,
            message_id,
            message: email.message_to_string().await?.as_bytes().to_vec(),
        })?;

        File::create(file)
            .await?
            .write_all(serialized.as_bytes())
            .await?;
        Ok(())
    }
}
