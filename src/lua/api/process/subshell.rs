use crate::lua::LuaWrapper;
use bstr::BString;
use std::default::Default;
use std::os::fd::{AsRawFd, OwnedFd};
use anyhow::{Result};
use mlua::{prelude::*};
use tokio::io::{
    BufReader,
    BufWriter,
};
use tokio::net::unix::pipe::{Sender, Receiver};
use tokio::sync::{oneshot, watch, RwLock};
use serde::{Deserialize};
use crate::ui::{Ui};
use crate::lua::api::asyncio::{ReadableFile, WriteableFile};
use super::spawn::{Stdio, Process, CommandResult};
use super::shell::Stream;
use crate::shell::FdAction;

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

struct FdOverride {
    stream: Stream<Stdio>,
    fd: Option<OwnedFd>,
}

impl FdOverride {

    fn new_basic(stream: Stream<Stdio>) -> Result<Option<Option<(Sender, Receiver)>>> {
        let (Stream::Stdin(stdio) | Stream::Stdout(stdio) | Stream::Stderr(stdio)) = stream;
        match stdio {
            Stdio::inherit => Ok(None),
            Stdio::null  => Ok(Some(None)),
            Stdio::piped => Ok(Some(Some(tokio::net::unix::pipe::pipe()?))),
        }
    }

    fn new_stdin(stdio: Stdio) -> Result<(Option<Self>, Option<Sender>)> {
        let stream = Stream::Stdin(stdio);
        Ok(match Self::new_basic(stream)? {
            Some(Some((send, recv))) => {
                (Some(Self{ stream, fd: Some(recv.into_nonblocking_fd()?) }), Some(send))
            },
            Some(None) => (Some(Self{ stream, fd: None }), None),
            None => (None, None),
        })
    }

    fn new_out(stream: Stream<Stdio>) -> Result<(Option<Self>, Option<Receiver>)> {
        Ok(match Self::new_basic(stream)? {
            Some(Some((send, recv))) => {
                (Some(Self{ stream, fd: Some(send.into_nonblocking_fd()?) }), Some(recv))
            },
            Some(None) => (Some(Self{ stream, fd: None }), None),
            None => (None, None),
        })
    }

    fn fd_action(&self) -> FdAction {
        let fd = self.stream.as_raw_fd();
        let other = self.fd.as_ref().map(|fd| fd.as_raw_fd());
        match self.stream {
            Stream::Stdin(_) => FdAction::RedirectFrom(fd, other),
            _ => FdAction::RedirectTo(fd, other),
        }
    }
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

    ui.clone().runtime.spawn_local(async move {
        let result = ui.clone().shell.trampoline_out_callback(move |mut ui, token| {

            ui.clone().shell_loop(false, async move {
                let mut result_sender = Some(result_sender);

                let result = ui.freeze_if(foreground, true, async {

                    let (stdin, stdin_pipe)   = FdOverride::new_stdin(args.stdin)?;
                    let (stdout, stdout_pipe) = FdOverride::new_out(Stream::Stdout(args.stdout))?;
                    let (stderr, stderr_pipe) = FdOverride::new_out(Stream::Stderr(args.stderr))?;
                    let streams = [stdin, stdout, stderr];

                    let redirections = streams.iter().flatten().map(|s| s.fd_action());
                    let closes = [
                        stdin_pipe.as_ref().map(|x| x.as_raw_fd()),
                        stdout_pipe.as_ref().map(|x| x.as_raw_fd()),
                        stderr_pipe.as_ref().map(|x| x.as_raw_fd()),
                    ].into_iter().flatten().map(FdAction::Close);
                    let fds = redirections.chain(closes);

                    // fork it now to get the pid
                    let pid = ui.shell.exec_subshell(token, args.command.as_ref(), false, fds)? as _;
                    let pid_waiter = crate::shell::process::register_pid(&ui, pid as _, true);
                    // close streams or we will block
                    drop(streams);
                    // send streams back to caller
                    let _ = result_sender.take().unwrap().send(Ok((
                        pid,
                        stdin_pipe,
                        stdout_pipe,
                        stderr_pipe,
                    )));
                    // get the status
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
        stdin.map(|stdin| WriteableFile{
            fd: stdin.as_raw_fd(),
            inner: RwLock::new(Some(BufWriter::new(stdin))),
        } ),
        stdout.map(|stdout| ReadableFile{
            fd: stdout.as_raw_fd(),
            inner: RwLock::new(Some(BufReader::new(stdout))),
            is_tty_master: false,
        }),
        stderr.map(|stderr| ReadableFile{
            fd: stderr.as_raw_fd(),
            inner: RwLock::new(Some(BufReader::new(stderr))),
            is_tty_master: false,
        }),
    ))?)
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_async_fn("__subshell_run", subshell_run)?;

    Ok(())
}

