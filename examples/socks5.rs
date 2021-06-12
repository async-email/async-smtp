use async_smtp::smtp::authentication::Credentials;
use async_smtp::{EmailAddress, Envelope, SendableEmail, SmtpClient};
use async_smtp::smtp::Socks5Config;
use anyhow;

fn main() -> Result<(), anyhow::Error> {
    env_logger::init();
    async_std::task::block_on(async move {
        let creds = Credentials::new("user".to_string(), "pass".to_string());
        let socks5_config = Socks5Config::new("127.0.0.1".to_string(), 9150);
        let mut transport  = SmtpClient::new_host_port(
                "xc7tgk2c5onxni2wsy76jslfsitxjbbptejnqhw6gy2ft7khpevhc7ad.onion".to_string(),
                25
            )
            .use_socks5(socks5_config)
            .credentials(creds)
            .into_transport();

        
        let email = SendableEmail::new(
            Envelope::new(
                Some(EmailAddress::new("from@mail2tor.com".to_string()).unwrap()),
                vec![EmailAddress::new("to@mail2tor.com".to_string()).unwrap()],
            )
            .unwrap(),
            "id".to_string(),
            "Hello ß☺ example".to_string().into_bytes(),
        );

        let result = transport.connect_and_send(email).await;

        if result.is_ok() {
            println!("Email sent");
        } else {
            println!("Could not send email: {:?}", result);
        }
        
        assert!(result.is_ok());
        
        Ok::<(), anyhow::Error>(())
    })
}
