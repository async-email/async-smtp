use std::fmt::Display;
use std::time::Duration;

use async_std::net::{SocketAddr, ToSocketAddrs};
use async_std::pin::Pin;
use async_trait::async_trait;
use log::{debug, info};
use pin_project::pin_project;

use crate::smtp::authentication::{
    Credentials, Mechanism, DEFAULT_ENCRYPTED_MECHANISMS, DEFAULT_UNENCRYPTED_MECHANISMS,
};
use crate::smtp::client::net::ClientTlsParameters;
use crate::smtp::client::InnerClient;
use crate::smtp::commands::*;
use crate::smtp::error::{Error, SmtpResult};
use crate::smtp::extension::{ClientId, Extension, MailBodyParameter, MailParameter, ServerInfo};
use crate::{SendableEmail, Transport};

#[cfg(feature = "socks5")]
use crate::smtp::client::net::NetworkStream;
#[cfg(feature = "socks5")]
use async_std::{future, net::TcpStream};
#[cfg(feature = "socks5")]
use fast_socks5::client::{Config, Socks5Stream};

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

#[derive(Clone, Debug)]
pub struct ServerAddress {
    pub host: String,
    pub port: u16,
}

impl ServerAddress {
    pub fn new(host: String, port: u16) -> ServerAddress {
        ServerAddress { host, port }
    }
}

impl Display for ServerAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}


#[cfg(feature = "socks5")]
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Socks5Config {
    pub host: String,
    pub port: u16,
    pub user_password: Option<(String, String)>,
}


#[cfg(feature = "socks5")]
impl Socks5Config {
    pub fn new(host: String, port: u16) -> Self {
        Socks5Config {
            host,
            port,
            user_password: None
        }
    }

    pub fn new_with_user_pass(host: String, port: u16, user: String, password: String) -> Self {
        Socks5Config {
            host,
            port,
            user_password: Some((user, password))
        }
    }
    pub async fn connect(
        &self,
        target_addr: &ServerAddress,
        timeout: Duration,
    ) -> Result<Socks5Stream<TcpStream>, Error> {
        let socks_server = format!("{}:{}", self.host.clone(), self.port);
        println!("{}", socks_server);

        let socks_connection = if let Some((user, password)) = self.user_password.as_ref() {
            future::timeout(timeout, Socks5Stream::connect_with_password(
                socks_server,
                target_addr.host.clone(),
                target_addr.port,
                user.into(),
                password.into(),
                Config::default(),
            )).await
        } else {      
            future::timeout(timeout, Socks5Stream::connect(
                socks_server,
                target_addr.host.clone(),
                target_addr.port,
                Config::default(),
            )).await
        };

        match socks_connection? {
            Ok(socks5_stream) => Ok(socks5_stream),
            Err(e) => Err(Error::Socks5Error(e)),
        }
        
    

    }
}

#[cfg(feature = "socks5")]
impl Display for Socks5Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{} {}",
            self.host,
            self.port,
            if let Some((user, _password)) = self.user_password.as_ref() {
                format!("user: {} password: ***", user)
            } else {
                "".to_string()
            }
        )
    }
}




#[derive(Clone, Debug)]
#[allow(missing_copy_implementations)]
pub enum ConnectionType {
    Direct,

    #[cfg(feature = "socks5")]
    Socks5(Socks5Config),
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
    server_addr: ServerAddress,
    /// TLS security configuration
    connection_type: ConnectionType,
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
    pub fn with_security(server_addr: ServerAddress, security: ClientSecurity) -> SmtpClient {
        SmtpClient {
            server_addr,
            security,
            connection_type: ConnectionType::Direct,
            smtp_utf8: false,
            credentials: None,
            connection_reuse: ConnectionReuseParameters::NoReuse,
            hello_name: Default::default(),
            authentication_mechanism: None,
            force_set_auth: false,
            timeout: Some(Duration::new(60, 0)),
        }
    }

