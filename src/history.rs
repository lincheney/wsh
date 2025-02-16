use std::collections::HashMap;
use anyhow::Result;
use mlua::{prelude::*};
use crate::ui::Ui;
use crate::shell::Shell;
use bstr::*;

async fn get_history(_ui: Ui, shell: Shell, _lua: Lua, _val: ()) -> Result<(HashMap<usize, BString>, usize)> {
    let mut shell = shell.lock().await;
    let history: HashMap<_, _> = shell.get_history().entries().map(|e| (e.histnum as _, e.text)).collect();
    let curhist = shell.get_curhist().0;
    Ok((history, curhist as _))
}

async fn get_history_index(_ui: Ui, shell: Shell, _lua: Lua, _val: ()) -> Result<usize> {
    Ok(shell.lock().await.get_curhist().0 as _)
}

async fn get_next_history(_ui: Ui, shell: Shell, _lua: Lua, val: usize) -> Result<(Option<usize>, Option<BString>)> {
    let mut shell = shell.lock().await;
    // get the next highest one
    let value = shell
        .get_history()
        .enumerate()
        .take_while(|(h, _)| *h > val as _)
        .map(|(h, e)| (h as _, e.as_entry().unwrap().text))
        .last()
    ;
    Ok(value.unzip())
}

async fn get_prev_history(_ui: Ui, shell: Shell, _lua: Lua, val: usize) -> Result<(Option<usize>, Option<BString>)> {
    let mut shell = shell.lock().await;
    // get the next lowest one
    let value = shell
        .get_history()
        .enumerate()
        .find(|(h, _)| *h < val as _)
        .map(|(h, e)| (h as _, e.as_entry().unwrap().text))
    ;
    Ok(value.unzip())
}

async fn goto_history(ui: Ui, shell: Shell, _lua: Lua, val: usize) -> Result<(usize, Option<BString>)> {
    let mut shell = shell.lock().await;

    let save = shell.get_curhist().1.is_none();
    let (curhist, entry) = shell.set_curhist(val as _);
    let text = entry.map(|e| e.as_entry().unwrap().text);

    let mut ui = ui.borrow_mut().await;
    // save the buffer if moving to another history
    if save {
        ui.buffer.save();
    }

    if let Some(text) = &text {
        ui.buffer.mutate(|buffer, cursor| {
            buffer.resize(text.len(), 0);
            buffer.copy_from_slice(text.as_bytes());
            *cursor = buffer.len();
        });
    } else {
        // restore history if off history
        ui.buffer.restore();
    }

    Ok((curhist as _, text))
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("get_history", shell, get_history).await?;
    ui.set_lua_async_fn("get_history_index", shell, get_history_index).await?;
    ui.set_lua_async_fn("get_next_history", shell, get_next_history).await?;
    ui.set_lua_async_fn("get_prev_history", shell, get_prev_history).await?;
    ui.set_lua_async_fn("goto_history", shell, goto_history).await?;

    Ok(())
}

