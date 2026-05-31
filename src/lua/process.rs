use crate::shell::ShellLoop;
use bstr::BString;
use std::sync::Arc;
use std::str::FromStr;
use std::collections::HashMap;
use std::default::Default;
use std::os::fd::{RawFd, AsRawFd, IntoRawFd};
use std::fs::File;
use anyhow::{Result, Context};
use mlua::{prelude::*, UserData, UserDataMethods, UserDataFields};
use tokio::io::{
    BufReader,
    BufWriter,
};
use tokio::process::Command;
use tokio::sync::{oneshot, watch};
use serde::{Deserialize, Deserializer, de};
use crate::ui::{Ui};
use super::asyncio::{ReadableFile, WriteableFile};
mod zpty;

#[derive(Debug, Copy, Clone)]
struct Signal(nix::sys::signal::Signal);
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum RawSignal {
    Number(i32),
    String(String),
}

impl<'de> Deserialize<'de> for Signal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let signal = match RawSignal::deserialize(deserializer)? {
            RawSignal::Number(x) => nix::sys::signal::Signal::try_from(x).map_err(de::Error::custom)?,
            RawSignal::String(x) => nix::sys::signal::Signal::from_str(x.as_ref()).map_err(de::Error::custom)?,
        };
        Ok(Signal(signal))
    }
}

#[derive(Clone)]
struct CommandResult {
    inner: watch::Receiver<Option<std::io::Result<i32>>>,
}

impl CommandResult {
    async fn wait(&mut self) -> LuaResult<i32> {
        match self.inner.wait_for(|x| x.is_some()).await {
            Ok(x) => match x.as_ref().unwrap() {
                Ok(x) => Ok(*x),
                Err(e) => Err(LuaError::RuntimeError(e.to_string())),
            },
            Err(_) => Ok(i32::MAX),
        }
    }
}

impl UserData for CommandResult {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("wait", |_lua, proc, ()| async move {
            proc.clone().wait().await
        });
    }
}

struct Process {
    pid: u32,
    result: CommandResult,
}

impl UserData for Process {

