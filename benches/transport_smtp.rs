use async_smtp::{ClientSecurity, EmailAddress, Envelope, SendableEmail, ServerAddress, SmtpClient, Transport, smtp::ConnectionReuseParameters};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

const SERVER_ADDR: &str = "127.0.0.1";
const SERVER_PORT: u16 = 2525;

fn bench_simple_send(c: &mut Criterion) {
    let mut sender = async_std::task::block_on(async move {
        SmtpClient::with_security(ServerAddress::new(SERVER_ADDR.to_string(), SERVER_PORT), ClientSecurity::None)
    })
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
        SmtpClient::with_security(ServerAddress::new(SERVER_ADDR.to_string(), SERVER_PORT), ClientSecurity::None)
    })
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
