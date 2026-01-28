use bstr::BString;
use std::time::SystemTime;
use std::default::Default;
use anyhow::{Result};
use mlua::{prelude::*};
use std::io::{Read, Write};
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use tokio::io::{BufStream, unix::AsyncFd, ReadBuf, AsyncRead, AsyncWrite};
use tokio::sync::{watch};
use serde::{Deserialize};
use crate::ui::{Ui};
use crate::lua::asyncio::{ReadWriteFile};

#[derive(Default, Debug, Deserialize)]
#[serde(default)]
struct FullZptyArgs {
    args: BString,
    height: Option<usize>,
    width: Option<usize>,
    echo_input: bool,
}


#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ZptyArgs {
    Simple(BString),
    Full(FullZptyArgs),
}

struct AsyncZpty {
    inner: AsyncFd<std::fs::File>,
}

impl std::os::fd::AsRawFd for AsyncZpty {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.inner.as_raw_fd()
    }
}

impl AsyncRead for AsyncZpty {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        loop {
            let mut guard = ready!(self.inner.poll_read_ready(cx))?;
            let unfilled = buf.initialize_unfilled();
            match guard.try_io(|inner| inner.get_ref().read(unfilled)) {
                Ok(Ok(len)) => {
                    buf.advance(len);
                    return Poll::Ready(Ok(()));
                },
                Ok(Err(err)) => return Poll::Ready(Err(err)),
                Err(_would_block) => continue,
            }
        }
    }
}

impl AsyncWrite for AsyncZpty {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        loop {
            let mut guard = ready!(self.inner.poll_write_ready(cx))?;
            match guard.try_io(|inner| inner.get_ref().write(buf)) {
                Ok(result) => return Poll::Ready(result),
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

pub async fn zpty(ui: Ui, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
    let args = match lua.from_value(val)? {
        ZptyArgs::Full(args) => args,
        ZptyArgs::Simple(args) => FullZptyArgs{args, ..Default::default()},
    };

    let (sender, receiver) = watch::channel(None);

    let cmd = args.args;
    let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs_f64();
    let name = format!("zpty-{time}");
    let opts = crate::shell::ZptyOpts{
        echo_input: args.echo_input,
        non_blocking: true,
    };
    let zpty = ui.shell.zpty(name.into(), cmd.into(), opts).await?;

    // do not drop the pty fd as zsh will do it for us
    // so we dup the fd to one we own instead
    let pty = crate::utils::dup_fd(unsafe{ std::os::fd::BorrowedFd::borrow_raw(zpty.fd) })?;
    // crate::utils::set_nonblocking_fd(&pty)?;
    let pty = AsyncZpty{ inner: AsyncFd::new(pty.into())? };
    let pty = ReadWriteFile{
        inner: Some(BufStream::new(pty)),
        is_tty_master: true,
    };

    let pid = zpty.pid;
    tokio::task::spawn(async move {
        // get the status
        let pid_waiter = crate::shell::process::register_pid(pid as _, false);
        let code = match ui.shell.check_pid_status(pid as _).await {
            None | Some(-1) => pid_waiter.await.unwrap_or(-1),
            Some(code) => code,
        };
        // send the code out
        let _ = sender.send(Some(Ok(code as _)));

        // delete the zpty once it has finished
        // this will close the original zpty fds
        // which is ok for us since we have dup-ed them
        ui.shell.zpty_delete(zpty.name).await
    });

    Ok(lua.pack_multi((
        super::Process{
            pid,
            result: super::CommandResult{ inner: receiver },
        },
        pty,
    ))?)
}
