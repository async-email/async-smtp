use async_smtp::{
    smtp::ConnectionReuseParameters, ClientSecurity, EmailAddress, Envelope, SendableEmail,
    SmtpClient, Transport,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

const SERVER: &str = "127.0.0.1:2525";

fn bench_simple_send(c: &mut Criterion) {
    let mut sender = async_std::task::block_on(async move {
        SmtpClient::with_security(SERVER, ClientSecurity::None).await
    })
    .unwrap()
    .into_transport();

    c.bench_function("send email", move |b| {
        b.iter(|| {
            let email = SendableEmail::new(
                Envelope::new(
                    Some(EmailAddress::new("user@localhost".to_string()).unwrap()),
                    vec![EmailAddress::new("root@localhost".to_string()).unwrap()],
                )
                .unwrap(),
                "id".to_string(),
                "From: user@localhost\r\n\
                 Content-Type: text/plain\r\n\
                 \r\n\
                 Hello example",
            );
            let result = black_box(async_std::task::block_on(async {
                sender.send(email).await
            }));
            result.unwrap();
        })
    });
}

fn bench_reuse_send(c: &mut Criterion) {
    let mut sender = async_std::task::block_on(async move {
        SmtpClient::with_security(SERVER, ClientSecurity::None).await
    })
    .unwrap()
    .connection_reuse(ConnectionReuseParameters::ReuseUnlimited)
    .into_transport();
    c.bench_function("send email with connection reuse", move |b| {
        b.iter(|| {
            let email = SendableEmail::new(
                Envelope::new(
                    Some(EmailAddress::new("user@localhost".to_string()).unwrap()),
                    vec![EmailAddress::new("root@localhost".to_string()).unwrap()],
                )
                .unwrap(),
                "id".to_string(),
                "From: user@localhost\r\n\
                 Content-Type: text/plain\r\n\
                 \r\n\
                 Hello example",
            );
            let result = black_box(async_std::task::block_on(async {
                sender.send(email).await
            }));
            result.unwrap();
        })
    });
}

criterion_group!(benches, bench_simple_send, bench_reuse_send);
criterion_main!(benches);
