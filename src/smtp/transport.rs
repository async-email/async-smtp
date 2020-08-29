use crate::smtp::response::Response;
use crate::MailStream;
use async_trait::async_trait;
use futures::io::{AsyncWrite as Write, AsyncWriteExt as WriteExt};
use futures::{ready, Future};
use log::{debug, info};
use pin_project::pin_project;
use std::fmt::Display;
use std::ops::DerefMut;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::smtp::client::{ClientCodec, InnerClient};
use crate::smtp::commands::*;
use crate::smtp::error::{Error, SmtpResult};
use crate::smtp::extension::{ClientId, Extension, MailBodyParameter, MailParameter, ServerInfo};
use crate::smtp::potential::{Lease, Potential};
use crate::smtp::smtp_client::{ClientSecurity, SmtpClient};
use crate::{SendableEmail, SendableEmailWithoutBody, StreamingTransport, Transport};

#[cfg(feature = "socks5")]
use crate::smtp::client::net::NetworkStream;

/// Represents the state of a client
#[derive(Debug)]
struct State {
    /// Panic state
    pub panic: bool,
}

/// Structure that implements the high level SMTP client
#[pin_project]
#[allow(missing_debug_implementations)]
pub struct SmtpTransport {
    /// Information about the server
    /// Value is None before HELO/EHLO
    server_info: Option<ServerInfo>,
    /// SmtpTransport variable states
    state: State,
    /// Information about the client
    client_info: SmtpClient,
    /// Low level client
    client: Potential<InnerClient>,
}

macro_rules! try_smtp (
    ($err: expr, $client: ident) => ({
        match $err {
            Ok(val) => val,
            Err(err) => {
                if !$client.state.panic {
                    $client.state.panic = true;
                    $client.close().await?;
                }
                return Err(From::from(err))
            },
        }
    })
);

