#[cfg(test)]
#[cfg(feature = "smtp-transport")]
mod test {
    use async_smtp::stub::StubTransport;
    use async_smtp::{async_test, EmailAddress, Envelope, SendableEmail, Transport};

    async_test! { stub_transport, {
        let mut sender_ok = StubTransport::new_positive();
        let mut sender_ko = StubTransport::new(Err(()));
        let email_ok = SendableEmail::new(
            Envelope::new(
                Some(EmailAddress::new("user@localhost".to_string()).unwrap()),
                vec![EmailAddress::new("root@localhost".to_string()).unwrap()],
            )
            .unwrap(),
            "id",
            "Hello ß☺ example".to_string().into_bytes(),
        );
        let email_ko = SendableEmail::new(
            Envelope::new(
                Some(EmailAddress::new("user@localhost".to_string()).unwrap()),
                vec![EmailAddress::new("root@localhost".to_string()).unwrap()],
            )
            .unwrap(),
            "id",
            "Hello ß☺ example".to_string().into_bytes(),
        );

        sender_ok.send(email_ok).await.unwrap();
        sender_ko.send(email_ko).await.unwrap_err();
    }}
}
