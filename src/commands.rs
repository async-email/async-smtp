//! SMTP commands

use crate::authentication::{Credentials, Mechanism};
use crate::error::Error;
use crate::extension::{ClientId, MailParameter, RcptParameter};
use crate::response::Response;
use crate::EmailAddress;
use log::debug;
use std::convert::AsRef;
use std::fmt::{self, Display, Formatter};

/// EHLO command
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct EhloCommand {
    client_id: ClientId,
}

impl Display for EhloCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "EHLO {}\r\n", self.client_id)
    }
}

impl EhloCommand {
    /// Creates a EHLO command
    pub fn new(client_id: ClientId) -> EhloCommand {
        EhloCommand { client_id }
    }
}

/// STARTTLS command
#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub struct StarttlsCommand;

impl Display for StarttlsCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("STARTTLS\r\n")
    }
}

/// MAIL command
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct MailCommand {
    sender: Option<EmailAddress>,
    parameters: Vec<MailParameter>,
}

impl Display for MailCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "MAIL FROM:<{}>",
            self.sender.as_ref().map(AsRef::as_ref).unwrap_or("")
        )?;
        for parameter in &self.parameters {
            write!(f, " {}", parameter)?;
        }
        f.write_str("\r\n")
    }
}

impl MailCommand {
    /// Creates a MAIL command
    pub fn new(sender: Option<EmailAddress>, parameters: Vec<MailParameter>) -> MailCommand {
        MailCommand { sender, parameters }
    }
}

/// RCPT command
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct RcptCommand {
    recipient: EmailAddress,
    parameters: Vec<RcptParameter>,
}

impl Display for RcptCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "RCPT TO:<{}>", self.recipient)?;
        for parameter in &self.parameters {
            write!(f, " {}", parameter)?;
        }
        f.write_str("\r\n")
    }
}

impl RcptCommand {
    /// Creates an RCPT command
    pub fn new(recipient: EmailAddress, parameters: Vec<RcptParameter>) -> RcptCommand {
        RcptCommand {
            recipient,
            parameters,
        }
    }
}

/// DATA command
#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub struct DataCommand;

impl Display for DataCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("DATA\r\n")
    }
}

/// QUIT command
#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub struct QuitCommand;

impl Display for QuitCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("QUIT\r\n")
    }
}

/// NOOP command
#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub struct NoopCommand;

impl Display for NoopCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("NOOP\r\n")
    }
}

/// HELP command
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct HelpCommand {
    argument: Option<String>,
}

impl Display for HelpCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("HELP")?;
        if let Some(arg) = &self.argument {
            write!(f, " {}", arg)?;
        }
        f.write_str("\r\n")
    }
}

impl HelpCommand {
    /// Creates an HELP command
    pub fn new(argument: Option<String>) -> HelpCommand {
        HelpCommand { argument }
    }
}

/// VRFY command
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct VrfyCommand {
    argument: String,
}

impl Display for VrfyCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "VRFY {}\r\n", self.argument)
    }
}

impl VrfyCommand {
    /// Creates a VRFY command
    pub fn new(argument: String) -> VrfyCommand {
        VrfyCommand { argument }
    }
}

/// EXPN command
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct ExpnCommand {
    argument: String,
}

impl Display for ExpnCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "EXPN {}\r\n", self.argument)
    }
}

impl ExpnCommand {
    /// Creates an EXPN command
    pub fn new(argument: String) -> ExpnCommand {
        ExpnCommand { argument }
    }
}

/// RSET command
#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub struct RsetCommand;

impl Display for RsetCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("RSET\r\n")
    }
}

/// AUTH command
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct AuthCommand {
    mechanism: Mechanism,
    credentials: Credentials,
    challenge: Option<String>,
    response: Option<String>,
}

impl Display for AuthCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let encoded_response = self
            .response
            .as_ref()
            .map(|r| base64::encode_config(r.as_bytes(), base64::STANDARD));

        if self.mechanism.supports_initial_response() {
            write!(
                f,
                "AUTH {} {}",
                self.mechanism,
                encoded_response.unwrap_or_default()
            )?;
        } else {
            match encoded_response {
                Some(response) => f.write_str(&response)?,
                None => write!(f, "AUTH {}", self.mechanism)?,
            }
        }
        f.write_str("\r\n")
    }
}

impl AuthCommand {
    /// Creates an AUTH command (from a challenge if provided)
    pub fn new(
        mechanism: Mechanism,
        credentials: Credentials,
        challenge: Option<String>,
    ) -> Result<AuthCommand, Error> {
        let response = if mechanism.supports_initial_response() || challenge.is_some() {
            Some(mechanism.response(&credentials, challenge.as_deref())?)
        } else {
            None
        };
        Ok(AuthCommand {
            mechanism,
            credentials,
            challenge,
            response,
        })
    }

