use anyhow::Result;
use mlua::{prelude::*};
use crate::ui::Ui;
use crate::shell::Shell;
use bstr::{ByteSlice};

fn slice(string: &LuaString, start: usize, end: Option<usize>) -> Option<std::ops::Range<usize>> {
    let bytes = string.as_bytes();
    let end = end.unwrap_or(start + 1);
    let mut slice = bytes.grapheme_indices().skip(start).take(end.saturating_sub(start));

    match (slice.next(), slice.last()) {
        (Some((s, _, _)), Some((_, e, _))) => Some(s..e),
        (Some((s, e, _)), None) => Some(s..e),
        (None, _) => return None,
    }
}

fn get(lua: &Lua, (string, start, end): (LuaString, usize, Option<usize>)) -> LuaResult<LuaValue> {
    if let Some(range) = slice(&string, start, end) {
        Ok(mlua::Value::String(lua.create_string(&string.as_bytes()[range])?))
    } else {
        Ok(mlua::Value::Nil)
    }
}

fn set(lua: &Lua, (string, replace, start, end): (LuaString, Option<LuaString>, usize, Option<usize>)) -> LuaResult<LuaString> {
    if let Some(range) = slice(&string, start, end) {
        let mut new = string.as_bytes()[..range.start].to_owned();
        if let Some(replace) = replace {
            new.extend(replace.as_bytes());
        }
        new.extend(&string.as_bytes()[range.end..]);
        Ok(lua.create_string(new)?)
    } else {
        Ok(string)
    }
}

fn len(_lua: &Lua, string: mlua::String) -> LuaResult<usize> {
    Ok(string.as_bytes().grapheme_indices().count())
}

fn to_byte_pos(_lua: &Lua, (string, index): (mlua::String, usize)) -> LuaResult<(Option<usize>, Option<usize>)> {
    Ok(if let Some((s, e, _)) = string.as_bytes().grapheme_indices().nth(index) {
        (Some(s), Some(e))
    } else {
        (None, None)
    })
}

fn from_byte_pos(_lua: &Lua, (string, index): (mlua::String, usize)) -> LuaResult<Option<usize>> {
    for (i, (_, e, _)) in string.as_bytes().grapheme_indices().enumerate() {
        if e > index {
            return Ok(Some(i))
        }
    }
    Ok(None)
}

pub async fn init_lua(ui: &Ui, _shell: &Shell) -> Result<()> {

    let ui = ui.borrow().await;
    let string = ui.lua.create_table()?;
    ui.lua_api.set("str", &string)?;

    string.set("get", ui.lua.create_function(get)?)?;
    string.set("set", ui.lua.create_function(set)?)?;
    string.set("len", ui.lua.create_function(len)?)?;
    string.set("to_byte_pos", ui.lua.create_function(to_byte_pos)?)?;
    string.set("from_byte_pos", ui.lua.create_function(from_byte_pos)?)?;

    Ok(())
}

