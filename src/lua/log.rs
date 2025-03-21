use anyhow::Result;
use crate::ui::Ui;
use mlua::prelude::*;

struct LogValue(LuaValue);

impl std::fmt::Display for LogValue {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        if let LuaValue::String(string) = &self.0 {
            write!(fmt, "{}", string.display())
        } else {
            write!(fmt, "{:?}", self.0)
        }
    }
}

pub async fn init_lua(ui: &Ui) -> Result<()> {
    let lua_api = ui.get_lua_api()?;
    let tbl = ui.lua.create_table()?;
    lua_api.set("log", &tbl)?;

    tbl.set("debug", ui.lua.create_function(|_, val: LuaValue| { log::debug!("{}", LogValue(val)); Ok(()) })?)?;
    tbl.set("info",  ui.lua.create_function(|_, val: LuaValue| {  log::info!("{}", LogValue(val)); Ok(()) })?)?;
    tbl.set("warn",  ui.lua.create_function(|_, val: LuaValue| {  log::warn!("{}", LogValue(val)); Ok(()) })?)?;
    tbl.set("error", ui.lua.create_function(|_, val: LuaValue| { log::error!("{}", LogValue(val)); Ok(()) })?)?;

    Ok(())
}
