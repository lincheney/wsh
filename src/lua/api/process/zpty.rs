use std::os::fd::AsRawFd;
use crate::lua::LuaWrapper;
use crate::shell::{MetaString};
use std::rc::Rc;
use crate::meta_str;
use std::num::NonZeroU16;
use bstr::BString;
use std::time::SystemTime;
use std::default::Default;
use anyhow::{Result};
use mlua::{prelude::*};
use std::io::{Read, Write};
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use tokio::io::{unix::AsyncFd, ReadBuf, AsyncRead, AsyncWrite, BufWriter, BufReader};
use tokio::sync::{watch, RwLock};
use serde::{Deserialize};
use crate::ui::{Ui};
use crate::lua::api::asyncio::{ReadableFile, WriteableFile};

#[derive(Default, Debug, Deserialize)]
#[serde(default)]
struct FullZptyArgs {
    args: BString,
    height: Option<NonZeroU16>,
    width: Option<NonZeroU16>,
    no_echo_input: bool,
}


#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ZptyArgs {
    Simple(BString),
    Full(FullZptyArgs),
}

struct AsyncZpty {
    inner: Rc<AsyncFd<std::fs::File>>,
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
            let result = guard.try_io(|inner| inner.get_ref().read(unfilled));
            match result {
                Ok(Ok(len)) => {
                    buf.advance(len);
                    return Poll::Ready(Ok(()));
                },
                Ok(Err(err)) => return Poll::Ready(Err(err)),
                Err(_would_block) => (),
            }
        }
    }
}

impl AsyncWrite for AsyncZpty {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        loop {
            let mut guard = ready!(self.inner.poll_write_ready(cx))?;
            let result = guard.try_io(|inner| inner.get_ref().write(buf));
            match result {
                Ok(result) => return Poll::Ready(result),
                Err(_would_block) => (),
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

    // wrap in an eval so that zpty doesn't immediately fail
    let mut cmd = crate::shell::shell_quote(args.args.into());
    cmd.insert_str(0, meta_str!(c"eval "));

    let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs_f64();
    let name: MetaString = format!("zpty-{time}").into();
    let opts = crate::shell::ZptyOpts{
        echo_input: !args.no_echo_input,
        non_blocking: true,
        height: args.height,
        width: args.width,
    };
    // TODO capture shout

    // fork it now to get the pid
    let (result, queue_result) = ui.shell.with_queued_signals::<Result<_>, _>(|| {
        let zpty = ui.shell.zpty(name, cmd.as_ref(), opts)?;
        let pid_waiter = crate::shell::signals::sigchld::register_pid(&ui, zpty.pid as _, true);
        Ok((zpty, pid_waiter))
    });
    crate::log_if_err(queue_result);
    let (zpty, pid_waiter) = result?;
    let pid_waiter = pid_waiter?;

    // do not drop the pty fd as zsh will do it for us
    // so we dup the fd to one we own instead
    let pty = crate::utils::dup_fd(unsafe{ std::os::fd::BorrowedFd::borrow_raw(zpty.fd) })?;
    // crate::utils::set_nonblocking_fd(&pty)?;
    let pty = Rc::new(AsyncFd::new(pty.into())?);
    let stdin = WriteableFile{
        fd: pty.as_raw_fd(),
        inner: RwLock::new(Some(BufWriter::new(AsyncZpty{ inner: pty.clone() }))),
    };
    let stdout = ReadableFile{
        fd: pty.as_raw_fd(),
        inner: RwLock::new(Some(BufReader::new(AsyncZpty{ inner: pty }))),
        is_tty_master: true,
    };

    let pid = zpty.pid;
    ui.clone().runtime.spawn_local(async move {
        let code = match ui.shell.check_pid_status(pid as _) {
            None | Some(-1) => pid_waiter.await.unwrap_or(-1),
            Some(code) => code,
        };
        // send the code out
        let _ = sender.send(Some(Ok(code as _)));

        // delete the zpty once it has finished
        // this will close the original zpty fds
        // which is ok for us since we have dup-ed them
        ui.shell.zpty_delete(zpty.name)
    })?;

    Ok(lua.pack_multi((
        super::spawn::Process{
            pid,
            result: super::spawn::CommandResult{ inner: receiver },
        },
        stdin,
        stdout,
    ))?)
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_async_fn("__zpty", zpty)?;

    Ok(())
}
