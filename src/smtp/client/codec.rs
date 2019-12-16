use async_std::io::{self, Write};
use async_std::prelude::*;

/// The codec used for transparency
#[derive(Default, Clone, Copy, Debug)]
#[cfg_attr(
    feature = "serde-impls",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
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
                        2 => self.escape_count = if *byte == b'.' { 3 } else { 0 },
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
