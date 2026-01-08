use std::sync::mpsc;
use std::os::fd::AsRawFd;
use bstr::{BString, ByteVec};
use anyhow::Result;
use tokio::net::unix::pipe;
use crate::unsafe_send::UnsafeSend;

unsafe extern "C" {
    static mut stdout: *mut nix::libc::FILE;
    static mut stderr: *mut nix::libc::FILE;
}

const BUF_SIZE: usize = 1024;
type BufResult = std::io::Result<(usize, [u8; BUF_SIZE])>;

pub struct Sink {
    output: mpsc::Receiver<BufResult>,
    #[allow(dead_code)]
    writer: pipe::Sender,
    writer_ptr: UnsafeSend<*mut nix::libc::FILE>,
}

impl Sink {

    pub fn new() -> Result<Self> {
        let (writer, reader) = pipe::pipe()?;
        let writer_ptr = unsafe{ nix::libc::fdopen(writer.as_raw_fd(), c"w".as_ptr()) };
        if writer_ptr.is_null() {
            return Err(std::io::Error::last_os_error())?;
        }

        let (sender, receiver) = mpsc::channel();
        // spawn a thread to read from the sink
        tokio::task::spawn(Sink::read_loop(reader, sender));

        Ok(Self {
            output: receiver,
            writer,
            writer_ptr: unsafe{ UnsafeSend::new(writer_ptr) },
        })
    }

    pub fn clear(&mut self) -> Result<(), std::io::Error> {
        self.output.try_iter()
            .find_map(|x| x.err())
            .map(Err)
            .unwrap_or(Ok(()))
    }

    pub fn read(&mut self) -> Result<BString, std::io::Error> {
        unsafe {
            nix::libc::fflush(self.writer_ptr.into_inner()); // ignore errors?
        }

        let mut result = BString::new(vec![]);
        for value in self.output.try_iter() {
            match value {
                Ok((size, buf)) => {
                    result.push_str(&buf[..size]);
                },
                Err(err) => return Err(err),
            }
        }
        Ok(result)
    }

    async fn read_loop(reader: pipe::Receiver, queue: mpsc::Sender<BufResult>) -> Result<(), mpsc::SendError<BufResult>> {
        loop {
            // Wait for the pipe to be readable
            if let Err(err) = reader.readable().await {
                return queue.send(Err(err))
            }

            let mut buf = [0; BUF_SIZE];
            // Try to read data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.
            match reader.try_read(&mut buf) {
                Ok(n) => queue.send(Ok((n, buf)))?,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => return queue.send(Err(e)),
            }
        }
    }

    pub fn override_file(&self, file: UnsafeSend<*mut *mut nix::libc::FILE>) -> FileGuard<'_> {
        FileGuard::new(self, file)
    }

    pub fn override_stdout(&self) -> FileGuard<'_> {
        self.override_file(unsafe{ UnsafeSend::new(&raw mut stdout) })
    }

    pub fn override_stderr(&self) -> FileGuard<'_> {
        self.override_file(unsafe{ UnsafeSend::new(&raw mut stderr) })
    }

    pub fn override_shout(&self) -> FileGuard<'_> {
        self.override_file(unsafe{ UnsafeSend::new((&raw mut zsh_sys::shout).cast()) })
    }

}

pub struct FileGuard<'a> {
    #[allow(dead_code)]
    pub inner: &'a Sink,
    dest: UnsafeSend<*mut *mut nix::libc::FILE>,
    old_file: UnsafeSend<*mut nix::libc::FILE>,
}

impl FileGuard<'_> {
    pub fn new(parent: &Sink, file: UnsafeSend<*mut *mut nix::libc::FILE>) -> FileGuard<'_> {
        let old_file = unsafe{ UnsafeSend::new(*file.into_inner()) };
        unsafe{ *file.into_inner() = parent.writer_ptr.into_inner(); }
        FileGuard{
            inner: parent,
            dest: file,
            old_file,
        }
    }
}

impl Drop for FileGuard<'_> {
    fn drop(&mut self) {
        unsafe{
            *self.dest.into_inner() = self.old_file.into_inner();
        }
    }
}
