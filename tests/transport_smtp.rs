#[cfg(test)]
#[cfg(feature = "smtp-transport")]
mod test {
    use async_smtp::{ClientSecurity, Envelope, SendableEmail, SmtpClient};

    #[async_attributes::test]
    async fn smtp_transport_simple() {
        let email = SendableEmail::new(
            Envelope::new(
                Some("user@localhost".parse().unwrap()),
                vec!["root@localhost".parse().unwrap()],
            )
            .unwrap(),
            "id",
            "From: user@localhost\r\n\
             Content-Type: text/plain\r\n\
             \r\n\
             Hello example",
        );

        println!("connecting");
        let mut transport = SmtpClient::with_security("127.0.0.1:2525", ClientSecurity::None)
            .await
            .unwrap()
            .into_transport();

        println!("sending");
        transport.connect_and_send(email).await.unwrap();
    }
}