impl<'a> SmtpTransport {
    /// Creates a new SMTP client
    ///
    /// It does not connect to the server, but only creates the `SmtpTransport`
    pub fn new(builder: SmtpClient) -> SmtpTransport {
        SmtpTransport {
            client: Potential::present(InnerClient::new(builder.connection_reuse)),
            server_info: None,
            client_info: builder,
            state: State { panic: false },
        }
    }

    /// Borrow the inner client mutably in a Pin if available.
    /// reutns `Error::NoStream` ifnot available
    async fn client(&mut self) -> Result<Pin<&mut InnerClient>, Error> {
        Ok(Pin::new(self.client.as_mut().await.ok_or(Error::NoStream)?))
    }

    /// Returns true if there is currently an established connection.
    pub fn is_connected(&self) -> bool {
        self.client
            .map_present(InnerClient::is_connected)
            .unwrap_or_default()
    }

    /// Operations to perform right after the connection has been established
    async fn post_connect(&mut self) -> Result<(), Error> {
        // Log the connection
        debug!("connection established to {}", self.client_info.server_addr);

        self.ehlo().await?;

        self.try_tls().await?;

        self.try_login().await?;

        Ok(())
    }

    /// Returns OK(true) if the client is ready to use
    async fn check_connection(&mut self) -> Result<bool, Error> {
        // Check if the connection is alreadyset up and still available
        if self
            .client
            .map_present(|c| !c.can_be_reused())
            .unwrap_or_default()
        {
            self.close().await?;
        }

        if self
            .client
            .map_present(|c| c.has_been_used())
            .unwrap_or_default()
        {
            debug!(
                "connection already established to {}",
                self.client_info.server_addr
            );
            return Ok(true);
        }

        Ok(false)
    }

    /// Try to connect, if not already connected.
    pub async fn connect(&mut self) -> Result<(), Error> {
        if self.check_connection().await? {
            return Ok(());
        }
        {
            let mut client = self.client.lease().await.ok_or(Error::NoStream)?;
            let mut client = Pin::new(client.deref_mut());

            client
                .as_mut()
                .connect(
                    &self.client_info.server_addr,
                    self.client_info.timeout,
                    match self.client_info.security {
                        ClientSecurity::Wrapper(ref tls_parameters) => Some(tls_parameters),
                        _ => None,
                    },
                )
                .await?;

            client.set_timeout(self.client_info.timeout);
            let _response = client
                .as_mut()
                .read_response_with_timeout(self.client_info.timeout.as_ref())
                .await?;
        }
        self.post_connect().await
    }

    /// Try to connect to pre-defined stream, if not already connected.
    #[cfg(feature = "socks5")]
    pub async fn connect_with_stream(&mut self, stream: NetworkStream) -> Result<(), Error> {
        if self.check_connection().await? {
            return Ok(());
        }
        {
            let mut client = self.client.lease().await.ok_or(Error::NoStream)?;
            let mut client = Pin::new(client.deref_mut());

            client.as_mut().connect_with_stream(stream).await?;

            client.set_timeout(self.client_info.timeout);
            let _response = client.as_mut().read_response().await?;
        }
        self.post_connect().await
    }

    async fn try_login(&mut self) -> Result<(), Error> {
        if self.client_info.credentials.is_none() {
            return Ok(());
        }

        let mut client = self.client.lease().await.ok_or(Error::NoStream)?;
        let mut client = Pin::new(client.deref_mut());
        let mut found = false;

        if !self.client_info.force_set_auth {
            // Compute accepted mechanism
            let accepted_mechanisms = self
                .client_info
                .get_accepted_mechanism(client.is_encrypted());

            if let Some(server_info) = &self.server_info {
                if let Some(mechanism) = accepted_mechanisms
                    .iter()
                    .find(|mechanism| server_info.supports_auth_mechanism(**mechanism))
                {
                    found = true;

                    if let Some(credentials) = &self.client_info.credentials {
                        try_smtp!(client.auth(*mechanism, credentials).await, self);
                    }
                }
            } else {
                return Err(Error::NoServerInfo);
            }
        } else if let Some(mechanisms) = self.client_info.authentication_mechanism.as_ref() {
            for mechanism in mechanisms {
                if let Some(credentials) = &self.client_info.credentials {
                    try_smtp!(client.as_mut().auth(*mechanism, credentials).await, self);
                }
            }
            found = true;
        } else {
            debug!("force_set_auth set to true, but no authentication mechanism set");
        }

        if !found {
            info!("No supported authentication mechanisms available");
        }

        Ok(())
    }

    async fn try_tls(&mut self) -> Result<(), Error> {
        let server_info = self.server_info.as_ref().ok_or(Error::NoServerInfo)?;
        let mut client = self.client.lease().await.ok_or(Error::NoStream)?;
        match (
            &self.client_info.security,
            server_info.supports_feature(Extension::StartTls),
        ) {
            (ClientSecurity::Required(_), false) => {
                Err(From::from("Could not encrypt connection, aborting"))
            }
            (ClientSecurity::Opportunistic(_), false)
            | (ClientSecurity::None, _)
            | (ClientSecurity::Wrapper(_), _) => Ok(()),
            (ClientSecurity::Opportunistic(ref tls_parameters), true)
            | (ClientSecurity::Required(ref tls_parameters), true) => {
                {
                    try_smtp!(
                        Pin::new(client.deref_mut()).command(StarttlsCommand).await,
                        self
                    );
                }
                client
                    .replace(|c| c.upgrade_tls_stream(tls_parameters))
                    .await?;

                debug!("connection encrypted");

                // Send EHLO again
                self.ehlo().await.map(|_| ())
            }
        }
    }

    /// Send the given SMTP command to the server.
    pub async fn command<C: Display>(&mut self, command: C) -> SmtpResult {
        self.client().await?.command(command).await
    }

    /// Gets the EHLO response and updates server information.
    async fn ehlo(&mut self) -> SmtpResult {
        // Extended Hello
        let ehlo_response = try_smtp!(
            self.command(EhloCommand::new(ClientId::new(
                self.client_info.hello_name.to_string()
            )))
            .await,
            self
        );

        let server_info = try_smtp!(ServerInfo::from_response(&ehlo_response), self);

        // Print server information
        debug!("server {}", server_info);

        self.server_info = Some(server_info);

        Ok(ehlo_response)
    }

    /// Reset the client state and close the connection.
    pub async fn close(&mut self) -> Result<(), Error> {
        // Close the SMTP transaction if needed
        self.client().await?.close().await?;

        // Reset the client state
        self.server_info = None;
        self.state.panic = false;

        Ok(())
    }

    fn supports_feature(&self, keyword: Extension) -> bool {
        self.server_info
            .as_ref()
            .map(|info| info.supports_feature(keyword))
            .unwrap_or_default()
    }

    /// Try to connect and then send a message.
    pub async fn connect_and_send(&mut self, email: SendableEmail) -> SmtpResult {
        self.connect().await?;
        self.send(email).await
    }
}

#[async_trait]
impl StreamingTransport for SmtpTransport {
    type StreamResult = Result<SmtpStream, Error>;

    fn default_timeout(&self) -> Option<Duration> {
        self.client_info.timeout
    }
    async fn send_stream_with_timeout(
        &mut self,
        email: SendableEmailWithoutBody,
        timeout: Option<&Duration>,
    ) -> Self::StreamResult {
        // Mail
        let mut mail_options = vec![];

        if self.supports_feature(Extension::EightBitMime) {
            mail_options.push(MailParameter::Body(MailBodyParameter::EightBitMime));
        }

        if self.supports_feature(Extension::SmtpUtfEight) && self.client_info.smtp_utf8 {
            mail_options.push(MailParameter::SmtpUtfEight);
        }

        let mut client = self.client().await?;

        try_smtp!(
            client
                .as_mut()
                .command_with_timeout(
                    MailCommand::new(email.envelope().from().cloned(), mail_options),
                    timeout
                )
                .await,
            self
        );

        // Recipient
        for to_address in email.envelope().to() {
            try_smtp!(
                client
                    .as_mut()
                    .command_with_timeout(RcptCommand::new(to_address.clone(), vec![]), timeout)
                    .await,
                self
            );
            // Log the rcpt command
            debug!("{}: to=<{}>", email.message_id(), to_address);
        }

        // Data
        try_smtp!(client.as_mut().command(DataCommand).await, self);

        Ok(SmtpStream::new(
            self.client.lease().await.ok_or(Error::NoStream)?,
            email.message_id().to_string(),
            timeout.cloned(),
        ))
    }
}

