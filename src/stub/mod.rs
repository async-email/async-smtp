//! The stub transport only logs message envelope and drops the content. It can be useful for
//! testing purposes.
//!

pub mod error;

use async_std::io::Write;
use async_trait::async_trait;
use log::info;
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::stub::error::{Error, StubResult};
use crate::{MailStream, SendableEmailWithoutBody, StreamingTransport};

/// This transport logs the message envelope and returns the given response
#[derive(Debug)]
pub struct StubTransport {
    responses: VecDeque<StubResult>,
}

impl StubTransport {
    /// Creates a new transport that always returns the given response
    pub fn new(response: StubResult) -> StubTransport {
        StubTransport {
            responses: vec![response].into(),
        }
    }

    /// Creates a new transport that always returns a success response
    pub fn new_positive() -> StubTransport {
        StubTransport {
            responses: vec![Ok(())].into(),
        }
    }
}

#[async_trait]
impl StreamingTransport for StubTransport {
    type StreamResult = Result<StubStream, Error>;

    async fn send_stream_with_timeout(
        &mut self,
        email: SendableEmailWithoutBody,
        _timeout: Option<&Duration>,
    ) -> Self::StreamResult {
        info!(
            "{}: from=<{}> to=<{:?}>",
            email.message_id(),
            match email.envelope().from() {
                Some(address) => address.to_string(),
                None => "".to_string(),
            },
            email.envelope().to()
        );
        let response = self
            .responses
            .pop_front()
            .ok_or(Error::Client("There's nothing left to say. Hug a tree..."))?;
        Ok(StubStream { response })
    }
    /// Get the default timeout for this transport
    fn default_timeout(&self) -> Option<Duration> {
        None
    }
}

#[derive(Debug)]
pub struct StubStream {
    response: StubResult,
}

impl MailStream for StubStream {
    type Output = ();
    type Error = Error;
    fn result(self) -> StubResult {
        info!("Done: {:?}", self.response);
        self.response
    }
}

impl Write for StubStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        info!("Writing {} bytes", buf.len());
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        info!("Flushing");
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        info!("Closing");
        Poll::Ready(Ok(()))
    }
}
