use crate::lua::LuaWrapper;
use bstr::BString;
use std::default::Default;
use std::os::fd::{RawFd, AsRawFd, IntoRawFd};
use std::fs::File;
use anyhow::{Result};
use mlua::{prelude::*};
use tokio::io::{
    BufReader,
    BufWriter,
};
use tokio::sync::{oneshot, watch};
use serde::{Deserialize};
use crate::ui::{Ui};
use crate::lua::api::asyncio::{ReadableFile, WriteableFile};
use super::spawn::{Stdio, Process, CommandResult};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct FullShellRunArgs {
    pub command: BString,
    pub stdin: Stdio,
    pub stdout: Stdio,
    pub stderr: Stdio,
    pub foreground: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SubshellRunArgs {
    Simple(BString),
    Full(FullShellRunArgs),
}

#[derive(Debug)]
struct OverriddenStream {
    fd: RawFd,
    replacement: RawFd,
    closed: bool,
}

impl OverriddenStream {
    fn new<A: AsRawFd, B: IntoRawFd>(fd: &A, replacement: B) -> Self {
        Self {
            fd: fd.as_raw_fd(),
            replacement: replacement.into_raw_fd(),
            closed: false,
        }
    }

    fn close(&mut self) -> Result<()> {
        nix::unistd::close(self.replacement)?;
        self.closed = true;
        Ok(())
    }
}

macro_rules! stdio_pipe {
    ($args:expr, $name:ident, true) => (
        stdio_pipe!($args, $name, File::create("/dev/null"), {
            let (send, recv) = tokio::net::unix::pipe::pipe()?;
            let fd = send.as_raw_fd();
            let send = WriteableFile(Some(BufWriter::new(send)));
            (Some(send), Some(OverriddenStream::new(&std::io::$name(), recv.into_nonblocking_fd()?)), Some(fd))
        })
    );
    ($args:expr, $name:ident, false) => (
        stdio_pipe!($args, $name, File::open("/dev/null"), {
            let (send, recv) = tokio::net::unix::pipe::pipe()?;
            let fd = recv.as_raw_fd();
            let recv = ReadableFile(Some(BufReader::new(recv)), false);
            (Some(recv), Some(OverriddenStream::new(&std::io::$name(), send.into_nonblocking_fd()?)), Some(fd))
        })
    );
    ($args:expr, $name:ident, $null:expr, $piped:expr) => (
        match $args.$name {
            Stdio::inherit => (None, None, None),
            Stdio::null => (None, Some(OverriddenStream::new(&std::io::$name(), $null?)), None),
            Stdio::piped => $piped,
        }
    );
}

async fn subshell_run(ui: Ui, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
    let args = match lua.from_value(val)? {
        SubshellRunArgs::Simple(command) => FullShellRunArgs{command, ..Default::default()},
        SubshellRunArgs::Full(args) => args
    };
    subshell_run_with_args(ui, lua, args).await
}

pub async fn subshell_run_with_args(ui: Ui, lua: Lua, args: FullShellRunArgs) -> Result<LuaMultiValue> {

    let (result_sender, result_receiver) = oneshot::channel();
    let (sender, receiver) = watch::channel(None);

    let foreground = args.foreground.unwrap_or(
        matches!(args.stdin, Stdio::inherit)
        || matches!(args.stdout, Stdio::inherit)
        || matches!(args.stderr, Stdio::inherit)
    );

    let stdin = stdio_pipe!(args, stdin, true);
    let stdout = stdio_pipe!(args, stdout, false);
    let stderr = stdio_pipe!(args, stderr, false);
    let mut streams = [stdin.1, stdout.1, stderr.1];
    let fds = [stdin.2, stdout.2, stderr.2];

    ui.clone().runtime.spawn_local(async move {
        let result = ui.clone().shell.trampoline_out_callback(move |mut ui, token| {

            ui.clone().shell_loop(false, async move {
                let mut result_sender = Some(result_sender);

                let result = ui.freeze_if(foreground, true, async {
                    // fork it now to get the pid
                    let redirections = streams.each_ref().map(|s| s.as_ref().map(|s| (s.fd, s.replacement, s.fd != 0)));
                    let pid = ui.shell.exec_subshell(token, args.command.as_ref(), false, &redirections, &fds)? as _;
                    // close them or we will block
                    for file in streams.iter_mut().flatten() {
                        crate::log_if_err(file.close());
                    }
                    // send streams back to caller
                    let _ = result_sender.take().unwrap().send(Ok((pid, stdin.0, stdout.0, stderr.0)));
                    // get the status
                    let pid_waiter = crate::shell::process::register_pid(&ui, pid as _, true);
                    let code = match ui.shell.check_pid_status(pid as _) {
                        None | Some(-1) => pid_waiter.await.unwrap_or(-1) as _,
                        Some(code) => code as _,
                    };

                    let _ = sender.send(Some(Ok(code)));
                    Ok::<_, anyhow::Error>(())
                }).await;

                match result {
                    Err(err) | Ok(Err(err)) => {
                        if let Some(result_sender) = result_sender {
                            let _ = result_sender.send(Err(err));
                        } else {
                            let err: Result<()> = Err(err);
                            ui.report_error(err);
                        }
                    },
                    _ => (),
                }

            })

        }).await;
        crate::log_if_err(result);

    });

    let (pid, stdin, stdout, stderr) = result_receiver.await.unwrap()?;

    Ok(lua.pack_multi((
        Process{
            pid,
            result: CommandResult{ inner: receiver },
        },
        stdin,
        stdout,
        stderr,
    ))?)
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_async_fn("__subshell_run", subshell_run)?;

    Ok(())
}

