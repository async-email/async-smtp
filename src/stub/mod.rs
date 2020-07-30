//! The stub transport only logs message envelope and drops the content. It can be useful for
//! testing purposes.
//!

use async_trait::async_trait;
use log::info;

use crate::SendableEmail;
use crate::Transport;
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

    async fn send(&mut self, email: SendableEmail) -> StubResult {
        info!(
            "{}: from=<{}> to=<{:?}>",
            email.message_id(),
            match email.envelope().from() {
                Some(address) => address.to_string(),
                None => "".to_string(),
            },
            email.envelope().to()
        );
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
