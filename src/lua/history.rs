use crate::lua::{HasEventCallbacks};
use anyhow::Result;
use mlua::{prelude::*};
use crate::ui::{Ui, ThreadsafeUiInner};
use crate::shell::history::HistoryIndex;
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

async fn goto_history_internal(ui: Ui, index: HistoryIndex) {
    let changed = {
        let shell = &ui.shell;
        let ui = ui.unlocked.read();

        let mut ui = ui.inner.write().await;
        let buffer = ui.buffer.get_contents();
        let cursor = ui.buffer.get_cursor();

        shell.set_zle_buffer(buffer.clone(), cursor as _).await;
        shell.goto_history(index, false).await;

        let (new_buffer, new_cursor) = shell.get_zle_buffer().await;
        let new_cursor = new_cursor.unwrap_or(new_buffer.len() as _) as _;
        let new_buffer = (new_buffer != *buffer).then_some(new_buffer);
        let new_cursor = (new_cursor != cursor).then_some(new_cursor);
        ui.buffer.insert_or_set(new_buffer.as_ref().map(|x| x.as_ref()), new_cursor);

        new_buffer.is_some()
    };
    if changed {
        ui.trigger_buffer_change_callbacks(()).await;
    }
}

async fn goto_history(ui: Ui, _lua: Lua, val: i32) -> Result<()> {
    goto_history_internal(ui, HistoryIndex::Absolute(val)).await;
    Ok(())
}

async fn goto_history_relative(ui: Ui, _lua: Lua, val: i32) -> Result<()> {
    goto_history_internal(ui, HistoryIndex::Relative(val)).await;
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("get_history", get_history)?;
    ui.set_lua_async_fn("get_history_index", get_history_index)?;
    ui.set_lua_async_fn("goto_history", goto_history)?;
    ui.set_lua_async_fn("goto_history_relative", goto_history_relative)?;

    Ok(())
}

