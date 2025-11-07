use anyhow::Result;
use mlua::{prelude::*};
use crate::ui::{Ui, ThreadsafeUiInner};
use bstr::*;

fn entry_to_lua(entry: crate::shell::history::Entry, lua: &Lua) -> Result<LuaTable> {
    let t = lua.create_table_with_capacity(0, 4)?;
    t.raw_set("text", entry.text)?;
    t.raw_set("start_time", entry.start_time)?;
    t.raw_set("finish_time", entry.finish_time)?;
    t.raw_set("histnum", entry.histnum)?;
    Ok(t)
}

async fn get_history(ui: Ui, lua: Lua, _val: ()) -> Result<(usize, LuaTable)> {
    let mut shell = ui.shell.lock().await;
    let current = shell.get_histline();

    let tbl = lua.create_table()?;
    for entry in shell.get_history().iter() {
        let entry = entry.as_entry();
        tbl.raw_push(entry_to_lua(entry, &lua)?)?;
    }
    Ok((current as _, tbl))
}

async fn get_history_index(ui: Ui, _lua: Lua, _val: ()) -> Result<usize> {
    Ok(ui.shell.lock().await.get_histline() as _)
}

async fn get_next_history(ui: Ui, lua: Lua, val: usize) -> Result<Option<LuaTable>> {
    let mut shell = ui.shell.lock().await;
    // get the next highest one
    if let Some(entry) = shell.get_history().closest_to((val+1) as _, std::cmp::Ordering::Greater) {
        Ok(Some(entry_to_lua(entry.as_entry(), &lua)?))
    } else {
        Ok(None)
    }
}

async fn get_prev_history(ui: Ui, lua: Lua, val: usize) -> Result<Option<LuaTable>> {
    let mut shell = ui.shell.lock().await;
    // get the next lowest one
    if let Some(entry) = shell.get_history().closest_to((val.saturating_sub(1)) as _, std::cmp::Ordering::Less) {
        Ok(Some(entry_to_lua(entry.as_entry(), &lua)?))
    } else {
        Ok(None)
    }
}

async fn goto_history(ui: Ui, _lua: Lua, val: usize) -> Result<Option<usize>> {
    let mut shell = ui.shell.lock().await;

    let current = shell.get_histline();
    let latest = shell.get_history().first().map(|entry| entry.histnum());

    let mut ui = ui.unlocked.write();
    let mut ui = ui.inner.borrow_mut().await;
    match shell.set_histline(val as _) {
        Some(entry) => {
            if Some(entry.histnum()) == latest {
                // restore history if off history
                ui.buffer.restore();
            } else {
                // save the buffer if moving to another history
                if Some(current.into()) == latest {
                    ui.buffer.save();
                }
                let text = entry.as_entry().text;
                ui.buffer.set(Some(text.as_bytes()), Some(text.len()));
            }
            Ok(Some(entry.histnum() as _))
        },
        None => Ok(None),
    }
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("get_history", get_history)?;
    ui.set_lua_async_fn("get_history_index", get_history_index)?;
    ui.set_lua_async_fn("get_next_history", get_next_history)?;
    ui.set_lua_async_fn("get_prev_history", get_prev_history)?;
    ui.set_lua_async_fn("goto_history", goto_history)?;

    Ok(())
}

