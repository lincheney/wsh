use crate::lua::LuaWrapper;
use crate::shell::{MetaString};
use std::rc::Rc;
use bstr::BString;
use crate::ui::{Ui, WeakUi};
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods};
use serde::{Deserialize};
use super::process::{shell_run_with_args, ShellRunCmd, Stdio};

pub struct Function {
    inner: Rc<crate::shell::Function>,
    ui: WeakUi,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FullFunctionArgs {
    args: Vec<BString>,
    foreground: Option<bool>,
    stdin: Option<BString>,
    stdout: Stdio,
    stderr: Stdio,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FunctionArgs {
    Simple(Vec<BString>),
    Full(FullFunctionArgs),
}

impl UserData for Function {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_meta_method(mlua::MetaMethod::Call, |lua, func, mut args: LuaMultiValue| async move {

            let (args, foreground, stdin, stdout, stderr) = if args.is_empty() {
                (vec![], None, None, Stdio::inherit, Stdio::inherit)

            } else if args.len() == 1 {
                let arg = args.pop_front().unwrap();
                match lua.from_value(arg)? {
                    FunctionArgs::Simple(args) => (args, None, None, Stdio::inherit, Stdio::inherit),
                    FunctionArgs::Full(f) => (f.args, f.foreground, f.stdin, f.stdout, f.stderr),
                }

            } else {
                let mut args: mlua::Variadic<BString> = lua.unpack_multi(args)?;
                (args.split_off(0), None, None, Stdio::inherit, Stdio::inherit)
            };

            let ui = Ui::try_upgrade(&func.ui).map_err(crate::lua::lua_error)?;
            let cmd = ShellRunCmd::Function { func: func.inner.clone(), args, arg0: None };
            let (code, stdout, stderr) = shell_run_with_args(
                ui,
                lua.clone(),
                cmd,
                foreground,
                stdin,
                stdout,
                stderr,
            ).await.map_err(crate::lua::lua_error)?;
            lua.pack_multi((code, stdout, stderr))
        });
    }
}

fn make_zsh_function(ui: &Ui, lua: &Lua, code: BString) -> Result<LuaValue> {
    let code: MetaString = code.into();
    let func = ui.shell.make_function(code.as_ref())?;
    Ok(lua.pack(Function {
        inner: func,
        ui: ui.downgrade(),
    })?)
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_fn("make_zsh_function", make_zsh_function)?;

    Ok(())
}
