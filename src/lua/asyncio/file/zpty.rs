use std::os::fd::{IntoRawFd, AsRawFd, RawFd};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{
    AsyncRead,
    AsyncWrite,
    ReadBuf,
};

// we use a tokio file so we can take advantage of the read/write impls
// but when we drop we leak the fd because zsh is responsible for closing it
pub struct ZptyFile(pub Option<tokio::fs::File>);

impl Drop for ZptyFile {
    fn drop(&mut self) {
        let _ = self.0.take().unwrap().try_into_std().unwrap().into_raw_fd();
    }
}

impl AsRawFd for ZptyFile {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_ref().unwrap().as_raw_fd()
    }
}

macro_rules! delegate_method {
    (fn $func:ident($($name:ident: $type:ty),*) -> $ret:ty {}) => (
        fn $func(mut self: Pin<&mut Self>, cx: &mut Context<'_>, $($name: $type),* ) -> $ret {
            std::pin::pin!(self.0.as_mut().unwrap()).$func(cx, $($name),*)
        }
    )
}

impl AsyncRead for ZptyFile {
    delegate_method!(fn poll_read(buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {});
}
impl AsyncWrite for ZptyFile {
    delegate_method!(fn poll_write(buf: &[u8]) -> Poll<std::io::Result<usize>> {});
    delegate_method!(fn poll_flush() -> Poll<std::io::Result<()>> {});
    delegate_method!(fn poll_shutdown() -> Poll<std::io::Result<()>> {});
}

