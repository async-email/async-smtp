#[cfg(test)]
#[cfg(feature = "smtp-transport")]
mod test {
    use async_smtp::stub::StubTransport;
    use async_smtp::{EmailAddress, Envelope, SendableEmail, Transport};

    #[async_attributes::test]
    async fn stub_transport() {
        let mut sender_ok = StubTransport::new_positive();
        let mut sender_ko = StubTransport::new(Err("fail".into()));
        let email_ok = SendableEmail::new(
            Envelope::new(
                Some(EmailAddress::new("user@localhost".to_string()).unwrap()),
                vec![EmailAddress::new("root@localhost".to_string()).unwrap()],
            )
            .unwrap(),
            "id".to_string(),
            "Hello ß☺ example".to_string().into_bytes(),
        );
        let email_ko = SendableEmail::new(
            Envelope::new(
                Some(EmailAddress::new("user@localhost".to_string()).unwrap()),
                vec![EmailAddress::new("root@localhost".to_string()).unwrap()],
            )
            .unwrap(),
            "id".to_string(),
            "Hello ß☺ example".to_string().into_bytes(),
        );

        Transport::send(&mut sender_ok, email_ok).await.unwrap();
        sender_ko.send(email_ko).await.unwrap_err();
    }
}
