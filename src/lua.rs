use bstr::BString;
use mlua::prelude::*;
use serde::{Deserialize};
use anyhow::Result;
use crate::ui::{Ui, ThreadsafeUiInner};

mod keybind;
mod string;
mod completion;
mod history;
mod events;
mod tui;
mod log;
mod process;
mod asyncio;
mod parser;
mod variables;
pub use keybind::KeybindMapping;
pub use events::{EventCallbacks, HasEventCallbacks};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RedrawOptions {
    prompt: bool,
    buffer: bool,
    messages: bool,
    status_bar: bool,
    all: bool,
}

async fn get_cursor(ui: Ui, _lua: Lua, _: ()) -> Result<usize> {
    Ok(ui.get().inner.borrow().await.buffer.get_cursor())
}

async fn get_buffer(ui: Ui, lua: Lua, _: ()) -> Result<mlua::String> {
    Ok(lua.create_string(ui.get().inner.borrow().await.buffer.get_contents())?)
}

async fn set_cursor(mut ui: Ui, _lua: Lua, val: usize) -> Result<()> {
    ui.get_mut().inner.borrow_mut().await.buffer.set_cursor(val);
    Ok(())
}

async fn set_buffer(mut ui: Ui, _lua: Lua, (val, len): (mlua::String, Option<usize>)) -> Result<()> {
    ui.get_mut().inner.borrow_mut().await.buffer.splice_at_cursor(&val.as_bytes(), len);
    ui.trigger_buffer_change_callbacks(()).await;
    Ok(())
}

async fn undo_buffer(mut ui: Ui, _lua: Lua, (): ()) -> Result<()> {
    if ui.get_mut().inner.borrow_mut().await.buffer.move_in_history(false) {
        ui.trigger_buffer_change_callbacks(()).await;
    }
    Ok(())
}

async fn redo_buffer(mut ui: Ui, _lua: Lua, (): ()) -> Result<()> {
    if ui.get_mut().inner.borrow_mut().await.buffer.move_in_history(true) {
        ui.trigger_buffer_change_callbacks(()).await;
    }
    Ok(())
}

async fn accept_line(mut ui: Ui, _lua: Lua, (): ()) -> Result<bool> {
    ui.accept_line().await
}

async fn redraw(mut ui: Ui, lua: Lua, val: Option<LuaValue>) -> Result<()> {
    if let Some(val) = val {
        let mut ui = ui.get_mut();
        let val: RedrawOptions = lua.from_value(val)?;
        let mut ui = ui.inner.borrow_mut().await;
        if val.all { ui.dirty = true; }
        if val.prompt { ui.prompt.dirty = true; }
        if val.buffer { ui.buffer.dirty = true; }
        if val.messages { ui.tui.dirty = true; }
        if val.status_bar { ui.status_bar.dirty = true; }
    }

    ui.draw().await
}

async fn eval(ui: Ui, lua: Lua, (cmd, stderr): (mlua::String, bool)) -> Result<mlua::String> {
    let data = ui.shell.lock().await.eval((*cmd.as_bytes()).into(), stderr).unwrap();
    Ok(lua.create_string(data)?)
}

async fn exit(mut ui: Ui, _lua: Lua, code: Option<i32>) -> Result<()> {
    let mut ui = ui.get_mut();
    let mut ui = ui.inner.borrow_mut().await;
    ui.events.exit(code.unwrap_or(0)).await;
    Ok(())
}

async fn get_cwd(ui: Ui, _lua: Lua, (): ()) -> Result<BString> {
    Ok(ui.shell.lock().await.get_cwd())
}

pub async fn __laggy(_ui: Ui, _lua: Lua, (): ()) -> Result<()> {
    let _ = tokio::task::spawn_blocking(move || {
        let _: Result<(), mlua::Error> = unsafe{ _lua.exec_raw((), |_| {
            // let lock = ui.inner.borrow_mut().await;
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    for i in 0..2 {
                        eprintln!("DEBUG(likes) \t{}\t= {:?}", stringify!(i), i);
                        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                    }
                });
            });
        }) };
    }).await;
    // drop(lock);
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("get_cursor", get_cursor)?;
    ui.set_lua_async_fn("get_buffer", get_buffer)?;
    ui.set_lua_async_fn("set_cursor", set_cursor)?;
    ui.set_lua_async_fn("set_buffer", set_buffer)?;
    ui.set_lua_async_fn("undo_buffer", undo_buffer)?;
    ui.set_lua_async_fn("redo_buffer", redo_buffer)?;
    ui.set_lua_async_fn("accept_line", accept_line)?;
    ui.set_lua_async_fn("redraw",  redraw)?;
    ui.set_lua_async_fn("eval", eval)?;
    ui.set_lua_async_fn("exit", exit)?;
    ui.set_lua_async_fn("get_cwd", get_cwd)?;
    ui.set_lua_async_fn("__laggy", __laggy)?;

    keybind::init_lua(ui)?;
    string::init_lua(ui)?;
    completion::init_lua(ui)?;
    history::init_lua(ui)?;
    events::init_lua(ui)?;
    tui::init_lua(ui)?;
    log::init_lua(ui)?;
    asyncio::init_lua(ui)?;
    process::init_lua(ui)?;
    parser::init_lua(ui)?;
    variables::init_lua(ui)?;

    Ok(())
}
