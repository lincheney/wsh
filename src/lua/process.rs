use bstr::BString;
use std::sync::Arc;
use std::time::SystemTime;
use std::str::FromStr;
use std::collections::HashMap;
use std::default::Default;
use std::os::fd::{RawFd, AsRawFd, IntoRawFd, FromRawFd};
use std::fs::File;
use anyhow::{Result, Context};
use mlua::{prelude::*, UserData, UserDataMethods};
use tokio::io::{
    BufReader,
    BufWriter,
    BufStream,
};
use tokio::process::Command;
use tokio::sync::{oneshot, watch};
use serde::{Deserialize, Deserializer, de};
use crate::ui::{Ui};
use super::asyncio::{ReadableFile, WriteableFile, ReadWriteFile};

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

struct CommandResult {
    inner: watch::Receiver<Option<std::io::Result<i32>>>,
}

impl CommandResult {
    async fn wait(&mut self) -> LuaResult<i32> {
        match self.inner.wait_for(|x| x.is_some()).await {
            Ok(x) => match x.as_ref().unwrap() {
                Ok(x) => Ok(*x),
                Err(e) => Err(LuaError::RuntimeError(format!("{e}"))),
            },
            Err(_) => Ok(i32::MAX),
        }
    }
}

impl UserData for CommandResult {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method_mut("wait", |_lua, mut proc, ()| async move {
            proc.wait().await
        });
    }
}

struct Zpty {
    name: String,
    shell: Arc<crate::shell::ShellClient>,
}

struct Process {
    pid: u32,
    result: CommandResult,
    #[allow(unused)]
    zpty: Option<Zpty>,
}

impl Drop for Zpty {
    fn drop(&mut self) {
        let name = std::mem::take(&mut self.name);
        let shell = self.shell.clone();
        tokio::task::spawn(async move {
            shell.zpty_delete(name.into()).await
        });
    }
}

impl UserData for Process {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("pid", |_lua, proc, ()| {
            Ok(proc.pid)
        });

        methods.add_method("is_finished", |_lua, proc, ()| {
            Ok(proc.result.inner.borrow().is_some())
        });

        methods.add_async_method_mut("wait", |_lua, mut proc, ()| async move {
            proc.result.wait().await
        });

