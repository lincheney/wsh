use std::os::fd::AsRawFd;
use std::os::unix::process::ExitStatusExt;
use std::collections::HashMap;
use anyhow::Result;
use mlua::{prelude::*, Function, UserData, UserDataMethods};
use tokio::io::{BufReader, BufWriter, AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, AsyncBufReadExt};
use tokio::process::Command;
use serde::{Deserialize};
use crate::ui::Ui;
use crate::shell::Shell;

struct ReadableFile<T>(BufReader<T>);

impl<T: AsyncRead + AsRawFd + std::marker::Unpin + mlua::MaybeSend + 'static> UserData for ReadableFile<T> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("as_fd", |_lua, file, ()| {
            Ok(file.0.get_ref().as_raw_fd())
        });

        methods.add_async_method_mut("read", |lua, mut file, _val: ()| async move {
            let mut buf = [0; 4096];
            let n = file.0.read(&mut buf).await?;
            Ok(if n == 0 { None } else { Some(lua.create_string(&buf[..n])?) })
        });

        methods.add_async_method_mut("read_until", |lua, mut file, val: u8| async move {
            let mut buf = vec![];
            let n = file.0.read_until(val, &mut buf).await?;
            Ok(if n == 0 { None } else { Some(lua.create_string(&buf[..n])?) })
        });

        methods.add_async_method_mut("read_line", |lua, mut file, _val: ()| async move {
            let mut buf = vec![];
            let n = file.0.read_until(b'\n', &mut buf).await?;
            Ok(if n == 0 { None } else { Some(lua.create_string(&buf[..n])?) })
        });

    }
}

struct WriteableFile<T>(BufWriter<T>);

impl<T: AsyncWrite + AsRawFd + std::marker::Unpin + mlua::MaybeSend + 'static> UserData for WriteableFile<T> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("as_fd", |_lua, file, ()| {
            Ok(file.0.get_ref().as_raw_fd())
        });

        methods.add_async_method_mut("write", |_lua, mut file, val: LuaString| async move {
            file.0.write_all(&*val.as_bytes()).await?;
            Ok(())
        });

    }
}

struct Process(tokio::process::Child);

impl UserData for Process {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_method("pid", |_lua, proc, ()| {
            Ok(proc.0.id())
        });

        methods.add_async_method_mut("wait", |_lua, mut proc, ()| async move {
            Ok(proc.0.wait().await?.into_raw())
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
struct SpawnArgs {
    args: Vec<String>,
    env: Option<HashMap<String, String>>,
    clear_env: bool,
    cwd: Option<String>,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
}


pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {
    let schedule = ui.make_lua_fn(shell, |ui, shell, _lua, cb: Function| {
        ui.call_lua_fn(shell.clone(), false, cb, ());
        Ok(())
    }).await?;

    let ui = ui.borrow().await;

    let tbl = ui.lua.create_table()?;
    ui.lua_api.set("async", &tbl)?;

    tbl.set("sleep", ui.lua.create_async_function(|_, millis: u64| async move {
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        Ok(())
    })?)?;

    tbl.set("schedule", schedule)?;

    tbl.set("spawn", ui.lua.create_function(|lua, val: LuaValue| {
        let args: SpawnArgs = lua.from_value(val)?;
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

        let mut proc = command.spawn()?;
        let stdin = proc.stdin.take().map(|s| WriteableFile(BufWriter::new(s)));
        let stdout = proc.stdout.take().map(|s| ReadableFile(BufReader::new(s)));
        let stderr = proc.stderr.take().map(|s| ReadableFile(BufReader::new(s)));
        Ok((
            Process(proc),
            stdin,
            stdout,
            stderr,
        ))

    })?)?;


    let tbl = ui.lua.create_table()?;
    ui.lua_api.set("log", &tbl)?;

    tbl.set("debug", ui.lua.create_function(|_, val: LuaValue| { log::debug!("{:?}", val); Ok(()) })?)?;
    tbl.set("info", ui.lua.create_function(|_, val: LuaValue| { log::info!("{:?}", val); Ok(()) })?)?;
    tbl.set("warn", ui.lua.create_function(|_, val: LuaValue| { log::warn!("{:?}", val); Ok(()) })?)?;
    tbl.set("error", ui.lua.create_function(|_, val: LuaValue| { log::error!("{:?}", val); Ok(()) })?)?;

    Ok(())
}
