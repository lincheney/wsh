use std::sync::Arc;
use bstr::BString;
use crate::ui::{Ui, DowngradeUi, UpgradeUi};
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods};
use serde::{Deserialize};
use super::process::{shell_run_with_args, FullShellRunOpts, ShellRunCmd};

pub struct Function {
    inner: Arc<crate::shell::Function>,
    ui: crate::ui::WeakUi,
}

#[derive(Debug, Default, Deserialize)]
struct FullFunctionArgs {
    args: Vec<String>,
    #[serde(flatten)]
    opts: FullShellRunOpts,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FunctionArgs {
    Simple(Vec<String>),
    Full(FullFunctionArgs),
}

impl UserData for Function {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_meta_method_mut(mlua::MetaMethod::Call, |lua, func, mut args: LuaMultiValue| async move {

            let (args, opts) = if args.is_empty() {
                (vec![], None)
            } else if args.len() == 1 {
                let args = args.pop_front().unwrap();
                match lua.from_value(args)? {
                    FunctionArgs::Simple(args) => {
                        (args, None)
                    },
                    FunctionArgs::Full(args) => {
                        (args.args, Some(args.opts))
                    },
                }
            } else {
                let mut args: mlua::Variadic<String> = lua.unpack_multi(args)?;
                (args.split_off(0), None)
            };

            let ui = func.ui.try_upgrade()?;
            let cmd = ShellRunCmd::Function{func: func.inner.clone(), args};
            let result = shell_run_with_args(ui, lua, cmd, opts.unwrap_or_default()).await;
            result.map_err(|e| mlua::Error::RuntimeError(format!("{e}")))

        });
    }
}

async fn make_zsh_function(ui: Ui, lua: Lua, code: BString) -> Result<LuaValue> {
    let func = ui.shell.make_function(code).await?;
    Ok(lua.pack(Function {
        inner: func,
        ui: ui.downgrade(),
    })?)
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("make_zsh_function", make_zsh_function)?;

    Ok(())
}

