//! The stub transport only logs message envelope and drops the content. It can be useful for
//! testing purposes.
//!

use async_trait::async_trait;
use log::info;

use crate::Transport;
use crate::{SendableEmail, SendableEmailWithoutBody};
use std::time::Duration;

/// This transport logs the message envelope and returns the given response
#[derive(Debug, Clone, Copy)]
pub struct StubTransport {
    response: StubResult,
}

impl StubTransport {
    /// Creates a new transport that always returns the given response
    pub fn new(response: StubResult) -> StubTransport {
        StubTransport { response }
    }

    /// Creates a new transport that always returns a success response
    pub fn new_positive() -> StubTransport {
        StubTransport { response: Ok(()) }
    }
}

/// SMTP result type
pub type StubResult = Result<(), ()>;

#[async_trait]
impl<'a> Transport<'a> for StubTransport {
    type Result = StubResult;
    type StreamResult = Result<Vec<u8>, ()>;

    async fn send_stream(
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
        Ok(vec![])
    }

    async fn send(&mut self, email: SendableEmail) -> StubResult {
        let email_nobody =
            SendableEmailWithoutBody::new(email.envelope().clone(), email.message_id().to_string());

        let _stream = self.send_stream(email_nobody, None).await?;
        self.response
    }
    async fn send_with_timeout(
        &mut self,
        email: SendableEmail,
        timeout: Option<&Duration>,
    ) -> Self::Result {
        info!("Timeout: {:?}", timeout);
        self.send(email).await
    }
}
