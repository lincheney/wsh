use anyhow::Result;
use mlua::{prelude::*, Function};
use serde::{Deserialize, Serialize};
use crate::ui::Ui;
use crate::shell::Shell;
use crossterm::event;


fn async_spawn(ui: &Ui, shell: &Shell, _lua: &Lua, cb: Function) -> Result<()> {
    ui.call_lua_fn(shell.clone(), false, cb, ());
    Ok(())
}

async fn async_sleep(_ui: Ui, _shell: Shell, _lua: Lua, millis: u64) -> Result<()> {
    async_std::task::sleep(std::time::Duration::from_millis(millis)).await;
    Ok(())
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_fn("__async_spawn", shell, async_spawn).await?;
    ui.set_lua_async_fn("__async_sleep", shell, async_sleep).await?;
    ui.set_lua_fn("__async_spawn", shell, async_spawn).await?;

    let ui = ui.borrow().await;
    let log = ui.lua.create_table()?;
    ui.lua_api.set("log", &log)?;

    log.set("debug", ui.lua.create_function(|_, val: LuaValue| { log::debug!("{:?}", val); Ok(()) })?)?;
    log.set("info", ui.lua.create_function(|_, val: LuaValue| { log::info!("{:?}", val); Ok(()) })?)?;
    log.set("warn", ui.lua.create_function(|_, val: LuaValue| { log::warn!("{:?}", val); Ok(()) })?)?;
    log.set("error", ui.lua.create_function(|_, val: LuaValue| { log::error!("{:?}", val); Ok(()) })?)?;

    Ok(())
}
