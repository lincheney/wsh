use std::sync::Arc;
use bstr::BString;
use crate::ui::Ui;
use anyhow::Result;
use mlua::{prelude::*, UserData, UserDataMethods};

struct Function {
    inner: Arc<crate::shell::Function>,
    shell: Arc::<crate::shell::ShellClient>,
}

impl UserData for Function {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_meta_method_mut(mlua::MetaMethod::Call, |_lua, func, args: Option<_>| async move {
            let args = args.unwrap_or(vec![]);
            Ok(func.shell.exec_function(func.inner.clone(), args).await)
        });
    }
}

async fn make_sh_function(ui: Ui, lua: Lua, code: BString) -> Result<LuaValue> {
    let shell = ui.shell.clone();
    let func = shell.make_function(code).await?;
    Ok(lua.pack(Function {
        inner: func,
        shell,
    })?)
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("make_sh_function", make_sh_function)?;

    Ok(())
}

