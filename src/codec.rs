#[cfg(feature = "runtime-async-std")]
use async_std::io::{Write, WriteExt};
#[cfg(feature = "runtime-tokio")]
use tokio::io::{AsyncWrite as Write, AsyncWriteExt};

use futures::io;

/// The codec used for transparency
#[derive(Default, Clone, Copy, Debug)]
pub struct ClientCodec {
    escape_count: u8,
}

impl ClientCodec {
    /// Creates a new client codec
    pub fn new() -> Self {
        ClientCodec::default()
    }
}

impl ClientCodec {
    /// Adds transparency
    /// TODO: replace CR and LF by CRLF
    #[allow(clippy::bool_to_int_with_if)]
    pub async fn encode<W: Write + Unpin>(&mut self, frame: &[u8], mut buf: W) -> io::Result<()> {
        match frame.len() {
            0 => {
                match self.escape_count {
                    0 => buf.write_all(b"\r\n.\r\n").await?,
                    1 => buf.write_all(b"\n.\r\n").await?,
                    2 => buf.write_all(b".\r\n").await?,
                    _ => unreachable!(),
                }
                self.escape_count = 0;
                Ok(())
            }
            _ => {
                let mut start = 0;
                for (idx, byte) in frame.iter().enumerate() {
                    match self.escape_count {
                        0 => self.escape_count = if *byte == b'\r' { 1 } else { 0 },
                        1 => self.escape_count = if *byte == b'\n' { 2 } else { 0 },
                        2 => {
                            self.escape_count = if *byte == b'.' {
                                3
                            } else if *byte == b'\r' {
                                1
                            } else {
                                0
                            }
                        }
                        _ => unreachable!(),
                    }
                    if self.escape_count == 3 {
                        self.escape_count = 0;
                        buf.write_all(&frame[start..idx]).await?;
                        buf.write_all(b".").await?;
                        start = idx;
                    }
                }
                buf.write_all(&frame[start..]).await?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::async_test;

    async_test! { test_codec, {
        let mut codec = ClientCodec::new();
        let mut buf: Vec<u8> = vec![];

        assert!(codec.encode(b"test\r\n", &mut buf).await.is_ok());
        assert!(codec.encode(b".\r\n", &mut buf).await.is_ok());
        assert!(codec.encode(b"\r\ntest", &mut buf).await.is_ok());
        assert!(codec.encode(b"te\r\n.\r\nst", &mut buf).await.is_ok());
        assert!(codec.encode(b"test", &mut buf).await.is_ok());
        assert!(codec.encode(b"test.", &mut buf).await.is_ok());
        assert!(codec.encode(b"test\n", &mut buf).await.is_ok());
        assert!(codec.encode(b".test\n", &mut buf).await.is_ok());
        assert!(codec.encode(b"test", &mut buf).await.is_ok());
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "test\r\n..\r\n\r\ntestte\r\n..\r\nsttesttest.test\n.test\ntest"
        );
    }}
}
