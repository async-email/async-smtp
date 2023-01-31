<h1 align="center">async-smtp</h1>
<div align="center">
 <strong>
   Async implementation of SMTP
 </strong>
</div>

<br />

<div align="center">
  <!-- Crates version -->
  <a href="https://crates.io/crates/async-smtp">
    <img src="https://img.shields.io/crates/v/async-smtp.svg?style=flat-square"
    alt="Crates.io version" />
  </a>
  <!-- Downloads -->
  <a href="https://crates.io/crates/async-smtp">
    <img src="https://img.shields.io/crates/d/async-smtp.svg?style=flat-square"
      alt="Download" />
  </a>
  <!-- docs.rs docs -->
  <a href="https://docs.rs/async-smtp">
    <img src="https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square"
      alt="docs.rs docs" />
  </a>
  <!-- CI -->
  <a href="https://github.com/async-email/async-smtp/actions">
    <img src="https://github.com/async-email/async-smtp/workflows/CI/badge.svg"
      alt="CI status" />
  </a>
</div>

<div align="center">
  <h3>
    <a href="https://docs.rs/async-smtp">
      API Docs
    </a>
    <span> | </span>
    <a href="https://github.com/async-email/async-smtp/releases">
      Releases
    </a>
  </h3>
</div>

<br/>

> Based on the great [lettre](https://crates.io/crates/lettre) library.

## Example

```rust
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

use async_smtp::{Envelope, SendableEmail, SmtpClient, SmtpTransport};

async fn smtp_transport_simple() -> Result<()> {
    let stream = TcpStream::connect("127.0.0.1:2525").await?;
    let client = SmtpClient::new();
    let mut transport = SmtpTransport::new(client, stream).await?;

    let email = SendableEmail::new(
        Envelope::new(
            Some("user@localhost".parse().unwrap()),
            vec!["root@localhost".parse().unwrap()],
        )?,
        "Hello world",
    );
    transport.send(email).await?;

    Ok(())
}
```

## License

Licensed under either of
 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
