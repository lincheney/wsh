use std::io::Read;
use std::sync::{mpsc};
use std::os::fd::AsRawFd;
use bstr::{BString, ByteVec};
use anyhow::Result;
use tokio::io::unix::AsyncFd;
use crate::unsafe_send::UnsafeSend;

unsafe extern "C" {
    static mut stdout: *mut nix::libc::FILE;
    static mut stderr: *mut nix::libc::FILE;
}

const BUF_SIZE: usize = 1024;
type BufResult = std::io::Result<(usize, [u8; BUF_SIZE])>;

pub struct Sink {
    output: mpsc::Receiver<BufResult>,
    flusher: tokio::sync::mpsc::Sender<mpsc::Sender<()>>,
    #[allow(dead_code)]
    writer: std::io::PipeWriter,
    writer_ptr: UnsafeSend<*mut nix::libc::FILE>,
}

impl Sink {

    pub fn new() -> Result<Self> {
        let (reader, writer) = std::io::pipe()?;
        let writer_ptr = unsafe{ nix::libc::fdopen(writer.as_raw_fd(), c"w".as_ptr()) };
        if writer_ptr.is_null() {
            return Err(std::io::Error::last_os_error())?;
        }
        crate::utils::set_nonblocking_fd(&reader)?;
        let writer_ptr = unsafe{ UnsafeSend::new(writer_ptr) };

        let (flusher, flushable) = tokio::sync::mpsc::channel(1);
        let (sender, receiver) = mpsc::channel();
        // spawn a thread to read from the sink
        tokio::task::spawn(Sink::read_loop(reader, writer_ptr, sender, flushable));

        Ok(Self {
            output: receiver,
            flusher,
            writer,
            writer_ptr,
        })
    }

    pub fn clear(&mut self) -> Result<(), std::io::Error> {
        self.output.try_iter()
            .find_map(|x| x.err())
            .map(Err)
            .unwrap_or(Ok(()))
    }

    pub fn read(&mut self) -> Result<BString, std::io::Error> {
        let (sender, receiver) = mpsc::channel();
        // ask it to finish
        if self.flusher.blocking_send(sender).is_ok() {
            let _ = receiver.recv();
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

    async fn read_loop(
        mut reader: std::io::PipeReader,
        writer_ptr: UnsafeSend<*mut nix::libc::FILE>,
        queue: mpsc::Sender<BufResult>,
        mut flushable: tokio::sync::mpsc::Receiver<mpsc::Sender<()>>,
    ) {

        let mut allow_flush = true;
        let mut flush_notifier: Option<mpsc::Sender<()>> = None;
        let fd = AsyncFd::new(reader.as_raw_fd()).unwrap();

        loop {
            flush_notifier.take();

            // Wait for the pipe to be readable
            tokio::select!(
                result = fd.readable() => {
                    match result {
                        Ok(mut guard) => {
                            guard.clear_ready();
                        },
                        Err(err) => {
                            let _ = queue.send(Err(err));
                            return
                        },
                    }
                },
                sender = flushable.recv(), if allow_flush => {
                    flush_notifier = sender;
                    if flush_notifier.is_some() {
                        allow_flush = true;
                        // flush as requested
                        unsafe {
                            nix::libc::fflush(writer_ptr.into_inner()); // ignore errors?
                        }
                    }
                },
            );

            loop {
                let mut buf = [0; BUF_SIZE];
                // Try to read data, this may still fail with `WouldBlock`
                // if the readiness event is a false positive.
                let x = reader.read(&mut buf);
                match x {
                    Ok(n) => if queue.send(Ok((n, buf))).is_err() {
                        return
                    } else if n < BUF_SIZE {
                        break
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        let _ = queue.send(Err(e));
                        return
                    },
                }
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
