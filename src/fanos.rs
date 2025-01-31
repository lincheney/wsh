use std::io::{Cursor, Write, IoSlice};
use std::os::fd::{AsRawFd, RawFd, OwnedFd};
use futures::{select, future::FutureExt};
use async_std::io::{BufReadExt, ReadExt};
use nix::sys::{socket, signal};
use anyhow::Result;

const OSH: &str = "/home/qianli/Documents/oils-for-unix-0.26.0/_bin/cxx-opt-sh/osh";

pub struct FanosClient {
    child: std::process::Child,
    pub socket: FanosSocket,

    capture_write: OwnedFd,
    capture_read: async_std::fs::File,
}

pub struct FanosSocket {
    writer: RawFd,
    reader: async_std::io::BufReader<async_std::os::unix::net::UnixStream>,
    pub closed: bool,
}

impl FanosSocket {

    async fn send<'a>(&self, cmd: &[IoSlice<'a>], fds: Option<&[RawFd; 3]>) -> Result<()> {
        if self.closed {
            return Err(anyhow::anyhow!("socket is closed"));
        }

        let len: usize = cmd.iter().map(|c| c.len()).sum();

        let mut cursor = Cursor::new([0u8; 256]);
        write!(cursor, "{len}:")?;
        let buffer = &cursor.get_ref()[..cursor.position() as usize];

        let fds = fds.unwrap_or(&[0, 1, 2]);
        socket::send(self.writer, buffer, socket::MsgFlags::empty())?;
        socket::sendmsg::<()>(
            self.writer,
            cmd,
            &[socket::ControlMessage::ScmRights(fds)],
            socket::MsgFlags::empty(),
            None,
        )?;

        socket::send(self.writer, b",", socket::MsgFlags::empty())?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<bool> {
        let mut buf = vec![];
        self.reader.read_until(b':', &mut buf).await?;

        Ok(if buf.is_empty() {
            self.closed = true;
            false
        } else {
            let size = std::str::from_utf8(&buf[..buf.len()-1])?.parse::<usize>()? + 1;
            buf.resize(size, 0);
            self.reader.read_exact(&mut buf[..size]).await?;
            true
        })
    }
}

impl FanosClient {

    pub fn new() -> Result<Self> {
        let (client, server) = socket::socketpair(
            socket::AddressFamily::Unix,
            socket::SockType::Stream,
            None,
            // socket::SockFlag::SOCK_NONBLOCK,
            socket::SockFlag::empty(),
        )?;

        // async std spawn subprocess
        let child = std::process::Command::new(OSH)
            .arg("--headless")
            .stdin(server.try_clone()?)
            .stdout(server)
            .stderr(std::process::Stdio::null())
            .spawn()?
            ;

        let writer = client.as_raw_fd();
        let reader = std::os::unix::net::UnixStream::from(client);
        let reader = async_std::os::unix::net::UnixStream::from(reader);
        let reader = async_std::io::BufReader::new(reader);

        let (capture_read, capture_write) = Self::make_pipe()?;

        Ok(Self{
            child,
            socket: FanosSocket{ reader, writer, closed: false },
            capture_read,
            capture_write,
        })
    }

    fn make_pipe() -> Result<(async_std::fs::File, OwnedFd)> {
        let (read, write) = nix::unistd::pipe()?;
        let read = async_std::fs::File::from(std::fs::File::from(read));
        Ok((read, write))
    }

    pub async fn send<'a>(&self, cmd: &[IoSlice<'a>], fds: Option<&[RawFd; 3]>) -> Result<()> {
        self.socket.send(cmd, fds).await
    }

    pub async fn recv(&mut self) -> Result<bool> {
        self.socket.recv().await
    }

    pub async fn exec(&mut self, string: &str, fds: Option<&[RawFd; 3]>) -> Result<()> {
        self.send(&[IoSlice::new(b"EVAL "), IoSlice::new(string.as_bytes())], fds).await
    }

    pub async fn eval(&mut self, string: &str, capture_stderr: bool) -> Result<Vec<u8>> {
        let fds = if capture_stderr {
            [0, self.capture_write.as_raw_fd(), self.capture_write.as_raw_fd()]
        } else {
            [0, self.capture_write.as_raw_fd(), 2]
        };
        self.exec(string, Some(&fds)).await?;

        let mut buf = vec![0; 1024];
        let mut pos = 0;
        let mut recv = std::pin::pin!(self.socket.recv().fuse());
        let mut reader = self.capture_read.read(&mut buf[pos..]).fuse();

        let mut capture_closed = false;
        // read until the proc is finished
        loop {
            select! {
                recv = recv => {
                    recv?;
                    break;
                },
                num = reader => match num {
                    Ok(num) if num > 0 => {
                        pos += num;
                        if pos + 1024 > buf.len() {
                            buf.resize(1024, 0);
                        }
                        reader = self.capture_read.read(&mut buf[pos..]).fuse();
                    },
                    _ => capture_closed = true,
                },
            };
        }

        if ! capture_closed {
            // read the rest
            // one read should do it?
            select! {
                default => (),
                num = reader => match num {
                    Ok(num) => {
                        pos += num;
                        buf.drain(pos..);
                    },
                    _ => capture_closed = true,
                },
            }
        }

        if capture_closed {
            (self.capture_read, self.capture_write) = Self::make_pipe()?;
        }

        Ok(buf)
    }

    pub fn finish(&mut self) -> Result<std::process::ExitStatus> {
        if let Some(status) = self.child.try_wait()? {
            Ok(status)
        } else {
            self.terminate()?;
            Ok(self.child.wait()?)
        }
    }

    fn terminate(&mut self) -> Result<()> {
        signal::kill(nix::unistd::Pid::from_raw(self.child.id() as _), signal::Signal::SIGTERM)?;
        Ok(())
    }

}

impl Drop for FanosClient {
    fn drop(&mut self) {
        if let Err(err) = self.finish() {
            eprintln!("ERROR: {}", err);
        }
    }
}
