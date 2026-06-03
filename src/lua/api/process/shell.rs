use crate::lua::LuaWrapper;
use crate::shell::{MetaString};
use std::rc::Rc;
use bstr::BString;
use anyhow::{Result};
use mlua::{prelude::*};
use serde::{Deserialize};
use crate::ui::{Ui};

pub enum ShellRunCmd {
    Simple(BString),
    Function{
        func: Rc<crate::shell::Function>,
        args: Vec<BString>,
        arg0: Option<BString>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ShellRunArgs {
    Simple(BString),
    Detailed{
        command: BString,
        foreground: Option<bool>,
    },
}

async fn shell_run(ui: Ui, lua: Lua, val: LuaValue) -> Result<i64> {
    let (command, foreground) = match lua.from_value(val)? {
        ShellRunArgs::Detailed{command, foreground} => (command, foreground.unwrap_or(true)),
        ShellRunArgs::Simple(command) => (command, true),
    };
    shell_run_with_args(ui, lua, ShellRunCmd::Simple(command), foreground).await
}

pub async fn shell_run_with_args(ui: Ui, _lua: Lua, cmd: ShellRunCmd, foreground: bool) -> Result<i64> {

    let code = ui.clone().shell.trampoline_out_callback(move |ui, token| {
        ui.clone().shell_loop(async move {
            ui.freeze_if(foreground, true, async {
                match cmd {
                    ShellRunCmd::Simple(command) => ui.shell.exec(token, command.into()),
                    ShellRunCmd::Function{func, args, arg0, ..} => {
                        let arg0: Option<MetaString> = arg0.map(|x| x.into());
                        let arg0 = arg0.as_ref().map(|x| x.as_ref());
                        let args: Vec<_> = args.into_iter().map(MetaString::from).collect();
                        ui.shell.exec_function(token, func.clone(), arg0, args.iter()).into()
                    },
                }
            }).await
        })
    }).await???;

    if foreground {
        ui.queue_draw();
    }

    Ok(code)
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_async_fn("__shell_run", shell_run)?;

    Ok(())
}
