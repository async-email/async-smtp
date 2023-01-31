use tokio::net::TcpStream;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

use async_smtp::{Envelope, SendableEmail, SmtpClient, SmtpTransport};

#[tokio::main]
async fn main() -> Result<()> {
    let stream = TcpStream::connect("127.0.0.1:2525").await?;
    let client = SmtpClient::new();
    let mut transport = SmtpTransport::new(client, stream).await?;

    let email = SendableEmail::new(
        Envelope::new(
            Some("user@localhost".parse().unwrap()),
            vec!["root@localhost".parse().unwrap()],
        )?,
        "Hello world",
    );
    transport.send(email).await?;

    Ok(())
}
