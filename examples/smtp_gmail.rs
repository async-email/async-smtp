use async_smtp::smtp::authentication::Credentials;
use async_smtp::{EmailAddress, Envelope, SendableEmail, SmtpClient};

fn main() {
    async_std::task::block_on(async move {
        let email = SendableEmail::new(
            Envelope::new(
                Some(EmailAddress::new("from@gmail.com".to_string()).unwrap()),
                vec![EmailAddress::new("to@example.com".to_string()).unwrap()],
            )
            .unwrap(),
            "id".to_string(),
            "Hello example".to_string().into_bytes(),
        );

        let creds = Credentials::new(
            "example_username".to_string(),
            "example_password".to_string(),
        );

        // Open a remote connection to gmail
        let mut mailer = SmtpClient::new("smtp.gmail.com".to_string())
            .credentials(creds)
            .into_transport();

        // Send the email
        let result = mailer.connect_and_send(email).await;

        if result.is_ok() {
            println!("Email sent");
        } else {
            println!("Could not send email: {:?}", result);
        }

        assert!(result.is_ok());
    });
}
