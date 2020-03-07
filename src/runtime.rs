#[cfg(feature="runtime-async-std")]
pub use async_std::{
    future::{ timeout, TimeoutError },
    fs::File,
    io::BufRead,
    io::prelude::BufReadExt,
    io::BufReader,
    io::timeout as io_timeout,
    net::ToSocketAddrs,
    net::TcpStream
};
#[cfg(feature="runtime-tokio")]
pub use tokio::{
    fs::File,
    io::BufReader,
    io::AsyncBufRead as BufRead,
    io::AsyncBufReadExt as BufReadExt,
    net::ToSocketAddrs,
    net::TcpStream,
    time::{ timeout, Elapsed as TimeoutError }
};

pub use futures::io::{
    Cursor,
    AsyncRead as Read,
    AsyncWrite as Write,
    AsyncReadExt,
    AsyncWriteExt
};

#[cfg(feature="runtime-tokio")]
use std::{
    io::Result as IoResult,
    io::{ Error as IoError, ErrorKind },
    time::Duration,
    future::Future
};

/// A shim to match the signature of async-std's io_timeout
#[cfg(feature="runtime-tokio")]
pub async fn io_timeout<F,T>(dur: Duration, f: F) -> IoResult<T>
where F: Future<Output = IoResult<T>> {
    match timeout(dur, f).await {
        Ok(r) => r,
        Err(e) => Err(IoError::new(ErrorKind::TimedOut, e))
    }
}

#[cfg(feature="runtime-tokio")]
pub async fn spawn_blocking<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static {
    tokio::task::spawn_blocking(f).await.unwrap()
}

#[cfg(feature="runtime-async-std")]
pub async fn spawn_blocking<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static {
    async_std::task::spawn_blocking(f).await
}