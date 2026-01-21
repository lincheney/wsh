use bstr::BString;
use std::time::SystemTime;
use std::default::Default;
use anyhow::{Result};
use mlua::{prelude::*};
use tokio::io::{BufStream};
use tokio::sync::{watch};
use serde::{Deserialize};
use crate::ui::{Ui};
use crate::lua::asyncio::{ReadWriteFile};

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

pub struct Zpty {
    name: String,
}

pub async fn zpty(ui: Ui, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
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
    let zpty = ui.shell.zpty(name.clone().into(), cmd.into(), opts).await?;

    // do not drop the pty fd as zsh will do it for us
    // so we dup the fd to one we own instead
    let pty = crate::utils::dup_fd(unsafe{ std::os::fd::BorrowedFd::borrow_raw(zpty.fd) })?;
    let pty: std::fs::File = pty.into();
    let pty = tokio::fs::File::from_std(pty);
    let pty = ReadWriteFile{
        inner: Some(BufStream::new(pty)),
        is_tty_master: true,
    };

    let zpty_name = name.clone().into();
    let pid = zpty.pid;
    tokio::task::spawn(async move {
        // get the status
        let pid_waiter = crate::shell::process::register_pid(pid as _, false);
        let code = match ui.shell.check_pid_status(pid as _).await {
            None | Some(-1) => pid_waiter.await.unwrap_or(-1),
            Some(code) => code,
        };
        // send the code out
        let _ = sender.send(Some(Ok(code as _)));

        // delete the pty once it has finished
        // this will close the original zpty fds
        // which is ok for us since we have dup-ed them
        ui.shell.zpty_delete(zpty_name).await
    });

    Ok(lua.pack_multi((
        super::Process{
            pid,
            result: super::CommandResult{ inner: receiver },
            zpty: Some(Zpty{ name }),
        },
        pty,
    ))?)
}
