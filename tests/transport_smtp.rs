#[cfg(test)]
#[cfg(feature = "smtp-transport")]
mod test {
    use async_smtp::{
        async_test_ignore, ClientSecurity, Envelope, SendableEmail, ServerAddress, SmtpClient,
    };

    // ignored as this needs a running server
    async_test_ignore! { smtp_transport_simple, {
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
    let mut transport = SmtpClient::with_security(
        ServerAddress {
            host: "127.0.0.1".to_string(),
            port: 3025,
        },
        ClientSecurity::None,
    )
    .into_transport();

    println!("sending");
    transport.connect_and_send(email).await.unwrap();
    }}
}
