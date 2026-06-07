use crate::lua::LuaWrapper;
use crate::ui::Ui;
use anyhow::Result;
use mlua::prelude::*;

fn tokens_to_lua(tokens: &Vec<crate::shell::Token>, lua: &Lua) -> Result<LuaTable> {
    let tbl = lua.create_table_with_capacity(tokens.len(), 0)?;
    for token in tokens {
        let t = lua.create_table()?;
        t.raw_set("start", token.range.start + 1)?;
        t.raw_set("finish", token.range.end)?;
        if !token.kind.is_none() {
            t.raw_set("kind", token.kind.to_string())?;
        }
        if let Some(children) = &token.children {
            t.raw_set("children", tokens_to_lua(children, lua)?)?;
        }
        tbl.raw_push(t)?;
    }
    Ok(tbl)
}

fn parse(ui: &Ui, lua: &Lua, (val, options): (bstr::BString, Option<LuaValue>)) -> Result<(bool, LuaTable)> {
    let options = if let Some(options) = options {
        lua.from_value(options)?
    } else {
        Default::default()
    };
    let (complete, tokens) = ui.shell.parse(val, options);
    let tokens = tokens_to_lua(&tokens, lua)?;
    Ok((complete, tokens))
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.set_fn("parse", parse)?;

    Ok(())
}
