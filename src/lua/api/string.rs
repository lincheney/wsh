use crate::lua::LuaWrapper;
use anyhow::Result;
use mlua::{prelude::*};
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
    let index = index.saturating_sub(1);
    let bytes = string.as_bytes();
    if bytes.is_ascii() {
        let index = (index < bytes.len()).then_some(index + 1);
        Ok((index, index))
    } else if let Some((s, e, _)) = string.as_bytes().grapheme_indices().nth(index.saturating_sub(1)) {
        Ok((Some(s + 1), Some(e)))
    } else {
        Ok((None, None))
    }
}

fn from_byte_pos(_lua: &Lua, (string, index): (mlua::String, usize)) -> LuaResult<Option<usize>> {
    let index = index.saturating_sub(1);
    let bytes = string.as_bytes();
    if bytes.is_ascii() {
        Ok((index < bytes.len()).then_some(index + 1))
    } else {
        Ok(bytes.grapheme_indices().position(|(_, e, _)| e > index).map(|i| i + 1))
    }
}

fn graphemes(lua: &Lua, string: mlua::String) -> LuaResult<LuaTable> {
    let bytes = string.as_bytes();
    let table = lua.create_table()?;
    for (s, e, _) in bytes.grapheme_indices() {
        table.raw_push(lua.create_string(&bytes[s..e])?)?;
    }
    Ok(table)
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    let tbl = lua.create_table()?;
    lua.api.set("str", &tbl)?;

    tbl.set("len", lua.create_function(len)?)?;
    tbl.set("width", lua.create_function(width)?)?;
    tbl.set("to_byte_pos", lua.create_function(to_byte_pos)?)?;
    tbl.set("from_byte_pos", lua.create_function(from_byte_pos)?)?;
    tbl.set("truncate", lua.create_function(truncate)?)?;
    tbl.set("graphemes", lua.create_function(graphemes)?)?;

    Ok(())
}