    /// Simple and secure transport, should be used when possible.
    /// Creates an encrypted transport over submissions port, using the provided domain
    /// to validate TLS certificates.
    pub fn new(domain: String) -> SmtpClient {
        SmtpClient::new_host_port(domain, SUBMISSIONS_PORT)
    }

    pub fn new_host_port(host: String, port: u16) -> SmtpClient {
        let tls = async_native_tls::TlsConnector::new();

        let tls_parameters = ClientTlsParameters::new(host.to_string(), tls);

        SmtpClient::with_security(
            ServerAddress::new(host, port),
            ClientSecurity::Wrapper(tls_parameters),
        )
    }

    /// Creates a new local SMTP client to port 25
    pub fn new_unencrypted_localhost() -> SmtpClient {
        SmtpClient::with_security(
            ServerAddress::new("localhost".to_string(), SMTP_PORT),
            ClientSecurity::None,
        )
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

    #[cfg(feature = "socks5")]
    pub fn use_socks5(mut self, socks5_config: Socks5Config) -> Self {
        self.connection_type = ConnectionType::Socks5(socks5_config);
        self
    }

    pub fn connection_type(mut self, connection_type: ConnectionType) -> Self {
        self.connection_type = connection_type;
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
    /// Connection reuse counter
    pub connection_reuse_count: u16,
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
    #[pin]
    client: InnerClient,
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
            client: InnerClient::new(),
            server_info: None,
            client_info: builder,
            state: State {
                panic: false,
                connection_reuse_count: 0,
            },
        }
    }