    // this is silly but otherwise wait() will lock Process and then we can't kill() it at the same time
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("wait", |lua, proc| {
            let cmd = proc.result.clone();
            lua.create_async_function(move |_lua, ()| {
                let mut cmd = cmd.clone();
                async move {
                    Ok(cmd.wait().await)
                }
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("pid", |_lua, proc, ()| {
            Ok(proc.pid)
        });

        methods.add_method("is_finished", |_lua, proc, ()| {
            Ok(proc.result.inner.borrow().is_some())
        });

        methods.add_method("kill", |lua, proc, signal: LuaValue| {
            if proc.result.inner.borrow().is_none() {
                let signal: Signal = lua.from_value(signal)?;
                let pid = nix::unistd::Pid::from_raw(proc.pid as _);
                nix::sys::signal::kill(pid, signal.0).map_err(|e| LuaError::RuntimeError(e.to_string()))
            } else {
                Ok(())
            }
        });

    }
}

#[derive(Debug, Default, Deserialize, Copy, Clone)]
#[allow(non_camel_case_types)]
enum Stdio {
    #[default]
    inherit,
    piped,
    null,
}

impl From<Stdio> for std::process::Stdio {
    fn from(val: Stdio) -> Self {
        match val {
            Stdio::inherit => Self::inherit(),
            Stdio::piped => Self::piped(),
            Stdio::null => Self::null(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct FullShellRunOpts {
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
    foreground: Option<bool>,
}

pub enum ShellRunCmd {
    Simple(BString),
    Function{
        func: Arc<crate::shell::Function>,
        args: Vec<BString>,
        arg0: Option<BString>,
    },
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FullShellRunArgs {
    args: BString,
    #[serde(flatten)]
    opts: FullShellRunOpts,
    subshell: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FullSpawnArgs {
    args: Vec<String>,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
    env: Option<HashMap<String, String>>,
    clear_env: bool,
    cwd: Option<String>,
    foreground: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SpawnArgs {
    Shell(BString),
    Simple(Vec<String>),
    Full(FullSpawnArgs),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ShellRunArgs {
    Simple(BString),
    Full(FullShellRunArgs),
}

async fn spawn(mut ui: Ui, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
    let args = match lua.from_value(val)? {
        SpawnArgs::Shell(args) => return subshell_run(ui, lua, args, Default::default()).await,
        SpawnArgs::Full(args) => args,
        SpawnArgs::Simple(args) => FullSpawnArgs{args, ..Default::default()},
    };
    let first_arg = args.args.first().ok_or_else(|| LuaError::RuntimeError("no args given".to_owned()))?;

    let mut command = Command::new(first_arg);
    if args.args.len() > 1 {
        command.args(&args.args[1..]);
    }
    if args.clear_env {
        command.env_clear();
    }
    if let Some(env) = args.env {
        for (k, v) in &env {
            command.env(k,v);
        }
    }
    if let Some(cwd) = args.cwd {
        command.current_dir(cwd);
    }
    command.stdin(args.stdin);
    command.stdout(args.stdout);
    command.stderr(args.stderr);
    let foreground = args.foreground.unwrap_or(
        matches!(args.stdin, Stdio::inherit)
        || matches!(args.stdout, Stdio::inherit)
        || matches!(args.stderr, Stdio::inherit)
    );

    let (result_sender, result_receiver) = oneshot::channel();
    let (sender, receiver) = watch::channel(None);

    tokio::task::spawn_local(async move {

        let mut result_sender = Some(result_sender);
        let mut proc = None;
        let result: Result<Result<_>> = ui.freeze_if(foreground, true, async {

            let (child, pid, pid_waiter) = if ui.shell.is_queuing_signals() {
                // spawn directly, don't use a pid waiter since it will just get queued
                let child = command.spawn()?;
                let pid = child.id().unwrap();
                (child, pid, None)
            } else {
                let (result, queue_result) = ui.shell.with_queued_signals(|| {
                    command.spawn().map(|child| {
                        let pid = child.id().unwrap();
                        let pid_waiter = Some(crate::shell::process::register_pid(&ui, pid as _, true));
                        (child, pid, pid_waiter)
                    })
                });
                crate::log_if_err(queue_result);
                result?
            };
            let child = proc.insert(child);

            let stdin  = child.stdin.take().map(|s| WriteableFile(Some(BufWriter::new(s))));
            let stdout = child.stdout.take().map(|s| ReadableFile(Some(BufReader::new(s)), false));
            let stderr = child.stderr.take().map(|s| ReadableFile(Some(BufReader::new(s)), false));

            let _ = result_sender.take().unwrap().send(Ok((pid, stdin, stdout, stderr)));

            let code = if let Some(pid_waiter) = pid_waiter {
                // if queuing is enabled then the pid_waiter won't work
                tokio::select!(
                    code = pid_waiter => code.ok(),
                    status = child.wait() => {
                        crate::shell::process::deregister_pid(&ui, pid as _);
                        status.ok().and_then(|x| x.code())
                    }
                )
            } else {
                child.wait().await.ok().and_then(|x| x.code())
            };
            let code = code.unwrap_or(-1);

            // ignore error
            let _ = sender.send(Some(Ok(code as _)));

            Ok(())
        }).await;

        // ensure the proc is dead
        if let Some(mut proc) = proc.take()
            && matches!(proc.try_wait(), Ok(None))
            && let Err(err) = proc.kill().await
            && err.raw_os_error() != Some(nix::errno::Errno::ESRCH as _)
        {
            crate::log_if_err::<(), _>(Err(err));
        }

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

#[derive(Debug)]
struct OverriddenStream {
    fd: RawFd,
    replacement: RawFd,
    backup: Option<RawFd>,
    closed: bool,
}

impl OverriddenStream {
    fn new<A: AsRawFd, B: IntoRawFd>(fd: &A, replacement: B) -> Self {
        Self {
            fd: fd.as_raw_fd(),
            replacement: replacement.into_raw_fd(),
            backup: None,
            closed: false,
        }
    }

    fn close(&mut self) -> Result<()> {
        nix::unistd::close(self.replacement)?;
        self.closed = true;
        Ok(())
    }

    fn override_fd(&mut self) -> Result<()> {
        let backup = nix::unistd::dup(self.fd)?;
        nix::unistd::dup2(self.replacement, self.fd)?;
        self.backup = Some(backup);
        self.close()?;
        Ok(())
    }

    fn restore(&mut self) -> Result<()> {
        if let Some(backup) = self.backup {
            nix::unistd::dup2(backup, self.fd)?;
            nix::unistd::close(backup)?;
        } else if !self.closed {
            self.close()?;
        }
        Ok(())
    }
}

async fn shell_run(ui: Ui, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
    match lua.from_value(val)? {
        ShellRunArgs::Full(args) if args.subshell => {
            subshell_run(ui, lua, args.args, args.opts).await
        },
        ShellRunArgs::Full(args) => {
            shell_run_with_args(ui, lua, ShellRunCmd::Simple(args.args), args.opts).await
        },
        ShellRunArgs::Simple(args) => {
            shell_run_with_args(ui, lua, ShellRunCmd::Simple(args), Default::default()).await
        },
    }
}

macro_rules! stdio_pipe {
    ($args:expr, $name:ident, true) => (
        stdio_pipe!($args, $name, File::create("/dev/null"), {
            let (send, recv) = tokio::net::unix::pipe::pipe()?;
            let send = WriteableFile(Some(BufWriter::new(send)));
            (Some(send), Some(OverriddenStream::new(&std::io::$name(), recv.into_nonblocking_fd()?)))
        })
    );
    ($args:expr, $name:ident, false) => (
        stdio_pipe!($args, $name, File::open("/dev/null"), {
            let (send, recv) = tokio::net::unix::pipe::pipe()?;
            let recv = ReadableFile(Some(BufReader::new(recv)), false);
            (Some(recv), Some(OverriddenStream::new(&std::io::$name(), send.into_nonblocking_fd()?)))
        })
    );
    ($args:expr, $name:ident, $null:expr, $piped:expr) => (
        match $args.$name {
            Stdio::inherit => (None, None),
            Stdio::null => (None, Some(OverriddenStream::new(&std::io::$name(), $null?))),
            Stdio::piped => $piped,
        }
    );
}

pub async fn subshell_run(mut ui: Ui, lua: Lua, cmd: BString, args: FullShellRunOpts) -> Result<LuaMultiValue> {
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
    let streams = [stdin.1, stdout.1, stderr.1];

    tokio::task::spawn_local(async move {
        let mut result_sender = Some(result_sender);
        let result: Result<Result<_>> = ui.freeze_if(foreground, true, async {
            // fork it now to get the pid
            let redirections = streams.iter().flatten().map(|s| (s.fd, s.replacement)).collect();
            let pid = ui.shell.exec_subshell(cmd, false, redirections)? as _;
            // send streams back to caller
            let _ = result_sender.take().unwrap().send(Ok((pid, stdin.0, stdout.0, stderr.0)));
            // get the status
            let pid_waiter = crate::shell::process::register_pid(&ui, pid as _, false);
            let code = match ui.shell.check_pid_status(pid as _) {
                None | Some(-1) => pid_waiter.await.unwrap_or(-1) as _,
                Some(code) => code as _,
            };

            let _ = sender.send(Some(Ok(code)));
            Ok(())
        }).await;

        if let Err(err) = result {
            if let Some(result_sender) = result_sender {
                let _ = result_sender.send(Err(err));
            } else {
                ui.report_error::<(), _>(Err(err));
            }
        }

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

pub async fn shell_run_with_args(mut ui: Ui, lua: Lua, cmd: ShellRunCmd, args: FullShellRunOpts) -> Result<LuaMultiValue> {
    let foreground = args.foreground.unwrap_or(
        matches!(args.stdin, Stdio::inherit)
        || matches!(args.stdout, Stdio::inherit)
        || matches!(args.stderr, Stdio::inherit)
    );

    let result = {
        let ui = ui.clone();
        ui.clone().shell.trampoline_out_callback(move |state| {
            state.shell_loop(async move {
                ui.freeze_if(foreground, true, async {

                    let stdin = stdio_pipe!(args, stdin, true);
                    let stdout = stdio_pipe!(args, stdout, false);
                    let stderr = stdio_pipe!(args, stderr, false);

                    let mut streams = [stdin.1, stdout.1, stderr.1];

                    // wrap the whole thing in a do_run to ensure fd restoration happens in the
                    // correct order
                    // no forking, override fds in place
                    let result = streams.iter_mut().flatten().try_for_each(|s| s.override_fd());
                    if let Err(result) = result {
                        let mut result = Err(result);
                        // didnt work, restore any backups
                        if let Err(e) = streams.iter_mut().flatten().try_for_each(|s| s.restore()) {
                            result = result.context(e);
                        }
                        return result
                    }

                    let code = match cmd {
                        ShellRunCmd::Simple(cmd) => ui.shell.exec(cmd.into()),
                        ShellRunCmd::Function{func, args, arg0} => {
                            let arg0 = arg0.map(|x| x.into());
                            let args = args.into_iter().map(|x| x.into()).collect();
                            ui.shell.exec_function(func.clone(), arg0, args).into()
                        },
                    };

                    // finished, restore any backups
                    let mut errors = [Ok(()), Ok(()), Ok(())];
                    for (s, e) in streams.iter_mut().zip(errors.iter_mut()) {
                        if let Some(s) = s {
                            *e = s.restore();
                        }
                    }

                    Ok((code, errors))
                }).await

            })
        }).await
    };

    let (code, errors) = result.unwrap()???;

    let mut drawn = false;
    for err in errors {
        drawn = ui.report_error(err) || drawn;
    }
    if foreground && ! drawn {
        ui.queue_draw();
    }

    Ok(lua.pack_multi((
        code,
    ))?)
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("__spawn", spawn)?;
    ui.set_lua_async_fn("__shell_run", shell_run)?;
    ui.set_lua_async_fn("__zpty", zpty::zpty)?;

    Ok(())
}
