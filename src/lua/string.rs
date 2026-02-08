use anyhow::Result;
use mlua::{prelude::*};
use crate::ui::Ui;
use bstr::{ByteSlice};

fn len(_lua: &Lua, string: LuaString) -> LuaResult<usize> {
    Ok(string.as_bytes().grapheme_indices().count())
}

fn to_byte_pos(_lua: &Lua, (string, index): (mlua::String, usize)) -> LuaResult<(Option<usize>, Option<usize>)> {
    Ok(if let Some((s, e, _)) = string.as_bytes().grapheme_indices().nth(index.saturating_sub(1)) {
        (Some(s + 1), Some(e))
    } else {
        (None, None)
    })
}

fn from_byte_pos(_lua: &Lua, (string, index): (mlua::String, usize)) -> LuaResult<Option<usize>> {
    let index = index.saturating_sub(1);
    for (i, (_, e, _)) in string.as_bytes().grapheme_indices().enumerate() {
        if e > index {
            return Ok(Some(i + 1))
        }
    }
    Ok(None)
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    let lua_api = ui.get_lua_api()?;
    let tbl = ui.lua.create_table()?;
    lua_api.set("str", &tbl)?;

    tbl.set("len", ui.lua.create_function(len)?)?;
    tbl.set("to_byte_pos", ui.lua.create_function(to_byte_pos)?)?;
    tbl.set("from_byte_pos", ui.lua.create_function(from_byte_pos)?)?;

    Ok(())
}