    /// Returns true if there is currently an established connection.
    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }

    /// Operations to perform right after the connection has been established
    async fn post_connect(&mut self) -> Result<(), Error> {
        // Log the connection
        debug!("connection established to {}", self.client_info.server_addr);

        self.ehlo().await?;

        self.try_tls().await?;

        if self.client_info.credentials.is_some() {
            self.try_login().await?;
        }

        Ok(())
    }

    pub async fn connect(&mut self) -> Result<(), Error> {
        match &self.client_info.connection_type {
            ConnectionType::Direct => self.connect_direct().await,

            #[cfg(feature = "socks5")]
            ConnectionType::Socks5(socks5) => {
                println!("Trying to connect with socks5...");
                let socks5_stream = socks5
                    .connect(
                        &self.client_info.server_addr,
                        self.client_info
                            .timeout
                            .unwrap_or_else(|| Duration::from_millis(100)),
                    )
                    .await?;
                println!("Connected through socks5");
                self.connect_with_stream(NetworkStream::Socks5Stream(socks5_stream))
                    .await
            }
        }
    }

    /// Try to connect, if not already connected.
    pub async fn connect_direct(&mut self) -> Result<(), Error> {
        // Check if the connection is still available
        if (self.state.connection_reuse_count > 0) && (!self.client.is_connected()) {
            self.close().await?;
        }

        if self.state.connection_reuse_count > 0 {
            debug!(
                "connection already established to {}",
                self.client_info.server_addr
            );
            return Ok(());
        }

        println!("{}", self.client_info.server_addr);

        // Perform dns lookup if needed
        let mut addresses = self.client_info.server_addr.to_string().to_socket_addrs().await?;

        match addresses.next() {
            Some(addr) => {
                let mut client = Pin::new(&mut self.client);
                client
                    .connect(
                        &addr,
                        self.client_info.timeout,
                        match self.client_info.security {
                            ClientSecurity::Wrapper(ref tls_parameters) => Some(tls_parameters),
                            _ => None,
                        },
                    )
                    .await?;

                client.set_timeout(self.client_info.timeout);
                let _response =
                    super::client::with_timeout(self.client_info.timeout.as_ref(), async {
                        client.read_response().await
                    })
                    .await?;
            }
            None => return Err(Error::Resolution),
        };

        self.post_connect().await
    }

    /// Try to connect to pre-defined stream, if not already connected.
    #[cfg(feature = "socks5")]
    pub async fn connect_with_stream(&mut self, stream: NetworkStream) -> Result<(), Error> {
        // Check if the connection is still available
        if (self.state.connection_reuse_count > 0) && (!self.client.is_connected()) {
            self.close().await?;
        }

        if self.state.connection_reuse_count > 0 {
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
            let _response = super::client::with_timeout(self.client_info.timeout.as_ref(), async {
                client.read_response().await
            })
            .await?;
        }

        self.post_connect().await
    }

    async fn try_login(&mut self) -> Result<(), Error> {
        let client = Pin::new(&mut self.client);
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
            let mut client = Pin::new(&mut self.client);

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
        match (
            &self.client_info.security,
            server_info.supports_feature(Extension::StartTls),
        ) {
            (&ClientSecurity::Required(_), false) => {
                Err(From::from("Could not encrypt connection, aborting"))
            }
            (&ClientSecurity::Opportunistic(_), false) => Ok(()),
            (&ClientSecurity::None, _) => Ok(()),
            (&ClientSecurity::Wrapper(_), _) => Ok(()),
            (&ClientSecurity::Opportunistic(ref tls_parameters), true)
            | (&ClientSecurity::Required(ref tls_parameters), true) => {
                {
                    let client = Pin::new(&mut self.client);
                    try_smtp!(client.command(StarttlsCommand).await, self);
                }

                let client = std::mem::take(&mut self.client);
                let ssl_client = client.upgrade_tls_stream(tls_parameters).await?;
                self.client = ssl_client;

                debug!("connection encrypted");

                // Send EHLO again
                self.ehlo().await.map(|_| ())
            }
        }
    }

    /// Send the given SMTP command to the server.
    pub async fn command<C: Display>(&mut self, command: C) -> SmtpResult {
        let mut client = Pin::new(&mut self.client);

        client.as_mut().command(command).await
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
        let client = Pin::new(&mut self.client);

        // Close the SMTP transaction if needed
        client.close().await?;

        // Reset the client state
        self.server_info = None;
        self.state.panic = false;
        self.state.connection_reuse_count = 0;

        Ok(())
    }

    fn supports_feature(&self, keyword: Extension) -> bool {
        self.server_info
            .as_ref()
            .map(|info| info.supports_feature(keyword))
            .unwrap_or_default()
    }

    /// Called after sending out a message, to update the connection state.
    async fn connection_was_used(&mut self) -> Result<(), Error> {
        // Test if we can reuse the existing connection
        match self.client_info.connection_reuse {
            ConnectionReuseParameters::ReuseLimited(limit)
                if self.state.connection_reuse_count >= limit =>
            {
                self.close().await?;
            }
            ConnectionReuseParameters::NoReuse => self.close().await?,
            _ => (),
        }

        Ok(())
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

    /// Sends an email
    async fn send(&mut self, email: SendableEmail) -> SmtpResult {
        let timeout = self.client.timeout().cloned();
        self.send_with_timeout(email, timeout.as_ref()).await
    }

    async fn send_with_timeout(
        &mut self,
        email: SendableEmail,
        timeout: Option<&Duration>,
    ) -> Self::Result {
        let message_id = email.message_id().to_string();

        // Mail
        let mut mail_options = vec![];

        if self.supports_feature(Extension::EightBitMime) {
            mail_options.push(MailParameter::Body(MailBodyParameter::EightBitMime));
        }

        if self.supports_feature(Extension::SmtpUtfEight) && self.client_info.smtp_utf8 {
            mail_options.push(MailParameter::SmtpUtfEight);
        }

        let mut client = Pin::new(&mut self.client);

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
            debug!("{}: to=<{}>", message_id, to_address);
        }

        // Data
        try_smtp!(client.as_mut().command(DataCommand).await, self);

        let res = client
            .as_mut()
            .message_with_timeout(email.message(), timeout)
            .await;

        // Message content
        if let Ok(result) = &res {
            // Increment the connection reuse counter
            self.state.connection_reuse_count += 1;

            // Log the message
            debug!(
                "{}: conn_use={}, status=sent ({})",
                message_id,
                self.state.connection_reuse_count,
                result.message.get(0).unwrap_or(&"no response".to_string())
            );
        }

        self.connection_was_used().await?;

        res
    }
}
