use std::borrow::Cow;
use crate::{meta_str};
use bstr::BString;
use crate::lua::{HasEventCallbacks};
use anyhow::Result;
use mlua::{prelude::*};
use crate::ui::{Ui};
use crate::shell::history::HistoryIndex;

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

    let tbl: Result<_> = ui.shell.run(move |shell| {
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

        let (buffer, cursor) = {
            let ui = ui.unlocked.read();
            let ui = ui.inner.blocking_read();
            (ui.buffer.get_contents().clone(), ui.buffer.get_cursor())
        };

        ui.shell.set_zle_buffer(buffer.clone(), cursor as _).await;
        ui.shell.goto_history(index, false).await;

        let (new_buffer, new_cursor) = ui.shell.get_zle_buffer().await;
        let new_cursor = new_cursor.unwrap_or(new_buffer.len() as _) as _;
        let new_buffer = (new_buffer != *buffer).then_some(new_buffer);
        let new_cursor = (new_cursor != cursor).then_some(new_cursor);

        if new_buffer.is_some() || new_cursor.is_some() {
            let ui = ui.get();
            let mut ui = ui.inner.blocking_write();
            ui.buffer.insert_or_set(new_buffer.as_ref().map(|x| x.as_ref()), new_cursor);
        }

        new_buffer.is_some()
    };
    if changed {
        ui.trigger_buffer_change_callbacks().await;
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

async fn append_history(ui: Ui, _lua: Lua, val: BString) -> Result<()> {
    ui.shell.append_history(val.clone()).await?;
    ui.shell.call_hook_func(Cow::Borrowed(meta_str!(c"zshaddhistory")), vec![val.into()]).await;
    Ok(())
}

async fn append_history_words(ui: Ui, _lua: Lua, val: Vec<BString>) -> Result<()> {
    let chline = bstr::join(b" ", &val);
    ui.shell.append_history_words(val).await?;
    ui.shell.call_hook_func(Cow::Borrowed(meta_str!(c"zshaddhistory")), vec![chline.into()]).await;
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("get_history", get_history)?;
    ui.set_lua_async_fn("get_history_index", get_history_index)?;
    ui.set_lua_async_fn("goto_history", goto_history)?;
    ui.set_lua_async_fn("goto_history_relative", goto_history_relative)?;
    ui.set_lua_async_fn("append_history", append_history)?;
    ui.set_lua_async_fn("append_history_words", append_history_words)?;

    Ok(())
}

