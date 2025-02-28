use std::os::unix::process::ExitStatusExt;
use std::collections::HashMap;
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods};
use tokio::io::{BufReader, BufWriter};
use tokio::process::Command;
use tokio::sync::oneshot;
use serde::{Deserialize};
use crate::ui::Ui;
use crate::shell::Shell;
use super::asyncio::{ReadableFile, WriteableFile};

struct Process {
    pid: u32,
    result: Option<oneshot::Receiver<std::io::Result<std::process::ExitStatus>>>,
}

impl UserData for Process {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("pid", |_lua, proc, ()| {
            Ok(proc.pid)
        });

        methods.add_async_method_mut("wait", |_lua, mut proc, ()| async move {
            if let Some(waiter) = proc.result.take() {
                let result = waiter.await.map_err(|e| LuaError::RuntimeError(format!("{}", e)))??;
                Ok(Some(result.into_raw()))
            } else {
                Ok(None)
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
struct FullSpawnArgs {
    args: Vec<String>,
    env: Option<HashMap<String, String>>,
    clear_env: bool,
    cwd: Option<String>,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
    foreground: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SpawnArgs {
    Simple(Vec<String>),
    Full(FullSpawnArgs),
}

async fn spawn(ui: Ui, shell: Shell, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
    let args = match lua.from_value(val)? {
        SpawnArgs::Full(args) => args,
        SpawnArgs::Simple(args) => FullSpawnArgs{args, ..std::default::Default::default()},
    };

    let arg0 = args.args.first().ok_or_else(|| LuaError::RuntimeError("no args given".to_owned()))?;
    let mut command = Command::new(arg0);
    if args.args.len() > 1 {
        command.args(&args.args[1..]);
    }
    if args.clear_env {
        command.env_clear();
    }
    if let Some(env) = args.env {
        for (k, v) in env.iter() {
            command.env(k,v);
        }
    }
    if let Some(cwd) = args.cwd {
        command.current_dir(cwd);
    }
    command.stdin(args.stdin);
    command.stdout(args.stdout);
    command.stderr(args.stderr);

    let lock = if args.foreground {
        // this essentially locks ui
        let mut ui = ui.clone();
        let lock = ui.borrow_mut().await.events.lock_owned().await;
        ui.deactivate().await?;
        Some((ui, lock))
    } else {
        None
    };

    let mut proc = command.spawn()?;
    let pid = proc.id().unwrap();
    shell.lock().await.add_pid(pid as _);

    let stdin  = proc.stdin.take().map(|s| WriteableFile(Some(BufWriter::new(s))));
    let stdout = proc.stdout.take().map(|s| ReadableFile(Some(BufReader::new(s))));
    let stderr = proc.stderr.take().map(|s| ReadableFile(Some(BufReader::new(s))));

    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {

        // zsh runs wait() in a SIGCHLD handler as well
        // so if it gets the status first, we have to fetch it out of the job table
        let result = match proc.wait().await {
            Ok(e) => Ok(e),
            Err(e) => {
                if e.raw_os_error().is_some_and(|e| e == nix::errno::Errno::ECHILD as _) {
                    if let Some(proc) = shell.lock().await.find_pid(pid as _) {
                        Ok(std::process::ExitStatus::from_raw(proc.status))
                    } else {
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            },
        };

        if let Some((ui, lock)) = lock {
            ui.activate().await;
            drop(lock);
        }
        sender.send(result);
    });

    Ok(lua.pack_multi((
        Process{pid, result: Some(receiver)},
        stdin,
        stdout,
        stderr,
    ))?)

}


pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("__spawn", shell, spawn).await?;

    Ok(())
}
