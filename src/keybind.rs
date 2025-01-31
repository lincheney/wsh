use std::collections::HashMap;
use anyhow::Result;
use mlua::{prelude::*, Function};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

pub type KeybindMapping = HashMap<usize, Function>;

fn set_keymap(_lua: &Lua, (keys, callback): (String, Function)) -> LuaResult<()> {
    println!("hello, {}!", keys);
    Ok(())
}

pub fn init_lua(ui: &crate::ui::Ui) -> Result<()> {

    ui.lua_api.set("set_keymap", ui.lua.create_function(set_keymap)?)?;

    Ok(())
}
