use crate::lua::LuaWrapper;
use crate::shell::{MetaString};
use std::rc::Rc;
use bstr::BString;
use crate::ui::{Ui, WeakUi};
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods};
use serde::{Deserialize};
use super::process::{shell_run_with_args, ShellRunCmd};

pub struct Function {
    inner: Rc<crate::shell::Function>,
    ui: WeakUi,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FunctionArgs {
    Simple(Vec<BString>),
    Full{
        args: Vec<BString>,
        foreground: Option<bool>,
    },
}

impl UserData for Function {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_meta_method_mut(mlua::MetaMethod::Call, |lua, func, mut args: LuaMultiValue| async move {

            let (args, foreground) = if args.is_empty() {
                (vec![], true)
            } else if args.len() == 1 {
                let args = args.pop_front().unwrap();
                match lua.from_value(args)? {
                    FunctionArgs::Simple(args) => (args, true),
                    FunctionArgs::Full{args, foreground} => (args, foreground.unwrap_or(true)),
                }
            } else {
                let mut args: mlua::Variadic<BString> = lua.unpack_multi(args)?;
                (args.split_off(0), true)
            };

            let ui = Ui::try_upgrade(&func.ui).map_err(crate::lua::lua_error)?;
            let cmd = ShellRunCmd::Function{func: func.inner.clone(), args, arg0: None};
            let result = shell_run_with_args(ui, lua, cmd, foreground).await;
            result.map_err(|e| mlua::Error::RuntimeError(e.to_string()))
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