#[allow(missing_debug_implementations)]
pub enum SmtpStream {
    Busy,
    Ready(SmtpStreamInner),
    Encoding(Pin<Box<dyn Future<Output = std::io::Result<SmtpStreamInner>> + Send>>),
    Closing(Pin<Box<dyn Future<Output = Result<Response, Error>> + Send>>),
    Done(Result<Response, Error>),
}
#[allow(missing_debug_implementations)]
pub struct SmtpStreamInner {
    inner: Lease<InnerClient>,
    codec: ClientCodec,
    message_id: String,
    timeout: Option<Duration>,
}

impl SmtpStream {
    fn new(inner: Lease<InnerClient>, message_id: String, timeout: Option<Duration>) -> Self {
        SmtpStream::Ready(SmtpStreamInner {
            inner,
            codec: ClientCodec::new(),
            message_id,
            timeout,
        })
    }
}

impl MailStream for SmtpStream {
    type Output = Response;
    type Error = Error;
    fn result(self) -> Result<Self::Output, Self::Error> {
        match self {
            SmtpStream::Done(result) => result,
            _ => Err(Error::Client("Mail sending was not completed properly")),
        }
    }
}

impl Write for SmtpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        loop {
            break match std::mem::replace(self.deref_mut(), SmtpStream::Busy) {
                SmtpStream::Ready(SmtpStreamInner {
                    mut inner,
                    mut codec,
                    message_id,
                    timeout,
                }) => {
                    let len = buf.len();
                    let buf = Vec::from(buf);
                    let fut = async move {
                        codec
                            .encode(&buf[..], inner.deref_mut().stream.as_mut().ok_or_else(broken)?)
                            .await?;
                        Ok(SmtpStreamInner {
                            inner,
                            codec,
                            message_id,
                            timeout,
                        })
                    };
                    *self = SmtpStream::Encoding(Box::pin(fut));
                    Poll::Ready(Ok(len))
                }
                otherwise => {
                    *self = otherwise;
                    ready!(self.as_mut().poll_flush(cx))?;
                    continue;
                }
            };
        }
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        loop {
            break match self.deref_mut() {
                SmtpStream::Ready(ref mut inner) => {
                    Pin::new(inner.inner.deref_mut().stream.as_mut().ok_or_else(broken)?)
                        .poll_flush(cx)
                }
                SmtpStream::Encoding(ref mut fut) => match fut.as_mut().poll(cx)? {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(inner) => {
                        *self = SmtpStream::Ready(inner);
                        continue;
                    }
                },
                SmtpStream::Closing(ref mut fut) => match fut.as_mut().poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(result) => {
                        *self = SmtpStream::Done(result);
                        continue;
                    }
                },
                SmtpStream::Done(Ok(_)) => Poll::Ready(Ok(())),
                SmtpStream::Done(Err(_)) => Poll::Ready(Err(broken())),
                SmtpStream::Busy => Poll::Ready(Err(broken())),
            };
        }
    }
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        // Defer close so that the connection can be reused.
        // Lease will send the inner client back on drop.
        // Here we take care of closing the stream with final dot
        // and reading the response
        loop {
            break match std::mem::replace(self.deref_mut(), SmtpStream::Busy) {
                SmtpStream::Ready(SmtpStreamInner {
                    mut inner,
                    mut codec,
                    message_id,
                    timeout,
                }) => {
                    let fut = async move {
                        let mut stream =
                            inner.deref_mut().stream.as_mut().ok_or(Error::NoStream)?;
                        // write final dot
                        codec.encode(&[][..], &mut stream).await?;
                        // make sure all is in before reading response
                        stream.flush().await?;
                        // collect response
                        let response = Pin::new(inner.deref_mut())
                            .read_response_with_timeout(timeout.as_ref())
                            .await;

                        // Log message sent
                        if let Ok(ref result) = response {
                            // Log the message
                            debug!(
                                "{}: {}, status=sent ({})",
                                message_id,
                                inner.debug_stats(),
                                result.message.get(0).unwrap_or(&"no response".to_string())
                            );
                        }

                        if !inner.reuse() {
                            let stream =
                                inner.deref_mut().stream.as_mut().ok_or(Error::NoStream)?;
                            stream.close().await?;
                        }
                        response
                    };
                    *self = SmtpStream::Closing(Box::pin(fut));
                    continue;
                }
                otherwise @ SmtpStream::Encoding(_) | otherwise @ SmtpStream::Closing(_) => {
                    *self = otherwise;
                    ready!(self.as_mut().poll_flush(cx))?;
                    continue;
                }
                otherwise @ SmtpStream::Done(_) | otherwise @ SmtpStream::Busy => {
                    *self = otherwise;
                    self.as_mut().poll_flush(cx)
                }
            };
        }
    }
}

fn broken() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::NotConnected)
}
