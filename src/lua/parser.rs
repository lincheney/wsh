use crate::ui::Ui;
use anyhow::Result;
use mlua::prelude::*;

fn tokens_to_lua(tokens: &Vec<crate::shell::Token>, lua: &Lua) -> Result<LuaTable> {
    let tbl = lua.create_table_with_capacity(tokens.len(), 0)?;
    for token in tokens {
        let t = lua.create_table()?;
        t.raw_set("start", token.range.start + 1)?;
        t.raw_set("finish", token.range.end)?;
        if let Some(kind) = &token.kind {
            t.raw_set("kind", kind.to_string())?;
        }
        if let Some(nested) = &token.nested {
            t.raw_set("nested", tokens_to_lua(nested, lua)?)?;
        }
        tbl.raw_push(t)?;
    }
    Ok(tbl)
}

async fn parse(ui: Ui, lua: Lua, (val, options): (bstr::BString, Option<LuaValue>)) -> Result<(bool, LuaTable)> {
    let options = if let Some(options) = options {
        lua.from_value(options)?
    } else {
        Default::default()
    };
    let (complete, tokens) = ui.shell.parse(val, options).await;
    let tokens = tokens_to_lua(&tokens, &lua)?;
    Ok((complete, tokens))
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("parse", parse)?;

    Ok(())
}
