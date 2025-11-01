use anyhow::Result;
use mlua::{prelude::*};
use crate::ui::{Ui, ThreadsafeUiInner};
use bstr::*;

async fn get_history(ui: Ui, _lua: Lua, _val: ()) -> Result<(usize, Vec<usize>, Vec<BString>)> {
    let mut shell = ui.shell.lock().await;
    let curhist = shell.get_curhist().0;
    let mut histnums = vec![];
    let mut text = vec![];

    for e in shell.get_history().entries() {
        histnums.push(e.histnum as _);
        text.push(e.text);
    }
    Ok((curhist as _, histnums, text))
}

async fn get_history_index(ui: Ui, _lua: Lua, _val: ()) -> Result<usize> {
    Ok(ui.shell.lock().await.get_curhist().0 as _)
}

async fn get_next_history(ui: Ui, _lua: Lua, val: usize) -> Result<(Option<usize>, Option<BString>)> {
    let mut shell = ui.shell.lock().await;
    // get the next highest one
    let value = shell
        .get_history()
        .entries()
        .take_while(|e| e.histnum > val as _)
        .map(|e| (e.histnum as _, e.text))
        .last()
    ;
    Ok(value.unzip())
}

async fn get_prev_history(ui: Ui, _lua: Lua, val: usize) -> Result<(Option<usize>, Option<BString>)> {
    let mut shell = ui.shell.lock().await;
    // get the next lowest one
    let value = shell
        .get_history()
        .entries()
        .find(|e| e.histnum < val as _)
        .map(|e| (e.histnum as _, e.text))
    ;
    Ok(value.unzip())
}

async fn goto_history(mut ui: Ui, _lua: Lua, val: usize) -> Result<(usize, Option<BString>)> {
    let mut shell = ui.shell.lock().await;

    let save = shell.get_curhist().1.is_none();
    let (curhist, entry) = shell.set_curhist(val as _);
    let text = entry.map(|e| crate::shell::history::Entry::from(e).text);

    let mut ui = ui.inner.borrow_mut().await;
    // save the buffer if moving to another history
    if save {
        ui.buffer.save();
    }

    if let Some(text) = &text {
        ui.buffer.set(Some(text), Some(text.len()));
    } else {
        // restore history if off history
        ui.buffer.restore();
    }

    Ok((curhist as _, text))
}

pub async fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("get_history", get_history)?;
    ui.set_lua_async_fn("get_history_index", get_history_index)?;
    ui.set_lua_async_fn("get_next_history", get_next_history)?;
    ui.set_lua_async_fn("get_prev_history", get_prev_history)?;
    ui.set_lua_async_fn("goto_history", goto_history)?;

    Ok(())
}

