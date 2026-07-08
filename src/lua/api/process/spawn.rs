use std::os::fd::AsRawFd;
use crate::lua::{LuaWrapper, auto_from_lua, lua_error, Array};
use bstr::BString;
use std::str::FromStr;
use std::collections::HashMap;
use std::default::Default;
use anyhow::{Result};
use mlua::{prelude::*, UserData, UserDataMethods, UserDataFields};
use tokio::io::{
    BufReader,
    BufWriter,
};
use tokio::process::Command;
use tokio::sync::{oneshot, watch, RwLock};
use crate::ui::{Ui};
use crate::lua::api::asyncio::{ReadableFile, WriteableFile};
use super::subshell;

#[derive(Debug, Copy, Clone)]
struct Signal(nix::sys::signal::Signal);

impl FromLua for Signal {
    fn from_lua(value: LuaValue, lua: &Lua) -> LuaResult<Self> {
        let signal = match mlua::Either::<i32, String>::from_lua(value, lua)? {
            mlua::Either::Left(x) => nix::sys::signal::Signal::try_from(x).map_err(lua_error)?,
            mlua::Either::Right(x) => nix::sys::signal::Signal::from_str(x.as_ref()).map_err(lua_error)?,
        };
        Ok(Signal(signal))
    }
}

#[derive(Clone)]
pub struct CommandResult {
    pub inner: watch::Receiver<Option<Result<i32>>>,
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

pub struct Process {
    pub pid: u32,
    pub result: CommandResult,
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

        methods.add_method("kill", |_lua, proc, signal: Signal| {
            if proc.result.inner.borrow().is_none() {
                let pid = nix::unistd::Pid::from_raw(proc.pid as _);
                if let Err(err) = nix::sys::signal::kill(pid, signal.0)
                    && err != nix::errno::Errno::ESRCH
                {
                    return Err(LuaError::RuntimeError(err.to_string()))
                }
            }
            Ok(())
        });

    }
}

auto_from_lua! {
    #[derive(Debug, Default, Copy, Clone)]
    #[allow(non_camel_case_types)]
    pub enum Stdio {
        #[default]
        inherit,
        piped,
        null,
    }
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

auto_from_lua! {
    #[derive(Debug, Default)]
    struct FullSpawnArgs {
        args: Array<String>,
        stdin: Option<Stdio>,
        stdout: Option<Stdio>,
        stderr: Option<Stdio>,
        env: Option<HashMap<String, String>>,
        clear_env: bool,
        cwd: Option<String>,
        foreground: Option<bool>,
    }
}

auto_from_lua! {
    #[derive(Debug)]
    enum SpawnArgs {
        Full(FullSpawnArgs),
        Simple(Array<String>),
        Shell(BString),
    }
}

async fn spawn(ui: Ui, lua: Lua, val: SpawnArgs) -> Result<LuaMultiValue> {
    let args = match val {
        SpawnArgs::Shell(command) => return subshell::subshell_run_with_args(ui, lua, subshell::FullShellRunArgs{command, ..Default::default()}).await,
        SpawnArgs::Full(args) => args,
        SpawnArgs::Simple(args) => FullSpawnArgs{args, ..Default::default()},
    };
    let first_arg = args.args.0.first().ok_or_else(|| LuaError::RuntimeError("no args given".to_owned()))?;

    let mut command = Command::new(first_arg);
    if args.args.0.len() > 1 {
        command.args(&args.args.0[1..]);
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
    command.stdin(args.stdin.unwrap_or_default());
    command.stdout(args.stdout.unwrap_or_default());
    command.stderr(args.stderr.unwrap_or_default());
    let foreground = args.foreground.unwrap_or(
        matches!(args.stdin.unwrap_or_default(), Stdio::inherit)
        || matches!(args.stdout.unwrap_or_default(), Stdio::inherit)
        || matches!(args.stderr.unwrap_or_default(), Stdio::inherit)
    );

    let (result_sender, result_receiver) = oneshot::channel();
    let (sender, receiver) = watch::channel(None);

    ui.clone().runtime.spawn_local(async move {

        let mut result_sender = Some(result_sender);
        let mut proc = None;
        let result = ui.freeze_if(foreground, true, async {

            let (result, queue_result) = ui.shell.with_queued_signals(|| {
                command.spawn().map(|child| {
                    let pid = child.id().unwrap();
                    let pid_waiter = crate::shell::signals::sigchld::register_pid(&ui, pid as _, true);
                    (child, pid, pid_waiter)
                })
            });
            crate::log_if_err(queue_result);
            let (child, pid, pid_waiter) = result?;

            let mut pid_waiter = match pid_waiter {
                Ok(pid_waiter) => pid_waiter,
                Err(err) => {
                    // ignore error
                    let _ = sender.send(Some(Err(err)));
                    return Ok(());
                },
            };

            let child = proc.insert(child);

            let stdin  = child.stdin.take().map(|s| WriteableFile{
                fd: s.as_raw_fd(),
                inner: RwLock::new(Some(BufWriter::new(s))),
            });
            let stdout = child.stdout.take().map(|s| ReadableFile{
                fd: s.as_raw_fd(),
                inner: RwLock::new(Some(BufReader::new(s))),
                is_tty_master: false,
            });
            let stderr = child.stderr.take().map(|s| ReadableFile{
                fd: s.as_raw_fd(),
                inner: RwLock::new(Some(BufReader::new(s))),
                is_tty_master: false,
            });

            let _ = result_sender.take().unwrap().send(Ok((pid, stdin, stdout, stderr)));

            // if queuing is enabled then the pid_waiter won't work
            let code = tokio::select!(
                code = &mut pid_waiter => code.ok(),
                status = child.wait() => {
                    // if zsh got the code first then status may just be a failure
                    // in that case still need to check pid_waiter
                    let code = if let Ok(status) = status {
                        status.code()
                    } else {
                        pid_waiter.await.ok()
                    };
                    crate::shell::signals::sigchld::deregister_pid(&ui, pid as _)?;
                    code
                }
            );
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
                    crate::log_if_err(ui.report_error::<(), _>(Err(err)));
                }
            },
            _ => (),
        }

    })?;

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
    lua.set_async_fn("__spawn", spawn)?;
    Ok(())
}

