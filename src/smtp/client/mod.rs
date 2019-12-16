//! SMTP client

mod codec;
mod inner;
pub mod mock;
pub mod net;

pub use self::codec::*;
pub use self::inner::*;
