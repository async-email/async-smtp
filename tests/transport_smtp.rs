#[cfg(test)]
#[cfg(feature = "smtp-transport")]
mod test {
    use async_smtp::{ClientSecurity, Envelope, SendableEmail, SmtpClient, Transport};

    #[async_attributes::test]
    async fn smtp_transport_simple() {
        let email = SendableEmail::new(
            Envelope::new(
                Some("user@localhost".parse().unwrap()),
                vec!["root@localhost".parse().unwrap()],
            )
            .unwrap(),
            "id",
            "Hello ß☺ example",
        );

        SmtpClient::with_security("127.0.0.1:2525", ClientSecurity::None)
            .await
            .unwrap()
            .into_transport()
            .send(email)
            .await
            .unwrap();
    }
}