        methods.add_method("kill", |lua, proc, signal: LuaValue| {
            if proc.result.inner.borrow().is_none() {
                let signal: Signal = lua.from_value(signal)?;
                let pid = nix::unistd::Pid::from_raw(proc.pid as _);
                nix::sys::signal::kill(pid, signal.0).map_err(|e| LuaError::RuntimeError(format!("{e}")))
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
    Subshell(BString),
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

async fn spawn(mut ui: Ui, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
    let args = match lua.from_value(val)? {
        SpawnArgs::Shell(args) => {
            return shell_run_with_args(ui, lua, ShellRunCmd::Subshell(args), Default::default()).await;
        },
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

    tokio::spawn(async move {

        let mut result_sender = Some(result_sender);
        let result: Result<_> = (async || {

            let foreground_lock = if foreground && !crate::is_forked() {
                // this essentially locks ui
                ui.events.read().pause().await;
                ui.prepare_for_unhandled_output().await?;
                Some(ui.has_foreground_process.lock().await)
            } else {
                None
            };

            // prevent sigchld from running
            ui.shell.queue_signals().await;
            let mut proc = command.spawn()?;
            let pid = proc.id().unwrap();
            ui.shell.add_pid(pid as _).await;
            ui.shell.unqueue_signals().await?;

            let stdin  = proc.stdin.take().map(|s| WriteableFile(Some(BufWriter::new(s))));
            let stdout = proc.stdout.take().map(|s| ReadableFile(Some(BufReader::new(s))));
            let stderr = proc.stderr.take().map(|s| ReadableFile(Some(BufReader::new(s))));
            let _ = result_sender.take().unwrap().send(Ok((pid, stdin, stdout, stderr)));
            let code = crate::shell::wait_for_pid(pid as _, &ui.shell).await.unwrap();

            drop(foreground_lock);
            // ignore error
            let _ = sender.send(Some(Ok(code as _)));

            Ok(())
        })().await;

        let mut drawn = false;
        if foreground {
            ui.events.read().resume().await;
            let result = ui.recover_from_unhandled_output().await;
            drawn = ui.report_error(result).await || drawn;
        }

        if let Err(err) = result {
            if let Some(result_sender) = result_sender {
                let _ = result_sender.send(Err(err));
            } else {
                let err: Result<()> = Err(err);
                drawn = ui.report_error(err).await || drawn;
            }
        }

        if foreground && ! drawn {
            ui.try_draw().await;
        }

    });

    let (pid, stdin, stdout, stderr) = result_receiver.await.unwrap()?;
    Ok(lua.pack_multi((
        Process{
            pid,
            result: CommandResult{ inner: receiver },
            zpty: None,
        },
        stdin,
        stdout,
        stderr,
    ))?)

}

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
            shell_run_with_args(ui, lua, ShellRunCmd::Subshell(args.args), args.opts).await
        },
        ShellRunArgs::Full(args) => {
            shell_run_with_args(ui, lua, ShellRunCmd::Simple(args.args), args.opts).await
        },
        ShellRunArgs::Simple(args) => {
            shell_run_with_args(ui, lua, ShellRunCmd::Simple(args), Default::default()).await
        },
    }
}

pub async fn shell_run_with_args(mut ui: Ui, lua: Lua, cmd: ShellRunCmd, args: FullShellRunOpts) -> Result<LuaMultiValue> {
    let (result_sender, result_receiver) = oneshot::channel();
    let (sender, receiver) = watch::channel(None);

    let foreground = args.foreground.unwrap_or(
        matches!(args.stdin, Stdio::inherit)
        || matches!(args.stdout, Stdio::inherit)
        || matches!(args.stderr, Stdio::inherit)
    );

    // run this in a thread
    tokio::task::spawn(async move {

        let mut result_sender = Some(result_sender);
        let mut errors = [Ok(()), Ok(()), Ok(())];
        let result: Result<_> = (async || {

            let foreground_lock = if foreground && !crate::is_forked() {
                // this essentially locks ui
                ui.events.read().pause().await;
                ui.prepare_for_unhandled_output().await?;
                Some(ui.has_foreground_process.lock().await)
            } else {
                None
            };

            macro_rules! stdio_pipe {
                ($name:ident, true) => (
                    stdio_pipe!($name, File::create("/dev/null"), {
                        let (send, recv) = tokio::net::unix::pipe::pipe()?;
                        let send = WriteableFile(Some(BufWriter::new(send)));
                        (Some(send), Some(OverriddenStream::new(&std::io::$name(), recv.into_nonblocking_fd()?)))
                    })
                );
                ($name:ident, false) => (
                    stdio_pipe!($name, File::open("/dev/null"), {
                        let (send, recv) = tokio::net::unix::pipe::pipe()?;
                        let recv = ReadableFile(Some(BufReader::new(recv)));
                        (Some(recv), Some(OverriddenStream::new(&std::io::$name(), send.into_nonblocking_fd()?)))
                    })
                );
                ($name:ident, $null:expr, $piped:expr) => (
                    match args.$name {
                        Stdio::inherit => (None, None),
                        Stdio::null => (None, Some(OverriddenStream::new(&std::io::$name(), $null?))),
                        Stdio::piped => $piped,
                    }
                );
            }

            let stdin = stdio_pipe!(stdin, true);
            let stdout = stdio_pipe!(stdout, false);
            let stderr = stdio_pipe!(stderr, false);

            let mut streams = [stdin.1, stdout.1, stderr.1];

            let is_subshell = matches!(cmd, ShellRunCmd::Subshell(_));
            let code = match cmd {
                cmd @ (ShellRunCmd::Simple(_) | ShellRunCmd::Function{..}) => {

                    let pid = std::process::id();
                    // send streams back to caller
                    let _ = result_sender.take().unwrap().send(Ok((pid, stdin.0, stdout.0, stderr.0)));

                    // no forking, override fds in place
                    let mut result = streams.iter_mut().flatten().try_for_each(|s| s.override_fd());
                    if result.is_err() {
                        // didnt work, restore any backups
                        if let Err(e) = streams.iter_mut().flatten().try_for_each(|s| s.restore()) {
                            result = result.context(e);
                        }
                        return result
                    }

                    match cmd {
                        ShellRunCmd::Simple(cmd) => ui.shell.exec(cmd).await,
                        ShellRunCmd::Function{func, args, arg0} => {
                            ui.shell.exec_function(func.clone(), arg0, args).await as _
                        },
                        ShellRunCmd::Subshell(_) => unreachable!(),
                    }
                },
                ShellRunCmd::Subshell(cmd) => {
                    // fork it now to get the pid
                    let redirections = streams.iter().flatten().map(|s| (s.fd, s.replacement)).collect();
                    let pid = ui.shell.exec_subshell(cmd, false, redirections).await? as _;

                    // send streams back to caller
                    let _ = result_sender.take().unwrap().send(Ok((pid, stdin.0, stdout.0, stderr.0)));

                    crate::shell::wait_for_pid(pid as _, &ui.shell).await.unwrap() as _
                },
            };

            if !is_subshell {
                // finished, restore any backups
                for (s, e) in streams.iter_mut().zip(errors.iter_mut()) {
                    if let Some(s) = s {
                        *e = s.restore();
                    }
                }
            }

            drop(foreground_lock);

            // send the code out
            let _ = sender.send(Some(Ok(code as _)));

            Ok(())
        })().await;

        let mut drawn = false;

        if foreground {
            ui.events.read().resume().await;
            let result = ui.recover_from_unhandled_output().await;
            drawn = ui.report_error(result).await || drawn;
        }

        for err in errors {
            drawn = ui.report_error(err).await || drawn;
        }

        if let Err(err) = result {
            if let Some(result_sender) = result_sender {
                let _ = result_sender.send(Err(err));
            } else {
                let err: Result<()> = Err(err);
                drawn = ui.report_error(err).await || drawn;
            }
        }

        if foreground && ! drawn {
            ui.try_draw().await;
        }

    });

    let (pid, stdin, stdout, stderr) = result_receiver.await.unwrap()?;

    Ok(lua.pack_multi((
        Process{
            pid,
            result: CommandResult{ inner: receiver },
            zpty: None,
        },
        stdin,
        stdout,
        stderr,
    ))?)
}

async fn zpty(ui: Ui, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
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
    let zpty = ui.shell.zpty(name.clone().into(), cmd, opts).await?;

    let pty = unsafe{ tokio::fs::File::from_raw_fd(zpty.fd) };
    let pty = ReadWriteFile{
        inner: Some(BufStream::new(pty)),
        is_tty_master: true,
    };

    let shell = ui.shell.clone();
    let pid = zpty.pid;
    tokio::task::spawn(async move {
        let code = crate::shell::wait_for_pid(pid as _, &ui.shell).await.unwrap();
        // send the code out
        let _ = sender.send(Some(Ok(code as _)));
    });

    Ok(lua.pack_multi((
        Process{
            pid,
            result: CommandResult{ inner: receiver },
            zpty: Some(Zpty{ shell, name }),
        },
        pty,
    ))?)
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("__spawn", spawn)?;
    ui.set_lua_async_fn("__shell_run", shell_run)?;
    ui.set_lua_async_fn("__zpty", zpty)?;

    Ok(())
}
