use async_smtp::smtp::response::Response;
use async_smtp::{EmailAddress, Envelope, SendableEmail, SmtpClient};
use async_std::{io, io::Read, io::ReadExt, task};
use structopt::StructOpt;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

fn main() {
    env_logger::init();

    // Collect all inputs
    let opt = Opt::from_args();
    let id = "some_random_id";
    println!("Type your mail and finish with Ctrl+D:");

    // Send mail
    let result = task::block_on(send_mail(opt, id, io::stdin()));

    if let Ok(response) = result {
        println!("Email sent. Response: {:?}", response);
    } else {
        println!("Could not send email: {:?}", result);
    }
}

async fn send_mail(opt: Opt, id: &str, mut mail: impl Read + Unpin) -> Result<Response> {
    let mut body = vec![];
    mail.read_to_end(&mut body).await?;

    // Compose a mail
    let email = SendableEmail::new(Envelope::new(Some(opt.from), opt.to)?, id.to_string(), body);

    // Open an SMTP connection to given address
    let mut mailer = SmtpClient::unencrypted(opt.server).await?.into_transport();

    // Send the email
    let response = mailer.connect_and_send(email).await?;

    Ok(response)
}

#[derive(StructOpt, Debug)]
#[structopt(name = "smtp")]
struct Opt {
    /// Mail from
    #[structopt(short = "f", name = "sender address")]
    from: EmailAddress,

    /// Rcpt to, can be repeated multiple times
    #[structopt(short = "t", name = "recipient address", min_values = 1)]
    to: Vec<EmailAddress>,

    /// SMTP server address:port to talk to
    #[structopt(short = "s", name = "smtp server", default_value = "localhost:25")]
    server: String,
}
