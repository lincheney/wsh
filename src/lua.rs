use std::os::fd::AsRawFd;
use std::os::unix::process::ExitStatusExt;
use std::collections::HashMap;
use anyhow::Result;
use mlua::{prelude::*, Function, UserData, UserDataMethods};
use tokio::io::{BufReader, BufWriter, AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, AsyncBufReadExt};
use tokio::process::Command;
use tokio::sync::oneshot;
use serde::{Deserialize};
use crate::ui::Ui;
use crate::shell::Shell;

struct ReadableFile<T>(Option<BufReader<T>>);

impl<T: AsyncRead + AsRawFd + std::marker::Unpin + mlua::MaybeSend + 'static> UserData for ReadableFile<T> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("as_fd", |_lua, file, ()| {
            Ok(file.0.as_ref().map(|x| x.get_ref().as_raw_fd()))
        });

        methods.add_method_mut("close", |_lua, file, ()| {
            file.0 = None;
            Ok(())
        });

        methods.add_async_method_mut("read", |lua, mut file, _val: ()| async move {
            if let Some(file) = file.0.as_mut() {
                let mut buf = [0; 4096];
                let n = file.read(&mut buf).await?;
                if n != 0 {
                    return Ok(Some(lua.create_string(&buf[..n])?))
                }
            }
            Ok(None)
        });

        methods.add_async_method_mut("read_all", |lua, mut file, _val: ()| async move {
            if let Some(file) = file.0.as_mut() {
                let mut buf = vec![];
                loop {
                    let start = buf.len();
                    buf.resize(buf.len() + 4096, 0);
                    let slice = &mut buf[start..];
                    let n = file.read(slice).await?;
                    buf.resize(start + n, 0);
                    if n == 0 {
                        return Ok(Some(lua.create_string(&buf)?));
                    }
                }
            }
            Ok(None)
        });

        methods.add_async_method_mut("read_until", |lua, mut file, val: u8| async move {
            if let Some(file) = file.0.as_mut() {
                let mut buf = vec![];
                let n = file.read_until(val, &mut buf).await?;
                if n != 0 {
                    return Ok(Some(lua.create_string(&buf[..n])?))
                }
            }
            Ok(None)
        });

        methods.add_async_method_mut("read_line", |lua, mut file, _val: ()| async move {
            if let Some(file) = file.0.as_mut() {
                let mut buf = vec![];
                let n = file.read_until(b'\n', &mut buf).await?;
                if n != 0 {
                    return Ok(Some(lua.create_string(&buf[..n])?))
                }
            }
            Ok(None)
        });

    }
}

struct WriteableFile<T>(Option<BufWriter<T>>);

impl<T: AsyncWrite + AsRawFd + std::marker::Unpin + mlua::MaybeSend + 'static> UserData for WriteableFile<T> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("as_fd", |_lua, file, ()| {
            Ok(file.0.as_ref().map(|x| x.get_ref().as_raw_fd()))
        });

        methods.add_method_mut("close", |_lua, file, ()| {
            file.0 = None;
            Ok(())
        });

        methods.add_async_method_mut("write", |_lua, mut file, val: LuaString| async move {
            if let Some(file) = file.0.as_mut() {
                file.write_all(&*val.as_bytes()).await?;
                file.flush().await?;
            }
            Ok(())
        });

    }
}

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


pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {
    let schedule = ui.make_lua_fn(shell, |ui, shell, _lua, cb: Function| {
        ui.call_lua_fn(shell.clone(), false, cb, ());
        Ok(())
    }).await?;

    let spawn = ui.make_lua_async_fn(shell, |ui, shell, lua, val: LuaValue| async move {
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

        let stdin = proc.stdin.take().map(|s| WriteableFile(Some(BufWriter::new(s))));
        let stdout = proc.stdout.take().map(|s| ReadableFile(Some(BufReader::new(s))));
        let stderr = proc.stderr.take().map(|s| ReadableFile(Some(BufReader::new(s))));

        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {

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

        Ok((
            Process{pid, result: Some(receiver)},
            stdin,
            stdout,
            stderr,
        ))

    }).await?;

    let ui = ui.borrow().await;

    let tbl = ui.lua.create_table()?;
    ui.lua_api.set("async", &tbl)?;

    tbl.set("sleep", ui.lua.create_async_function(|_, millis: u64| async move {
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        Ok(())
    })?)?;

    tbl.set("schedule", schedule)?;

    tbl.set("__spawn", spawn)?;


    let tbl = ui.lua.create_table()?;
    ui.lua_api.set("log", &tbl)?;

    tbl.set("debug", ui.lua.create_function(|_, val: LuaValue| { log::debug!("{:?}", val); Ok(()) })?)?;
    tbl.set("info", ui.lua.create_function(|_, val: LuaValue| { log::info!("{:?}", val); Ok(()) })?)?;
    tbl.set("warn", ui.lua.create_function(|_, val: LuaValue| { log::warn!("{:?}", val); Ok(()) })?)?;
    tbl.set("error", ui.lua.create_function(|_, val: LuaValue| { log::error!("{:?}", val); Ok(()) })?)?;

    Ok(())
}
