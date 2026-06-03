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

pub fn init_lua(ui: &Ui) -> Result<()> {
    let lua_api = ui.get_lua_api()?;
    let tbl = ui.lua.create_table()?;
    lua_api.set("log", &tbl)?;

    macro_rules! make_logger {
        ($name:ident) => (
            make_logger!($name, $name)
        );
        ($name:ident, $loglevel:ident) => (
            make_logger!($name, $loglevel, 0)
        );
        ($name:ident, $loglevel:ident, $lualevel:expr) => (
            tbl.set(stringify!($name), ui.lua.create_function(|lua, val: LuaValue| {
                let traceback = lua.traceback(None, 1 + $lualevel)?.display().to_string();
                let line = traceback.lines().nth(1).unwrap().trim();
                log::$loglevel!("{} {}", line, LogValue(val)); Ok(())
            })?)?;
        );
    }

    make_logger!(debug);
    make_logger!(info);
    make_logger!(warn);
    make_logger!(error);
    make_logger!(debug1, debug, 1);

    Ok(())
}
