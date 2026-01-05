use std::str::FromStr;
use serde::{Deserialize, Deserializer, de};
use bstr::BString;
use std::io::Write;
use mlua::prelude::*;
use anyhow::Result;
use crate::ui::{Ui, ThreadsafeUiInner};
use std::time::SystemTime;

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
mod functions;
mod regex;
pub use keybind::KeybindMapping;
pub use events::{EventCallbacks, HasEventCallbacks};

#[derive(Debug, Copy, Clone)]
pub struct SerdeWrap<T>(T);
impl<'de, T: FromStr> Deserialize<'de> for SerdeWrap<T>
    where <T as FromStr>::Err: std::fmt::Display
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let data = String::deserialize(deserializer)?;
        Ok(Self(T::from_str(&data).map_err(de::Error::custom)?))
    }
}

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

async fn set_cursor(ui: Ui, _lua: Lua, val: usize) -> Result<()> {
    ui.get().inner.borrow_mut().await.buffer.set_cursor(val);
    Ok(())
}

async fn set_buffer(ui: Ui, _lua: Lua, (val, len): (mlua::String, Option<usize>)) -> Result<()> {
    ui.get().inner.borrow_mut().await.buffer.splice_at_cursor(&val.as_bytes(), len);
    ui.trigger_buffer_change_callbacks(()).await;
    Ok(())
}

async fn undo_buffer(ui: Ui, _lua: Lua, (): ()) -> Result<()> {
    if ui.get().inner.borrow_mut().await.buffer.move_in_history(false) {
        ui.trigger_buffer_change_callbacks(()).await;
    }
    Ok(())
}

async fn redo_buffer(ui: Ui, _lua: Lua, (): ()) -> Result<()> {
    if ui.get().inner.borrow_mut().await.buffer.move_in_history(true) {
        ui.trigger_buffer_change_callbacks(()).await;
    }
    Ok(())
}

async fn accept_line(mut ui: Ui, _lua: Lua, (): ()) -> Result<bool> {
    ui.accept_line().await
}

async fn redraw(mut ui: Ui, lua: Lua, val: Option<LuaValue>) -> Result<()> {
    if let Some(val) = val {
        let val: RedrawOptions = lua.from_value(val)?;
        let ui = ui.get();
        let mut ui = ui.inner.borrow_mut().await;
        if val.all { ui.dirty = true; }
        if val.prompt { ui.prompt.dirty = true; }
        if val.buffer { ui.buffer.dirty = true; }
        if val.messages { ui.tui.dirty = true; }
        if val.status_bar { ui.status_bar.dirty = true; }
    }

    ui.draw().await
}

async fn exit(ui: Ui, _lua: Lua, code: Option<i32>) -> Result<()> {
    ui.events.read().exit(code.unwrap_or(0)).await;
    Ok(())
}

async fn get_cwd(ui: Ui, _lua: Lua, (): ()) -> Result<BString> {
    Ok(ui.shell.get_cwd().await)
}

async fn call_hook_func(mut ui: Ui, _lua: Lua, mut args: Vec<BString>) -> Result<Option<i32>> {
    let arg0 = args.remove(0);

    let foreground_lock = if !crate::is_forked() {
        // this essentially locks ui
        ui.events.read().pause().await;
        ui.prepare_for_unhandled_output().await?;
        Some(ui.has_foreground_process.lock().await)
    } else {
        None
    };

    let result = ui.shell.call_hook_func(arg0, args).await;

    if foreground_lock.is_some() {
        drop(foreground_lock);
        ui.events.read().resume().await;
        let result = ui.recover_from_unhandled_output().await;
        ui.report_error(result).await;
    }

    Ok(result)
}

async fn print(mut ui: Ui, _lua: Lua, value: BString) -> Result<()> {
    let lock = ui.has_foreground_process.lock().await;
    ui.prepare_for_unhandled_output().await?;
    ui.get().inner.borrow_mut().await.stdout.write_all(value.as_ref())?;
    drop(lock);
    ui.recover_from_unhandled_output().await?;
    ui.try_draw().await;
    Ok(())
}

fn time(_lua: &Lua, (): ()) -> LuaResult<f64> {
    Ok(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs_f64())
}

async fn lua_try(lua: Lua, (func, catch, finally): (LuaFunction, Option<LuaFunction>, Option<LuaFunction>)) -> LuaResult<LuaMultiValue> {
    let mut result = func.call_async(()).await;

    if let Some(catch) = catch {
        result = match result {
            Ok(x) => Ok(x),
            Err(e) => {
                let err = e.clone().into_lua(&lua).unwrap();
                let catch_result: LuaResult<LuaValue> = catch.call_async(err).await;
                match catch_result {
                    Ok(_) => Ok(LuaMultiValue::new()),
                    Err(new_err) if new_err.to_string() == e.to_string() => Err(e),
                    Err(new_err) => Err(new_err.context(e)),
                }
            },
        };
    }


    if let Some(finally) = finally {
        let finally_result: LuaResult<()> = finally.call_async(()).await;
        result = match (result, finally_result) {
            (x, Ok(_)) => x,
            (Ok(_), Err(e)) => Err(e),
            (Err(e1), Err(e2)) => Err(e2.context(e1)),
        };
    }
    result

}

pub async fn __laggy(_ui: Ui, lua: Lua, (): ()) -> Result<()> {
    let _ = tokio::task::spawn_blocking(move || {
        let _: Result<(), mlua::Error> = unsafe{ lua.exec_raw((), |_| {
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
    ui.set_lua_async_fn("exit", exit)?;
    ui.set_lua_async_fn("get_cwd", get_cwd)?;
    ui.set_lua_async_fn("call_hook_func", call_hook_func)?;
    ui.set_lua_async_fn("print", print)?;
    ui.get_lua_api()?.set("time", ui.lua.create_function(time)?)?;
    ui.get_lua_api()?.set("try", ui.lua.create_async_function(lua_try)?)?;
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
    functions::init_lua(ui)?;
    regex::init_lua(ui)?;

    Ok(())
}
