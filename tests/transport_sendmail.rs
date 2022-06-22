#[cfg(test)]
#[cfg(feature = "sendmail-transport")]
mod test {
    use async_smtp::sendmail::SendmailTransport;
    use async_smtp::{async_test, EmailAddress, Envelope, SendableEmail, Transport};

    async_test! {
        sendmail_transport_simple,
        {
            let mut sender = SendmailTransport::new();
            let email = SendableEmail::new(
                Envelope::new(
                    Some(EmailAddress::new("user@localhost".to_string()).unwrap()),
                    vec![EmailAddress::new("root@localhost".to_string()).unwrap()],
                )
                    .unwrap(),
                "id".to_string(),
                "Hello ß☺ example".to_string().into_bytes(),
            );

            let result = sender.send(email).await;
            println!("{:?}", result);
            assert!(result.is_ok());
        }
    }
}
