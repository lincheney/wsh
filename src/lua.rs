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

    Ok(())
}
