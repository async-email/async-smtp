use std::fmt::Debug;

use log::{debug, info};

use crate::authentication::{Credentials, Mechanism};
use crate::commands::*;
use crate::error::{Error, SmtpResult};
use crate::extension::{ClientId, Extension, MailBodyParameter, MailParameter, ServerInfo};
use crate::stream::SmtpStream;
use crate::SendableEmail;

#[cfg(feature = "runtime-async-std")]
use async_std::io::{BufRead, Write};
#[cfg(feature = "runtime-tokio")]
use tokio::io::{AsyncBufRead as BufRead, AsyncWrite as Write};

/// Contains client configuration
#[derive(Debug)]
pub struct SmtpClient {
    /// Name sent during EHLO
    hello_name: ClientId,
    /// Enable UTF8 mailboxes in envelope or headers
    smtp_utf8: bool,
    /// Whether to expect greeting.
    /// Normally the server sends a greeting after connection,
    /// but not after STARTTLS.
    expect_greeting: bool,
    /// Use pipelining if the server supports it
    pipelining: bool,
}

impl Default for SmtpClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for the SMTP `SmtpTransport`
impl SmtpClient {
    /// Creates a new SMTP client.
    ///
    /// It does not connect to the server, but only creates the `SmtpTransport`.
    ///
    /// Defaults are:
    ///
    /// * No authentication
    /// * No SMTPUTF8 support
    pub fn new() -> Self {
        SmtpClient {
            smtp_utf8: false,
            hello_name: Default::default(),
            expect_greeting: true,
            pipelining: true,
        }
    }

    /// Enable SMTPUTF8 if the server supports it
    pub fn smtp_utf8(self, enabled: bool) -> SmtpClient {
        Self {
            smtp_utf8: enabled,
            ..self
        }
    }

    /// Enable PIPELINING if the server supports it
    pub fn pipelining(self, enabled: bool) -> SmtpClient {
        Self {
            pipelining: enabled,
            ..self
        }
    }

    /// Set the name used during EHLO
    pub fn hello_name(self, name: ClientId) -> SmtpClient {
        Self {
            hello_name: name,
            ..self
        }
    }

    /// Do not expect greeting.
    ///
    /// Could be used for STARTTLS connections.
    pub fn without_greeting(self) -> SmtpClient {
        Self {
            expect_greeting: false,
            ..self
        }
    }
}

/// Structure that implements the high level SMTP client
#[derive(Debug)]
pub struct SmtpTransport<S: BufRead + Write + Unpin> {
    /// Information about the server
    server_info: ServerInfo,
    /// Information about the client
    client_info: SmtpClient,
    /// Low level client
    stream: SmtpStream<S>,
}

impl<S: BufRead + Write + Unpin> SmtpTransport<S> {
    /// Creates a new SMTP transport and connects.
    pub async fn new(builder: SmtpClient, stream: S) -> Result<Self, Error> {
        let mut stream = SmtpStream::new(stream);
        if builder.expect_greeting {
            let _greeting = stream.read_response().await?;
        }
        let ehlo_response = stream
            .ehlo(ClientId::new(builder.hello_name.to_string()))
            .await?;
        let server_info = ServerInfo::from_response(&ehlo_response)?;

        // Print server information
        debug!("server {}", server_info);

        let transport = SmtpTransport {
            server_info,
            client_info: builder,
            stream,
        };
        Ok(transport)
    }

    /// Try to login with the given accepted mechanisms.
    pub async fn try_login(
        &mut self,
        credentials: &Credentials,
        accepted_mechanisms: &[Mechanism],
    ) -> Result<(), Error> {
        if let Some(mechanism) = accepted_mechanisms
            .iter()
            .find(|mechanism| self.server_info.supports_auth_mechanism(**mechanism))
        {
            self.auth(*mechanism, credentials).await?;
        } else {
            info!("No supported authentication mechanisms available");
        }

        Ok(())
    }

    /// Sends STARTTLS command if the server supports it.
    ///
    /// Returns inner stream which should be upgraded to TLS.
    pub async fn starttls(mut self) -> Result<S, Error> {
        if !self.supports_feature(Extension::StartTls) {
            return Err(From::from("server does not support STARTTLS"));
        }

        self.stream.command(StarttlsCommand).await?;

        // Return the stream, so the caller can upgrade it to TLS.
        Ok(self.stream.into_inner())
    }

    fn supports_feature(&self, keyword: Extension) -> bool {
        self.server_info.supports_feature(keyword)
    }

    /// Closes the SMTP transaction if possible.
    pub async fn quit(&mut self) -> Result<(), Error> {
        self.stream.command(QuitCommand).await?;

        Ok(())
    }

    /// Sends an AUTH command with the given mechanism, and handles challenge if needed
    pub async fn auth(&mut self, mechanism: Mechanism, credentials: &Credentials) -> SmtpResult {
        // TODO
        let mut challenges = 10;
        let mut response = self
            .stream
            .command(AuthCommand::new(mechanism, credentials.clone(), None)?)
            .await?;

        while challenges > 0 && response.has_code(334) {
            challenges -= 1;
            response = self
                .stream
                .command(AuthCommand::new_from_response(
                    mechanism,
                    credentials.clone(),
                    &response,
                )?)
                .await?;
        }

        if challenges == 0 {
            Err(Error::ResponseParsing("Unexpected number of challenges"))
        } else {
            Ok(response)
        }
    }

    /// Sends an email.
    pub async fn send(&mut self, email: SendableEmail) -> SmtpResult {
        // Mail
        let mut mail_options = vec![];

        if self.supports_feature(Extension::EightBitMime) {
            mail_options.push(MailParameter::Body(MailBodyParameter::EightBitMime));
        }

        if self.supports_feature(Extension::SmtpUtfEight) && self.client_info.smtp_utf8 {
            mail_options.push(MailParameter::SmtpUtfEight);
        }

        let pipelining =
            self.supports_feature(Extension::Pipelining) && self.client_info.pipelining;

        if pipelining {
            self.stream
                .send_command(MailCommand::new(
                    email.envelope().from().cloned(),
                    mail_options,
                ))
                .await?;
            let mut sent_commands = 1;

            // Recipient
            for to_address in email.envelope().to() {
                self.stream
                    .send_command(RcptCommand::new(to_address.clone(), vec![]))
                    .await?;
                sent_commands += 1;
            }

            // Data
            self.stream.send_command(DataCommand).await?;
            sent_commands += 1;

            for _ in 0..sent_commands {
                self.stream.read_response().await?;
            }
        } else {
            self.stream
                .command(MailCommand::new(
                    email.envelope().from().cloned(),
                    mail_options,
                ))
                .await?;

            // Recipient
            for to_address in email.envelope().to() {
                self.stream
                    .command(RcptCommand::new(to_address.clone(), vec![]))
                    .await?;
                // Log the rcpt command
                debug!("to=<{}>", to_address);
            }

            // Data
            self.stream.command(DataCommand).await?;
        }

        let res = self.stream.message(email.message()).await;

        // Message content
        if let Ok(result) = &res {
            // Log the message
            debug!(
                "status=sent ({})",
                result.message.get(0).unwrap_or(&"no response".to_string())
            );
        }

        res
    }
}
