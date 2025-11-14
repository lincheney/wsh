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
    let current = ui.shell.get_histline().await;

    let tbl: Result<_> = ui.shell.do_run(move |shell| {
        let tbl = lua.create_table()?;
        let history = crate::shell::history::History::get(shell);
        for entry in history.iter() {
            let entry = entry.as_entry();
            tbl.raw_push(entry_to_lua(entry, &lua)?)?;
        }
        Ok(tbl)
    }).await;
    Ok((current as _, tbl?))
}

async fn get_history_index(ui: Ui, _lua: Lua, _val: ()) -> Result<usize> {
    Ok(ui.shell.get_histline().await as _)
}

async fn get_next_history(ui: Ui, lua: Lua, val: usize) -> Result<Option<LuaTable>> {
    // get the next highest one

    ui.shell.do_run(move |shell| {
        let history = crate::shell::history::History::get(shell);
        if let Some(entry) = history.closest_to((val+1) as _, std::cmp::Ordering::Greater) {
            entry_to_lua(entry.as_entry(), &lua).map(|entry| Some(entry))
        } else {
            Ok(None)
        }
    }).await
}

async fn get_prev_history(ui: Ui, lua: Lua, val: usize) -> Result<Option<LuaTable>> {
    ui.shell.do_run(move |shell| {
        let history = crate::shell::history::History::get(shell);
        // get the next lowest one
        if let Some(entry) = history.closest_to((val.saturating_sub(1)) as _, std::cmp::Ordering::Less) {
            Ok(Some(entry_to_lua(entry.as_entry(), &lua)?))
        } else {
            Ok(None)
        }
    }).await
}

async fn goto_history(ui: Ui, _lua: Lua, val: usize) -> Result<Option<usize>> {
    let current = ui.shell.get_histline().await;
    let result = ui.shell.do_run(move |shell| {
        let mut history = crate::shell::history::History::get(shell);
        let latest = history.first().map(|entry| entry.histnum());

        match history.set_histline(val as _) {
            Some(entry) => Some((latest, entry.histnum(), entry.as_entry().text)),
            None => None,
        }
    }).await;

    if let Some((latest, histnum, text)) = result {
        let ui = ui.unlocked.read();
        let mut ui = ui.inner.borrow_mut().await;
        if Some(histnum) == latest {
            // restore history if off history
            ui.buffer.restore();
        } else {
            // save the buffer if moving to another history
            if Some(current.into()) == latest {
                ui.buffer.save();
            }
            ui.buffer.set(Some(text.as_bytes()), Some(text.len()));
        }
        Ok(Some(histnum as _))
    } else {
        Ok(None)
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

