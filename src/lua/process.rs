use std::str::FromStr;
use std::os::unix::process::ExitStatusExt;
use std::collections::HashMap;
use std::default::Default;
use std::os::fd::{RawFd, AsRawFd, IntoRawFd};
use std::fs::File;
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods};
use tokio::io::{BufReader, BufWriter};
use tokio::process::Command;
use tokio::sync::oneshot;
use serde::{Deserialize, Deserializer, de};
use crate::ui::{Ui, ThreadsafeUiInner};
use super::asyncio::{ReadableFile, WriteableFile};

#[derive(Debug, Copy, Clone)]
struct Signal(nix::sys::signal::Signal);
#[derive(Debug, Deserialize, Clone)]
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
    inner: Option<oneshot::Receiver<std::io::Result<i32>>>,
}

impl CommandResult {
    async fn wait(&mut self) -> LuaResult<Option<i32>> {
        if let Some(waiter) = self.inner.take() {
            let result = waiter.await.map_err(|e| LuaError::RuntimeError(format!("{e}")))??;
            Ok(Some(result))
        } else {
            Ok(None)
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

struct Process {
    pid: u32,
    result: CommandResult,
}

impl UserData for Process {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("pid", |_lua, proc, ()| {
            Ok(proc.pid)
        });

        methods.add_async_method_mut("wait", |_lua, mut proc, ()| async move {
            proc.result.wait().await
        });

        methods.add_method("kill", |lua, proc, signal: LuaValue| {
            let signal: Signal = lua.from_value(signal)?;
            let pid = nix::unistd::Pid::from_raw(proc.pid as _);
            nix::sys::signal::kill(pid, signal.0).map_err(|e| LuaError::RuntimeError(format!("{e}")))
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
struct FullCommandSpawnArgs {
    args: String,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
    foreground: bool,
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
    foreground: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CommandSpawnArgs {
    Simple(String),
    Full(FullCommandSpawnArgs),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SpawnArgs {
    Simple(Vec<String>),
    Full(FullSpawnArgs),
}

async fn spawn(mut ui: Ui, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
    let args = match lua.from_value(val)? {
        SpawnArgs::Full(args) => args,
        SpawnArgs::Simple(args) => FullSpawnArgs{args, ..std::default::Default::default()},
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

    let (result_sender, result_receiver) = oneshot::channel();
    let (sender, receiver) = oneshot::channel();

    tokio::spawn(async move {

        let mut result_sender = Some(result_sender);
        let result: Result<_> = (async || {

            let mut foreground_lock = if args.foreground && !crate::is_forked() {
                // this essentially locks ui
                let this = ui.unlocked.read();
                {
                    let mut ui = this.inner.borrow_mut().await;
                    ui.events.pause().await;
                    ui.deactivate()?;
                }
                Some(this.has_foreground_process.clone().lock_owned().await)
            } else {
                None
            };

            let mut proc = command.spawn()?;
            let pid = proc.id().unwrap();
            ui.shell.lock().await.add_pid(pid as _);

            let stdin  = proc.stdin.take().map(|s| WriteableFile(Some(BufWriter::new(s))));
            let stdout = proc.stdout.take().map(|s| ReadableFile(Some(BufReader::new(s))));
            let stderr = proc.stderr.take().map(|s| ReadableFile(Some(BufReader::new(s))));
            let _ = result_sender.take().unwrap().send(Ok((pid, stdin, stdout, stderr)));

            // zsh runs wait() in a SIGCHLD handler as well
            // so if it gets the status first, we have to fetch it out of the job table
            let result = match proc.wait().await {
                Ok(e) => Ok(e),
                Err(e) => {
                    if e.raw_os_error().is_some_and(|e| e == nix::errno::Errno::ECHILD as _) {
                        if let Some(proc) = ui.shell.lock().await.find_pid(pid as _) {
                            Ok(std::process::ExitStatus::from_raw(proc.status))
                        } else {
                            Err(e)
                        }
                    } else {
                        Err(e)
                    }
                },
            };

            if foreground_lock.take().is_some() {
                ui.report_error(true, ui.activate().await).await;
                ui.get().inner.borrow_mut().await.events.resume().await;
            }
            // ignore error
            let _ = sender.send(result.map(|r| r.into_raw()));

            Ok(())
        })().await;

        if let Some((result_sender, err)) = result_sender.zip(result.err()) {
            let _ = result_sender.send(Err(err));
        }

    });

    let (pid, stdin, stdout, stderr) = result_receiver.await.unwrap()?;
    Ok(lua.pack_multi((
        Process{pid, result: CommandResult{ inner: Some(receiver) }},
        stdin,
        stdout,
        stderr,
    ))?)

}

fn override_fd<A: AsRawFd, B: IntoRawFd>(old: A, new: B) -> Result<RawFd> {
    let old = old.as_raw_fd();
    let new = new.into_raw_fd();
    let backup = nix::unistd::dup(old)?;
    nix::unistd::dup2(new, old)?;
    nix::unistd::close(new)?;
    Ok(backup)
}

fn restore_fd<A: AsRawFd>(old: RawFd, new: A) -> Result<()> {
    let new = new.as_raw_fd();
    nix::unistd::dup2(old, new)?;
    nix::unistd::close(old)?;
    Ok(())
}

async fn shell_run(mut ui: Ui, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
    let args = match lua.from_value(val)? {
        CommandSpawnArgs::Full(args) => args,
        CommandSpawnArgs::Simple(args) => FullCommandSpawnArgs{args, ..Default::default()},
    };

    let (result_sender, result_receiver) = oneshot::channel();
    let (sender, receiver) = oneshot::channel();

    // run this in a thread
    tokio::task::spawn(async move {

        let mut result_sender = Some(result_sender);
        let result: Result<_> = (async || {

            let mut foreground_lock = if args.foreground && !crate::is_forked() {
                // this essentially locks ui
                let this = ui.unlocked.read();
                {
                    let mut ui = this.inner.borrow_mut().await;
                    ui.events.pause().await;
                    ui.deactivate()?;
                }
                Some(this.has_foreground_process.clone().lock_owned().await)
            } else {
                None
            };

            macro_rules! stdio_pipe {
                ($name:ident, true) => (
                    stdio_pipe!($name, File::create("/dev/null"), {
                        let (send, recv) = tokio::net::unix::pipe::pipe()?;
                        let send = WriteableFile(Some(BufWriter::new(send)));
                        (Some(send), Some(override_fd(std::io::$name(), recv.into_nonblocking_fd()?)?))
                    })
                );
                ($name:ident, false) => (
                    stdio_pipe!($name, File::open("/dev/null"), {
                        let (send, recv) = tokio::net::unix::pipe::pipe()?;
                        let recv = ReadableFile(Some(BufReader::new(recv)));
                        (Some(recv), Some(override_fd(std::io::$name(), send.into_nonblocking_fd()?)?))
                    })
                );
                ($name:ident, $null:expr, $piped:expr) => (
                    match args.$name {
                        Stdio::inherit => (None, None),
                        Stdio::null => (None, Some(override_fd(std::io::$name(), $null?)?)),
                        Stdio::piped => $piped,
                    }
                );
            }

            let mut shell = ui.shell.lock().await;
            let stdin = stdio_pipe!(stdin, true);
            let stdout = stdio_pipe!(stdout, false);
            let stderr = stdio_pipe!(stderr, false);
            // send streams back to caller
            let _ = result_sender.take().unwrap().send(Ok((stdin.0, stdout.0, stderr.0)));

            // run the scripts
            let code = tokio::task::block_in_place(|| {
                shell.exec(bstr::BStr::new(&args.args)).err().unwrap_or(0)
            });

            // restore stdio
            let stdin_err = stdin.1.map(|stdin| restore_fd(stdin, std::io::stdin()));
            let stdout_err = stdout.1.map(|stdout| restore_fd(stdout, std::io::stdout()));
            let stderr_err = stderr.1.map(|stderr| restore_fd(stderr, std::io::stderr()));
            drop(shell);

            if foreground_lock.take().is_some() {
                ui.report_error(true, ui.activate().await).await;
                ui.get().inner.borrow_mut().await.events.resume().await;
            }

            for err in [stdin_err, stdout_err, stderr_err].into_iter().flatten() {
                ui.report_error(true, err).await;
            }

            // send the code out
            let _ = sender.send(Ok(code as _));

            Ok(())
        })().await;

        if let Some((result_sender, err)) = result_sender.zip(result.err()) {
            let _ = result_sender.send(Err(err));
        }

    });

    let (stdin, stdout, stderr) = result_receiver.await.unwrap()?;

    Ok(lua.pack_multi((
        CommandResult{ inner: Some(receiver) },
        stdin,
        stdout,
        stderr,
    ))?)
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("__spawn", spawn)?;
    ui.set_lua_async_fn("__shell_run", shell_run)?;

    Ok(())
}
