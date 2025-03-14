use crate::ui::Ui;
use crate::shell::Shell;
use anyhow::Result;
use mlua::prelude::*;

async fn parse(_ui: Ui, shell: Shell, lua: Lua, (val, recursive): (bstr::BString, Option<bool>)) -> Result<(bool, LuaTable, LuaTable, LuaTable)> {
    let val = val.as_ref();
    let (complete, tokens) = shell.lock().await.parse(val, recursive.unwrap_or(false));

    let starts = lua.create_table()?;
    let ends = lua.create_table()?;
    let kinds = lua.create_table()?;

    for t in tokens {
        starts.raw_push(t.range.start)?;
        ends.raw_push(t.range.end)?;
        kinds.raw_push(t.kind_as_str())?;
    }

    Ok((complete, starts, ends, kinds))
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("parse", shell, parse).await?;

    Ok(())
}
