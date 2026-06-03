use anyhow::Result;
use mlua::{prelude::*};
use crate::ui::Ui;
use bstr::{ByteSlice};
use unicode_width::{UnicodeWidthStr, UnicodeWidthChar};

fn len(_lua: &Lua, string: LuaString) -> LuaResult<usize> {
    Ok(string.as_bytes().grapheme_indices().count())
}

fn width(_lua: &Lua, string: String) -> LuaResult<usize> {
    Ok(string.width())
}

fn truncate(_lua: &Lua, (mut string, max_width): (String, usize)) -> LuaResult<String> {
    let mut current_width = 0;
    for (i, c) in string.chars().enumerate() {
        let w = c.width().unwrap_or(0);
        if current_width + w > max_width {
            string.truncate(i);
            break;
        }
        current_width += w;
    }
    Ok(string)
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
    tbl.set("width", ui.lua.create_function(width)?)?;
    tbl.set("to_byte_pos", ui.lua.create_function(to_byte_pos)?)?;
    tbl.set("from_byte_pos", ui.lua.create_function(from_byte_pos)?)?;
    tbl.set("truncate", ui.lua.create_function(truncate)?)?;

    Ok(())
}

