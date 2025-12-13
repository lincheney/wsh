use std::os::fd::AsRawFd;
use bstr::BString;
use anyhow::Result;
use tokio::net::unix::pipe;
use crate::unsafe_send::UnsafeSend;

unsafe extern "C" {
    static mut stdout: *mut nix::libc::FILE;
    static mut stderr: *mut nix::libc::FILE;
}

struct Sink {
    reader: Option<pipe::Receiver>,
    #[allow(dead_code)]
    writer: pipe::Sender,
    writer_ptr: UnsafeSend<*mut nix::libc::FILE>,
}

impl Sink {

    fn new() -> Result<Self> {
        let (writer, reader) = pipe::pipe()?;
        let writer_ptr = unsafe{ nix::libc::fdopen(reader.as_raw_fd(), c"w".as_ptr()) };
        if writer_ptr.is_null() {
            return Err(std::io::Error::last_os_error())?;
        }

        Ok(Self {
            reader: Some(reader),
            writer,
            writer_ptr: unsafe{ UnsafeSend::new(writer_ptr) },
        })
    }

    async fn read(reader: &mut pipe::Receiver) -> std::io::Result<BString> {
        const BUF_SIZE: usize = 1024;

        let mut buf = BString::new(vec![]);
        loop {
            // Wait for the pipe to be readable
            reader.readable().await?;

            let old_len = buf.len();
            buf.resize(old_len + BUF_SIZE, 0);

            // Try to read data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.
            match reader.try_read(&mut buf[old_len .. ]) {
                Ok(BUF_SIZE) => {
                    continue
                },
                Ok(n) => {
                    buf.truncate(old_len + n);
                    break
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    continue
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok(buf)
    }

    pub async fn override_file(&mut self, file: UnsafeSend<*mut *mut nix::libc::FILE>) -> ShoutGuard<'_> {
        ShoutGuard::new(self, file)
    }

    pub async fn override_stdout(&mut self) -> ShoutGuard<'_> {
        self.override_file(unsafe{ UnsafeSend::new(&raw mut stdout) }).await
    }

    pub async fn override_stderr(&mut self) -> ShoutGuard<'_> {
        self.override_file(unsafe{ UnsafeSend::new(&raw mut stderr) }).await
    }

    pub async fn override_shout(&mut self) -> ShoutGuard<'_> {
        self.override_file(unsafe{ UnsafeSend::new(&raw mut zsh_sys::shout as _) }).await
    }

}

struct ShoutGuard<'a> {
    inner: &'a mut Sink,
    dest: UnsafeSend<*mut *mut nix::libc::FILE>,
    old_file: UnsafeSend<*mut nix::libc::FILE>,
    result: Option<tokio::task::JoinHandle<(pipe::Receiver, std::io::Result<BString>)>>,
}

impl ShoutGuard<'_> {

    pub fn new(parent: &mut Sink, file: UnsafeSend<*mut *mut nix::libc::FILE>) -> ShoutGuard<'_> {
        let old_file = unsafe{ UnsafeSend::new(*file.into_inner()) };
        unsafe{ *file.into_inner() = parent.writer_ptr.into_inner(); }
        let mut reader = parent.reader.take().unwrap();
        let handle = tokio::task::spawn(async move {
            let result = Sink::read(&mut reader).await;
            (reader, result)
        });
        ShoutGuard{
            inner: parent,
            dest: file,
            old_file,
            result: Some(handle),
        }
    }

    async fn _read(&mut self) -> Result<BString> {
        unsafe{
            nix::libc::fflush(self.inner.writer_ptr.into_inner()); // ignore errors?
            *self.dest.into_inner() = self.old_file.into_inner();
        }
        let (reader, result) = self.result.take().unwrap().await?;
        self.inner.reader = Some(reader);
        Ok(result?)

    }

    pub async fn read(mut self) -> Result<BString> {
        self._read().await
    }
}

impl Drop for ShoutGuard<'_> {
    fn drop(&mut self) {
        tokio::task::block_in_place(|| {
            let _ = tokio::runtime::Handle::current().block_on(self._read());
        });
    }
}
