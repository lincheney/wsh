use std::os::fd::{AsRawFd, RawFd};
use async_std::io::{BufReadExt, ReadExt};
use nix::sys::socket;
use anyhow::Result;

const OSH: &str = "/home/qianli/Documents/oils-for-unix-0.26.0/_bin/cxx-opt-sh/osh";

pub struct FanosClient {
    child: std::process::Child,
    writer: RawFd,
    reader: async_std::io::BufReader<async_std::os::unix::net::UnixStream>,
}

impl FanosClient {

    pub async fn new() -> Result<Self> {
        let (client, server) = socket::socketpair(
            socket::AddressFamily::Unix,
            socket::SockType::Stream,
            None,
            socket::SockFlag::SOCK_NONBLOCK,
        )?;

        // async std spawn subprocess
        let child = std::process::Command::new(OSH)
            .arg("--headless")
            .stdin(server.try_clone()?)
            .stdout(server)
            .spawn()?
            ;

        let writer = client.as_raw_fd();
        let reader = std::os::unix::net::UnixStream::from(client);
        let reader = async_std::os::unix::net::UnixStream::from(reader);
        let reader = async_std::io::BufReader::new(reader);

        Ok(Self{
            child,
            reader,
            writer,
        })
    }

    pub async fn send(&self, cmd: &[u8], fds: Option<&[RawFd; 3]>) -> Result<()> {
        let fds = fds.unwrap_or(&[0, 1, 2]);

        let buf = format!("{}:", cmd.len());
        socket::send(self.writer, buf.as_bytes(), socket::MsgFlags::empty())?;
        socket::sendmsg::<()>(
            self.writer,
            &[std::io::IoSlice::new(cmd)],
            &[socket::ControlMessage::ScmRights(fds)],
            socket::MsgFlags::empty(),
            None,
        )?;

        socket::send(self.writer, b",", socket::MsgFlags::empty())?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<()> {
        let mut buf = vec![];
        self.reader.read_until(b':', &mut buf).await?;
        let size = std::str::from_utf8(&buf[..buf.len()-1])?.parse::<usize>()?;
        buf.resize(size, 0);
        self.reader.read_exact(&mut buf[..size]).await?;
        Ok(())
    }

}