    /// Creates an AUTH command from a response that needs to be a
    /// valid challenge (with 334 response code)
    pub fn new_from_response(
        mechanism: Mechanism,
        credentials: Credentials,
        response: &Response,
    ) -> Result<AuthCommand, Error> {
        if !response.has_code(334) {
            return Err(Error::ResponseParsing("Expecting a challenge"));
        }

        let encoded_challenge = response
            .first_word()
            .ok_or(Error::ResponseParsing("Could not read auth challenge"))?;
        debug!("auth encoded challenge: {}", encoded_challenge);

        let decoded_challenge = String::from_utf8(base64::decode(encoded_challenge)?)?;
        debug!("auth decoded challenge: {}", decoded_challenge);

        let response = Some(mechanism.response(&credentials, Some(decoded_challenge.as_ref()))?);

        Ok(AuthCommand {
            mechanism,
            credentials,
            challenge: Some(decoded_challenge),
            response,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::extension::MailBodyParameter;

    #[test]
    fn test_display() {
        let id = ClientId::Domain("localhost".to_string());
        let id_ipv4 = ClientId::Ipv4(std::net::Ipv4Addr::new(127, 0, 0, 1));
        let email = EmailAddress::new("test@example.com".to_string()).unwrap();
        let mail_parameter = MailParameter::Other {
            keyword: "TEST".to_string(),
            value: Some("value".to_string()),
        };
        let rcpt_parameter = RcptParameter::Other {
            keyword: "TEST".to_string(),
            value: Some("value".to_string()),
        };
        assert_eq!(format!("{}", EhloCommand::new(id)), "EHLO localhost\r\n");
        assert_eq!(
            format!("{}", EhloCommand::new(id_ipv4)),
            "EHLO [127.0.0.1]\r\n"
        );
        assert_eq!(
            format!("{}", MailCommand::new(Some(email.clone()), vec![])),
            "MAIL FROM:<test@example.com>\r\n"
        );
        assert_eq!(
            format!("{}", MailCommand::new(None, vec![])),
            "MAIL FROM:<>\r\n"
        );
        assert_eq!(
            format!(
                "{}",
                MailCommand::new(Some(email.clone()), vec![MailParameter::Size(42)])
            ),
            "MAIL FROM:<test@example.com> SIZE=42\r\n"
        );
        assert_eq!(
            format!(
                "{}",
                MailCommand::new(
                    Some(email.clone()),
                    vec![
                        MailParameter::Size(42),
                        MailParameter::Body(MailBodyParameter::EightBitMime),
                        mail_parameter,
                    ],
                )
            ),
            "MAIL FROM:<test@example.com> SIZE=42 BODY=8BITMIME TEST=value\r\n"
        );
        assert_eq!(
            format!("{}", RcptCommand::new(email.clone(), vec![])),
            "RCPT TO:<test@example.com>\r\n"
        );
        assert_eq!(
            format!("{}", RcptCommand::new(email, vec![rcpt_parameter])),
            "RCPT TO:<test@example.com> TEST=value\r\n"
        );
        assert_eq!(format!("{}", QuitCommand), "QUIT\r\n");
        assert_eq!(format!("{}", DataCommand), "DATA\r\n");
        assert_eq!(format!("{}", NoopCommand), "NOOP\r\n");
        assert_eq!(format!("{}", HelpCommand::new(None)), "HELP\r\n");
        assert_eq!(
            format!("{}", HelpCommand::new(Some("test".to_string()))),
            "HELP test\r\n"
        );
        assert_eq!(
            format!("{}", VrfyCommand::new("test".to_string())),
            "VRFY test\r\n"
        );
        assert_eq!(
            format!("{}", ExpnCommand::new("test".to_string())),
            "EXPN test\r\n"
        );
        assert_eq!(format!("{}", RsetCommand), "RSET\r\n");
        let credentials = Credentials::new("user".to_string(), "password".to_string());
        assert_eq!(
            format!(
                "{}",
                AuthCommand::new(Mechanism::Plain, credentials.clone(), None).unwrap()
            ),
            "AUTH PLAIN AHVzZXIAcGFzc3dvcmQ=\r\n"
        );
        assert_eq!(
            format!(
                "{}",
                AuthCommand::new(Mechanism::Login, credentials, None).unwrap()
            ),
            "AUTH LOGIN\r\n"
        );
    }
}
