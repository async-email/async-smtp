use async_std::io::{Read, Write};
use async_std::net::{SocketAddr, ToSocketAddrs};
use async_std::pin::Pin;
use async_trait::async_trait;
use futures::channel::oneshot::{channel as oneshot, Receiver, Sender};
use futures::Future;
use log::{debug, info};
use pin_project::pin_project;
use std::fmt::Display;
use std::ops::DerefMut;
use std::time::Duration;

use crate::smtp::authentication::{
    Credentials, Mechanism, DEFAULT_ENCRYPTED_MECHANISMS, DEFAULT_UNENCRYPTED_MECHANISMS,
};
use crate::smtp::client::net::ClientTlsParameters;
#[cfg(feature = "socks5")]
use crate::smtp::client::net::NetworkStream;
use crate::smtp::client::InnerClient;
use crate::smtp::commands::*;
use crate::smtp::error::{Error, SmtpResult};
use crate::smtp::extension::{ClientId, Extension, MailBodyParameter, MailParameter, ServerInfo};
use crate::{SendableEmail, SendableEmailWithoutBody, Transport};

// Registered port numbers:
// https://www.iana.
// org/assignments/service-names-port-numbers/service-names-port-numbers.xhtml

/// Default smtp port
pub const SMTP_PORT: u16 = 25;
/// Default submission port
pub const SUBMISSION_PORT: u16 = 587;
/// Default submission over TLS port
pub const SUBMISSIONS_PORT: u16 = 465;

/// How to apply TLS to a client connection
#[derive(Debug)]
pub enum ClientSecurity {
    /// Insecure connection only (for testing purposes)
    None,
    /// Start with insecure connection and use `STARTTLS` when available
    Opportunistic(ClientTlsParameters),
    /// Start with insecure connection and require `STARTTLS`
    Required(ClientTlsParameters),
    /// Use TLS wrapped connection
    Wrapper(ClientTlsParameters),
}

/// Configures connection reuse behavior
#[derive(Clone, Debug, Copy)]
#[cfg_attr(
    feature = "serde-impls",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
pub enum ConnectionReuseParameters {
    /// Unlimited connection reuse
    ReuseUnlimited,
    /// Maximum number of connection reuse
    ReuseLimited(u16),
    /// Disable connection reuse, close connection after each transaction
    NoReuse,
}

/// Contains client configuration
#[derive(Debug)]
#[allow(missing_debug_implementations)]
pub struct SmtpClient {
    /// Enable connection reuse
    connection_reuse: ConnectionReuseParameters,
    /// Name sent during EHLO
    hello_name: ClientId,
    /// Credentials
    credentials: Option<Credentials>,
    /// Socket we are connecting to
    server_addr: SocketAddr,
    /// TLS security configuration
    security: ClientSecurity,
    /// Enable UTF8 mailboxes in envelope or headers
    smtp_utf8: bool,
    /// Optional enforced authentication mechanism
    authentication_mechanism: Option<Vec<Mechanism>>,
    /// Force use of the set authentication mechanism even if server does not report to support it
    force_set_auth: bool,
    /// Define network timeout
    /// It can be changed later for specific needs (like a different timeout for each SMTP command)
    timeout: Option<Duration>,
}

/// Builder for the SMTP `SmtpTransport`
impl SmtpClient {
    /// Creates a new SMTP client
    ///
    /// Defaults are:
    ///
    /// * No connection reuse
    /// * No authentication
    /// * No SMTPUTF8 support
    /// * A 60 seconds timeout for smtp commands
    ///
    /// Consider using [`SmtpClient::new_simple`] instead, if possible.
    pub async fn with_security<A: ToSocketAddrs>(
        addr: A,
        security: ClientSecurity,
    ) -> Result<SmtpClient, Error> {
        let mut addresses = addr.to_socket_addrs().await?;

        match addresses.next() {
            Some(addr) => Ok(SmtpClient {
                server_addr: addr,
                security,
                smtp_utf8: false,
                credentials: None,
                connection_reuse: ConnectionReuseParameters::NoReuse,
                hello_name: Default::default(),
                authentication_mechanism: None,
                force_set_auth: false,
                timeout: Some(Duration::new(60, 0)),
            }),
            None => Err(Error::Resolution),
        }
    }

    /// Simple and secure transport, should be used when possible.
    /// Creates an encrypted transport over submissions port, using the provided domain
    /// to validate TLS certificates.
    pub async fn new(domain: &str) -> Result<SmtpClient, Error> {
        let tls = async_native_tls::TlsConnector::new();

        let tls_parameters = ClientTlsParameters::new(domain.to_string(), tls);

        SmtpClient::with_security(
            (domain, SUBMISSIONS_PORT),
            ClientSecurity::Wrapper(tls_parameters),
        )
        .await
    }

