use anyhow::Result;
use crate::ui::Ui;
use crate::shell::Shell;

pub async fn init_lua(ui: &Ui, _shell: &Shell) -> Result<()> {
    let ui = ui.borrow().await;
    let tbl = ui.lua.create_table()?;
    ui.lua_api.set("log", &tbl)?;

    tbl.set("debug", ui.lua.create_function(|_, val: ()| { log::debug!("{:?}", val); Ok(()) })?)?;
    tbl.set("info",  ui.lua.create_function(|_, val: ()| {  log::info!("{:?}", val); Ok(()) })?)?;
    tbl.set("warn",  ui.lua.create_function(|_, val: ()| {  log::warn!("{:?}", val); Ok(()) })?)?;
    tbl.set("error", ui.lua.create_function(|_, val: ()| { log::error!("{:?}", val); Ok(()) })?)?;

    Ok(())
}
