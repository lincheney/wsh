use std::os::fd::{AsRawFd, FromRawFd};
use async_std::io::BufReadExt;
use nix::sys::socket;
use crossterm::event::{self, Event};
use anyhow::Result;

#[async_std::main]
async fn main() -> Result<()> {

    let (client, server) = socket::socketpair(
        socket::AddressFamily::Unix,
        socket::SockType::Stream,
        None,
        socket::SockFlag::SOCK_NONBLOCK,
    )?;

    // async std spawn subprocess
    let child = std::process::Command::new("/home/qianli/Documents/oils-for-unix-0.26.0/_bin/cxx-opt-sh/osh")
        .arg("--headless")
        .stdin(server.try_clone()?)
        .stdout(server)
        .spawn()?
        ;

    let cmd = b"EVAL echo $PWD";
    let x = socket::send(client.as_raw_fd(), format!("{}:", cmd.len()).as_bytes(), socket::MsgFlags::empty())?;
    eprintln!("DEBUG(eczema)\t{}\t= {:?}", stringify!(x), x);
    let x = socket::sendmsg::<()>(
        client.as_raw_fd(),
        &[std::io::IoSlice::new(cmd)],
        &[socket::ControlMessage::ScmRights(&[0, 1, 2])],
        socket::MsgFlags::empty(),
        None,
    )?;
    eprintln!("DEBUG(amuse) \t{}\t= {:?}", stringify!(x), x);
    let x = socket::send(client.as_raw_fd(), b",", socket::MsgFlags::empty())?;
    eprintln!("DEBUG(define)\t{}\t= {:?}", stringify!(x), x);

    let stream = std::os::unix::net::UnixStream::from(client);
    let stream = async_std::os::unix::net::UnixStream::from(stream);
    let mut stream = async_std::io::BufReader::new(stream);
    let mut buf = vec![];
    let x = stream.read_until(b':', &mut buf).await?;

    eprintln!("DEBUG(dhoti) \t{}\t= {:?}", stringify!(x), x);
    eprintln!("DEBUG(dabble)\t{}\t= {:?}", stringify!(std::str::from_utf8(&buf)), std::str::from_utf8(&buf));

    eprintln!("DEBUG(long)  \t{}\t= {:?}", stringify!(123), 123);

    // cmd.arg("-l");
    // cmd.stdout(Stdio::piped());
    // let mut child = cmd.spawn().unwrap();
    // let output = child.wait_with_output().unwrap();
    // println!("status: {}", output.status);
    // println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    // println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
//
    // // async std spawn subprocess with pipe and read
    // let mut cmd = Command::new("ls");
    // cmd.arg("-l");
    // cmd.stdout

    Ok(())
}