    /// Creates a new local SMTP client to port 25
    pub async fn new_unencrypted_localhost() -> Result<SmtpClient, Error> {
        SmtpClient::with_security(("localhost", SMTP_PORT), ClientSecurity::None).await
    }

    /// Enable SMTPUTF8 if the server supports it
    pub fn smtp_utf8(mut self, enabled: bool) -> SmtpClient {
        self.smtp_utf8 = enabled;
        self
    }

    /// Set the name used during EHLO
    pub fn hello_name(mut self, name: ClientId) -> SmtpClient {
        self.hello_name = name;
        self
    }

    /// Enable connection reuse
    pub fn connection_reuse(mut self, parameters: ConnectionReuseParameters) -> SmtpClient {
        self.connection_reuse = parameters;
        self
    }

    /// Set the client credentials
    pub fn credentials<S: Into<Credentials>>(mut self, credentials: S) -> SmtpClient {
        self.credentials = Some(credentials.into());
        self
    }

    /// Set the authentication mechanism to use
    pub fn authentication_mechanism(mut self, mechanism: Vec<Mechanism>) -> SmtpClient {
        self.authentication_mechanism = Some(mechanism);
        self
    }

    /// Set if the set authentication mechanism should be force
    pub fn force_set_auth(mut self, force: bool) -> SmtpClient {
        self.force_set_auth = force;
        self
    }

    /// Set the timeout duration
    pub fn timeout(mut self, timeout: Option<Duration>) -> SmtpClient {
        self.timeout = timeout;
        self
    }

    /// Build the SMTP client
    ///
    /// It does not connect to the server, but only creates the `SmtpTransport`
    pub fn into_transport(self) -> SmtpTransport {
        SmtpTransport::new(self)
    }

    fn get_accepted_mechanism(&self, encrypted: bool) -> &[Mechanism] {
        match self.authentication_mechanism {
            Some(ref mechanism) => mechanism,
            None => {
                if encrypted {
                    DEFAULT_ENCRYPTED_MECHANISMS
                } else {
                    DEFAULT_UNENCRYPTED_MECHANISMS
                }
            }
        }
    }
}

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

    /// Try to connect, if not already connected.
    pub async fn connect(&mut self) -> Result<(), Error> {
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
            return Ok(());
        }

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
        let _response = super::client::with_timeout(self.client_info.timeout.as_ref(), async {
            client.as_mut().read_response().await
        })
        .await?;

        self.post_connect().await
    }

    /// Try to connect to pre-defined stream, if not already connected.
    #[cfg(feature = "socks5")]
    pub async fn connect_with_stream(&mut self, stream: NetworkStream) -> Result<(), Error> {
        // Check if the connection is still available
        if (self.client.connection_reuse_count > 0) && (!self.client.is_connected()) {
            self.close().await?;
        }

        if self.client.connection_reuse_count > 0 {
            debug!(
                "connection already established to {}",
                self.client_info.server_addr
            );
            return Ok(());
        }

        {
            let mut client = Pin::new(&mut self.client);
            client.connect_with_stream(stream).await?;

            client.set_timeout(self.client_info.timeout);
            let _response = client.read_response().await?;
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
        } else {
            if let Some(mechanisms) = self.client_info.authentication_mechanism.as_ref() {
                for mechanism in mechanisms {
                    if let Some(credentials) = &self.client_info.credentials {
                        try_smtp!(client.as_mut().auth(*mechanism, credentials).await, self);
                    }
                }
                found = true;
            } else {
                debug!("force_set_auth set to true, but no authentication mechanism set");
            }
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
impl<'a> Transport<'a> for SmtpTransport {
    type Result = SmtpResult;

    type StreamResult = Result<SmtpStream, Error>;

    async fn send_stream(
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

        Ok(SmtpStream {
            inner: self.client.lease().await.ok_or(Error::NoStream)?,
        })
    }

    /// Sends an email
    async fn send(&mut self, email: SendableEmail) -> SmtpResult {
        let timeout = self.client.map_present(|c| c.timeout().cloned()).flatten();
        self.send_with_timeout(email, timeout.as_ref()).await
    }

    async fn send_with_timeout(
        &mut self,
        email: SendableEmail,
        timeout: Option<&Duration>,
    ) -> SmtpResult {
        let message_id = email.message_id().to_string();
        let email_nobody =
            SendableEmailWithoutBody::new(email.envelope().clone(), email.message_id().to_string());
        self.send_stream(email_nobody, timeout).await?;

        let mut client = self.client().await?;
        let res = client
            .as_mut()
            .message_with_timeout(email.message(), timeout)
            .await;

        // Message content
        if let Ok(ref result) = res {
            // Log the message
            debug!(
                "{}: {}, status=sent ({})",
                message_id,
                client.debug_stats(),
                result.message.get(0).unwrap_or(&"no response".to_string())
            );
        }

        if !client.reuse() {
            self.close().await?;
        }

        res
    }
}

enum Potential<T> {
    Present(T),
    Eventual(Receiver<T>),
    Gone,
}
impl<T> Potential<T> {
    pub fn present(present: T) -> Self {
        Potential::Present(present)
    }
    pub fn gone() -> Self {
        Potential::Gone
    }
    pub fn eventual() -> (Sender<T>, Self) {
        let (sender, receiver) = oneshot();
        (sender, Potential::Eventual(receiver))
    }
    pub fn is_present(&self) -> bool {
        match self {
            Potential::Present(_) => true,
            _ => false,
        }
    }
    pub fn is_gone(&self) -> bool {
        match self {
            Potential::Gone => true,
            _ => false,
        }
    }
    pub fn is_eventual(&self) -> bool {
        match self {
            Potential::Eventual(_) => true,
            _ => false,
        }
    }
    pub async fn lease(&mut self) -> Option<Lease<T>> {
        match self.take().await {
            None => None,
            Some(present) => {
                let (sender, receiver) = oneshot();
                *self = Potential::Eventual(receiver);
                Some(Lease::new(present, sender))
            }
        }
    }
    pub async fn take(&mut self) -> Option<T> {
        match std::mem::take(self) {
            Potential::Gone => None,
            Potential::Present(present) => Some(present),
            Potential::Eventual(receiver) => receiver.await.ok(),
        }
    }
    pub async fn as_mut(&mut self) -> Option<&mut T> {
        match self {
            Potential::Gone => None,
            Potential::Present(ref mut present) => Some(present),
            Potential::Eventual(ref mut receiver) => match receiver.await.ok() {
                Some(present) => {
                    *self = Potential::Present(present);
                    if let Potential::Present(ref mut present) = self {
                        Some(present)
                    } else {
                        unreachable!("self is Present")
                    }
                }
                None => {
                    *self = Potential::Gone;
                    None
                }
            },
        }
    }
    pub fn map_present<F, U>(&self, map: F) -> Option<U>
    where
        F: FnOnce(&T) -> U,
    {
        match self {
            Potential::Present(ref t) => Some(map(t)),
            Potential::Eventual(_) | Potential::Gone => None,
        }
    }
}
impl<T> Default for Potential<T> {
    fn default() -> Self {
        Potential::Gone
    }
}

#[derive(Debug)]
struct Lease<T>(Option<T>, Option<Sender<T>>);

impl<T> Lease<T> {
    fn new(item: T, owner: Sender<T>) -> Self {
        Lease(Some(item), Some(owner))
    }
    async fn replace<F, Fut, E>(mut self, replacement: F) -> Result<Self, E>
    where
        F: FnOnce(T) -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let item = self.0.take().expect("item must be set");
        let item = replacement(item).await?;
        self.0 = Some(item);
        Ok(self)
    }
}
impl<T> std::ops::Deref for Lease<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("item must be set")
    }
}
impl<T> std::ops::DerefMut for Lease<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("item must be set")
    }
}
impl<T> Drop for Lease<T> {
    fn drop(&mut self) {
        // this may not hold after an error in replace()
        debug_assert!(self.0.is_some(), "item must be set");
        debug_assert!(self.1.is_some(), "owner must be set");
        if let Some(item) = self.0.take() {
            if let Some(owner) = self.1.take() {
                owner.send(item);
            }
        }
    }
}
#[allow(missing_debug_implementations)]
pub struct SmtpStream {
    inner: Lease<InnerClient>,
}

fn broken() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::NotConnected)
}

impl Write for SmtpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::result::Result<usize, std::io::Error>> {
        let stream = self.inner.deref_mut().stream.as_mut().ok_or(broken())?;
        Pin::new(stream).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        let stream = self.inner.deref_mut().stream.as_mut().ok_or(broken())?;
        Pin::new(stream).poll_flush(cx)
    }
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        // Defer close so that the connection can be reused.
        // Lease will send the inner client back on drop.
        //Pin::new(&mut self.inner).poll_close(cx)
        let stream = self.inner.deref_mut().stream.as_mut().ok_or(broken())?;
        Pin::new(stream).poll_flush(cx)
    }
}
